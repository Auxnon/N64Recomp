//! ELF input (`src/elf.cpp`), with a small big-endian ELF32 reader in place of ELFIO.
//!
//! The reader keeps ELFIO's behaviors that the recompiler depends on: every section header
//! is a section (index 0 included), NULL and NOBITS sections have no data, symbol names
//! come from the linked string table, and symbol/relocation counts are `size / entsize`.

use std::collections::HashMap;
use std::path::Path;

use recomp_core::cxx_sort;

use crate::context::{
    Context, DataSymbol, DataSymbolMap, ElfParsingConfig, Function, Reloc, RelocType,
    SECTION_ABSOLUTE, read_rom_word, read_rom_words, write_rom_word,
};
use crate::mdebug;
use crate::symbol_lists::{IGNORED_FUNCS, REIMPLEMENTED_FUNCS, RENAMED_FUNCS};

const SHT_NULL: u32 = 0;
const SHT_SYMTAB: u32 = 2;
const SHT_NOBITS: u32 = 8;
const SHT_REL: u32 = 9;
const SHT_MIPS_DEBUG: u32 = 0x7000_0005;
const SHF_ALLOC: u32 = 0x2;
const SHF_EXECINSTR: u32 = 0x4;
const SHN_UNDEF: u16 = 0;
const SHN_ABS: u16 = 0xFFF1;
const STB_LOCAL: u8 = 0;
const STT_NOTYPE: u8 = 0;
const STT_OBJECT: u8 = 1;
const STT_FUNC: u8 = 2;
const STV_INTERNAL: u8 = 1;

pub struct ElfSection {
    pub index: u16,
    pub name: String,
    pub kind: u32,
    pub flags: u32,
    pub addr: u32,
    pub offset: u32,
    pub size: u32,
    pub link: u32,
    pub entsize: u32,
    /// Empty for NULL and NOBITS sections.
    pub data: Vec<u8>,
}

pub struct ElfSegment {
    pub offset: u32,
    pub paddr: u32,
    pub filesz: u32,
}

pub struct ElfSymbol {
    pub name: String,
    pub value: u32,
    pub size: u32,
    pub bind: u8,
    pub kind: u8,
    pub shndx: u16,
    pub other: u8,
}

pub struct Elf {
    pub sections: Vec<ElfSection>,
    pub segments: Vec<ElfSegment>,
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// ELFIO's `string_section_accessor::get_string`: a NUL-terminated string inside the section.
fn get_string(section: &ElfSection, index: u32) -> Option<String> {
    let rest = section.data.get(index as usize..)?;
    let len = rest.iter().position(|&b| b == 0)?;
    Some(String::from_utf8_lossy(&rest[..len]).into_owned())
}

pub enum ElfError {
    NotFound,
    WrongClass,
    WrongEndianness,
    Malformed(&'static str),
}

impl Elf {
    pub fn parse(file: &[u8]) -> Result<Elf, ElfError> {
        if file.get(..4) != Some(b"\x7fELF") {
            return Err(ElfError::Malformed("not an ELF file"));
        }
        if file.get(4) != Some(&1) {
            return Err(ElfError::WrongClass);
        }
        if file.get(5) != Some(&2) {
            return Err(ElfError::WrongEndianness);
        }
        let header = |at| be32(file, at).ok_or(ElfError::Malformed("truncated header"));
        let header16 = |at| be16(file, at).ok_or(ElfError::Malformed("truncated header"));
        let phoff = header(0x1C)? as usize;
        let shoff = header(0x20)? as usize;
        let phentsize = header16(0x2A)? as usize;
        let phnum = header16(0x2C)? as usize;
        let shentsize = header16(0x2E)? as usize;
        let shnum = header16(0x30)? as usize;
        let shstrndx = header16(0x32)?;

        let mut sections = Vec::with_capacity(shnum);
        for index in 0..shnum {
            let at = shoff + index * shentsize;
            let field =
                |off| be32(file, at + off).ok_or(ElfError::Malformed("truncated section header"));
            let (name_offset, kind, flags, addr, offset, size, link, entsize) = (
                field(0)?,
                field(4)?,
                field(8)?,
                field(12)?,
                field(16)?,
                field(20)?,
                field(24)?,
                field(36)?,
            );
            let data = if kind != SHT_NULL && kind != SHT_NOBITS && (size as usize) < file.len() {
                file.get(offset as usize..offset as usize + size as usize)
                    .ok_or(ElfError::Malformed("section data out of bounds"))?
                    .to_vec()
            } else {
                Vec::new()
            };
            sections.push((
                name_offset,
                ElfSection {
                    index: index as u16,
                    name: String::new(),
                    kind,
                    flags,
                    addr,
                    offset,
                    size,
                    link,
                    entsize,
                    data,
                },
            ));
        }
        // Section names come from the section header string table.
        if shstrndx != SHN_UNDEF {
            let names: Vec<Option<String>> = sections
                .iter()
                .map(|(name_offset, _)| {
                    sections
                        .get(usize::from(shstrndx))
                        .and_then(|(_, strtab)| get_string(strtab, *name_offset))
                })
                .collect();
            for ((_, section), name) in sections.iter_mut().zip(names) {
                if let Some(name) = name {
                    section.name = name;
                }
            }
        }
        let sections = sections.into_iter().map(|(_, section)| section).collect();

        let mut segments = Vec::with_capacity(phnum);
        for index in 0..phnum {
            let at = phoff + index * phentsize;
            let field =
                |off| be32(file, at + off).ok_or(ElfError::Malformed("truncated program header"));
            segments.push(ElfSegment {
                offset: field(4)?,
                paddr: field(12)?,
                filesz: field(16)?,
            });
        }

        Ok(Elf { sections, segments })
    }

    fn entry_count(section: &ElfSection) -> usize {
        section.size.checked_div(section.entsize).unwrap_or(0) as usize
    }

    pub fn symbol_count(&self, symtab: &ElfSection) -> usize {
        Self::entry_count(symtab)
    }

    /// ELFIO's `symbol_section_accessor::get_symbol`.
    pub fn symbol(&self, symtab: &ElfSection, index: usize) -> Option<ElfSymbol> {
        if index >= self.symbol_count(symtab) {
            return None;
        }
        let at = index * symtab.entsize as usize;
        let data = &symtab.data;
        let name_offset = be32(data, at)?;
        let info = *data.get(at + 12)?;
        let name = self
            .sections
            .get(symtab.link as usize)
            .and_then(|strtab| get_string(strtab, name_offset))
            .unwrap_or_default();
        Some(ElfSymbol {
            name,
            value: be32(data, at + 4)?,
            size: be32(data, at + 8)?,
            bind: info >> 4,
            kind: info & 0xF,
            other: *data.get(at + 13)?,
            shndx: be16(data, at + 14)?,
        })
    }

    pub fn rel_count(&self, rel: &ElfSection) -> usize {
        Self::entry_count(rel)
    }

    /// An `Elf32_Rel` entry as (offset, symbol, type).
    pub fn rel(&self, rel: &ElfSection, index: usize) -> (u32, u32, u8) {
        let at = index * rel.entsize as usize;
        let offset = be32(&rel.data, at).expect("relocation out of bounds");
        let info = be32(&rel.data, at + 4).expect("relocation out of bounds");
        (offset, info >> 8, info as u8)
    }
}

fn get_segment(segments: &[ElfSegment], section_size: u32, section_offset: u32) -> Option<usize> {
    // A linear search, as segments may overlap.
    segments.iter().position(|segment| {
        let (offset, size) = (u64::from(section_offset), u64::from(section_size));
        offset >= u64::from(segment.offset)
            && offset + size <= u64::from(segment.offset) + u64::from(segment.filesz)
    })
}

/// `read_sections`: fills in sections, builds the ROM image and reads relocations. Returns
/// the indices of the symbol table and mdebug sections.
fn read_sections(
    context: &mut Context,
    elf_config: &ElfParsingConfig,
    elf: &Elf,
) -> Result<(usize, Option<usize>), String> {
    let mut symtab_index = None;
    let mut mdebug_index = None;
    let has_reference_symbols = context.has_reference_symbols();
    let mut reloc_sections_by_name: HashMap<String, usize> = HashMap::new();
    let mut bss_sections_by_name: HashMap<String, usize> = HashMap::new();

    // First pass: find load addresses and the minimum one (like objcopy's raw binary output).
    let mut min_load_address = u32::MAX;
    for section in &elf.sections {
        let section_out = &mut context.sections[usize::from(section.index)];
        // Sections in the ROM aren't NOBITS, have the alloc flag and a nonzero size.
        if section.kind != SHT_NOBITS && section.flags & SHF_ALLOC != 0 && section.size != 0 {
            let Some(segment_index) = get_segment(&elf.segments, section.size, section.offset)
            else {
                return Err(format!(
                    "Could not find segment that section {} belongs to!",
                    section.name
                ));
            };
            let segment = &elf.segments[segment_index];
            // Adjusted by the minimum load address in the next pass.
            section_out.rom_addr = segment
                .paddr
                .wrapping_add(section.offset.wrapping_sub(segment.offset));
            min_load_address = min_load_address.min(section_out.rom_addr);
        } else {
            section_out.rom_addr = u32::MAX;
        }
    }

    // Second pass: record addresses, find special sections, and copy data into the ROM.
    for section in &elf.sections {
        let index = usize::from(section.index);
        let section_out = &mut context.sections[index];
        section_out.ram_addr = section.addr;
        section_out.size = section.size;
        let name = &section.name;

        if section.kind == SHT_SYMTAB {
            symtab_index = Some(index);
        }
        // There should be at most one mdebug section.
        if section.kind == SHT_MIPS_DEBUG {
            mdebug_index = Some(index);
        }
        if elf_config.all_sections_relocatable || elf_config.relocatable_sections.contains(name) {
            section_out.relocatable = true;
        }

        if section.kind == SHT_REL {
            // FIXME (from the C++): this should use sh_info rather than the name.
            let Some(target) = name.strip_prefix(".rel") else {
                return Err(format!(
                    "Could not determine corresponding section for reloc section {name}"
                ));
            };
            // Record relocs for relocatable sections, or for all sections when reference
            // symbols are in use.
            let target_relocatable = elf_config.all_sections_relocatable
                || elf_config.relocatable_sections.contains(target);
            if has_reference_symbols || target_relocatable {
                reloc_sections_by_name.insert(target.to_owned(), index);
            }
        }

        // NOBITS sections ending with the bss suffix pair with the section they're named after.
        if section.kind == SHT_NOBITS && name.ends_with(&elf_config.bss_section_suffix) {
            let target = &name[..name.len() - elf_config.bss_section_suffix.len()];
            if !target.is_empty() {
                bss_sections_by_name.insert(target.to_owned(), index);
            }
        }

        if section_out.rom_addr != u32::MAX {
            section_out.rom_addr = section_out.rom_addr.wrapping_sub(min_load_address);
            let rom_addr = section_out.rom_addr as usize;
            let required = section_out.rom_addr.wrapping_add(section_out.size) as usize;
            if required > context.rom.len() {
                context.rom.resize(required, 0);
            }
            context.rom[rom_addr..rom_addr + section.data.len()].copy_from_slice(&section.data);
        }
        let section_out = &mut context.sections[index];
        if section.flags & SHF_EXECINSTR != 0 {
            section_out.executable = true;
        }
        section_out.name = name.clone();
    }

    let Some(symtab_index) = symtab_index else {
        return Err("No symtab section found".into());
    };
    let symtab = &elf.sections[symtab_index];

    // TODO (from the C++): make sure a reloc section was found for every relocatable section.

    for section_index in 0..context.sections.len() {
        // Pair this section with its bss section.
        if let Some(&bss_index) = bss_sections_by_name.get(&context.sections[section_index].name) {
            let section_out = &mut context.sections[section_index];
            section_out.bss_section_index = bss_index as u16;
            section_out.bss_size = elf.sections[bss_index].size;
            context
                .bss_section_to_section
                .insert(bss_index as u16, section_index as u16);
        }

        // Only sections in the ROM that are relocatable get relocations.
        let elf_section = &elf.sections[section_index];
        let in_rom = elf_section.kind != SHT_NOBITS && elf_section.flags & SHF_ALLOC != 0;
        let is_relocatable =
            context.sections[section_index].relocatable || context.has_reference_symbols();
        if !(in_rom && is_relocatable) {
            continue;
        }

        if let Some(&rel_index) = reloc_sections_by_name.get(&context.sections[section_index].name)
        {
            read_relocs(
                context,
                elf_config,
                elf,
                symtab,
                section_index,
                &elf.sections[rel_index],
            )?;
        }

        // Sort by address. Pairing has already folded each HI16's full immediate in, so a
        // HI16 doesn't need to precede its LO16 anymore.
        cxx_sort::sort_by(&mut context.sections[section_index].relocs, |a, b| {
            a.address < b.address
        });

        // Patch the ROM for HI16/LO16 reference symbol relocs to non-relocatable sections.
        for reloc_index in 0..context.sections[section_index].relocs.len() {
            let section_out = &context.sections[section_index];
            let reloc = section_out.relocs[reloc_index];
            if !(reloc.reference_symbol
                && matches!(reloc.kind, RelocType::R_MIPS_HI16 | RelocType::R_MIPS_LO16))
            {
                continue;
            }
            if context.is_reference_section_relocatable(reloc.target_section) {
                continue;
            }
            let reloc_rom_addr = reloc
                .address
                .wrapping_sub(section_out.ram_addr)
                .wrapping_add(section_out.rom_addr) as usize;
            let reloc_rom_word = read_rom_word(&context.rom, reloc_rom_addr).swap_bytes();
            let full_immediate = reloc
                .target_section_offset
                .wrapping_add(context.get_reference_section_vram(reloc.target_section));
            let imm = if reloc.kind == RelocType::R_MIPS_HI16 {
                (full_immediate >> 16).wrapping_add((full_immediate >> 15) & 1)
            } else {
                full_immediate & 0xFFFF
            };
            write_rom_word(
                &mut context.rom,
                reloc_rom_addr,
                (reloc_rom_word | imm).swap_bytes(),
            );
            // Remove the reloc.
            let reloc = &mut context.sections[section_index].relocs[reloc_index];
            reloc.kind = RelocType::R_MIPS_NONE;
            reloc.reference_symbol = false;
            reloc.symbol_index = u32::MAX;
        }
    }

    Ok((symtab_index, mdebug_index))
}

/// Reads one section's relocations, pairing HI16/LO16 per the MIPS System V ABI (page 4-18,
/// https://refspecs.linuxfoundation.org/elf/mipsabi.pdf).
fn read_relocs(
    context: &mut Context,
    elf_config: &ElfParsingConfig,
    elf: &Elf,
    symtab: &ElfSection,
    section_index: usize,
    rel_section: &ElfSection,
) -> Result<(), String> {
    let count = elf.rel_count(rel_section);
    context.sections[section_index].relocs = vec![Reloc::default(); count];
    // Consecutive identical HI16s are allowed (a GNU extension to the o32 ABI).
    let mut prev_hi_count = 0usize;
    let mut prev_lo = false;
    let mut prev_hi_immediate = 0u32;
    let mut prev_hi_symbol = u32::MAX;

    for i in 0..count {
        let (rel_offset, rel_symbol, rel_type) = elf.rel(rel_section, i);
        let (section_rom, section_ram, bss_index, section_name) = {
            let s = &context.sections[section_index];
            (s.rom_addr, s.ram_addr, s.bss_section_index, s.name.clone())
        };

        // The instruction's immediate holds the addend.
        let reloc_rom_addr = section_rom
            .wrapping_add(rel_offset)
            .wrapping_sub(section_ram) as usize;
        let reloc_rom_word = read_rom_word(&context.rom, reloc_rom_addr).swap_bytes();

        let mut reloc = Reloc {
            address: rel_offset,
            symbol_index: rel_symbol,
            kind: RelocType(rel_type),
            ..Reloc::default()
        };

        let symbol = elf
            .symbol(symtab, rel_symbol as usize)
            .ok_or_else(|| format!("Invalid symbol index {rel_symbol}"))?;
        let mut rel_symbol_section_index = symbol.shndx;
        let mut rel_section_vram = 0u32;
        let mut rel_symbol_offset = 0u32;

        // Remap relocations from this section's bss section to the section itself.
        // TODO (from the C++): do this for any bss section?
        if rel_symbol_section_index == bss_index {
            rel_symbol_section_index = section_index as u16;
        }

        if rel_symbol_section_index == SHN_UNDEF {
            // Undefined: look it up in the reference symbols.
            let Some(sym_ref) = context.find_reference_symbol(&symbol.name) else {
                return Err(format!(
                    "Undefined symbol: {}, not found in input or reference symbols!",
                    symbol.name
                ));
            };
            reloc.reference_symbol = true;
            // Point the reloc at the reference symbol instead.
            reloc.target_section = sym_ref.section_index;
            reloc.symbol_index = sym_ref.symbol_index as u32;
            rel_symbol_offset = context
                .get_reference_symbol(reloc.target_section, sym_ref.symbol_index)
                .section_offset;

            if reloc.kind == RelocType::R_MIPS_32
                && context.is_reference_section_relocatable(reloc.target_section)
            {
                return Err(format!(
                    "Cannot reference {} in a statically initialized variable as it's defined in a relocatable section!",
                    symbol.name
                ));
            }
        } else if rel_symbol_section_index == SHN_ABS {
            reloc.reference_symbol = false;
            reloc.target_section = SECTION_ABSOLUTE;
        } else {
            reloc.reference_symbol = false;
            reloc.target_section = rel_symbol_section_index;
            if usize::from(rel_symbol_section_index) >= context.sections.len() {
                return Err(format!(
                    "Reloc {i} references symbol {} which is in an unknown section 0x{rel_symbol_section_index:04X}!",
                    symbol.name
                ));
            }
            rel_section_vram = context.sections[usize::from(rel_symbol_section_index)].ram_addr;
        }

        let relocs = &mut context.sections[section_index].relocs;

        if reloc.kind == RelocType::R_MIPS_LO16 {
            let rel_immediate = reloc_rom_word & 0xFFFF;
            let lo = rel_immediate as u16 as i16 as i32 as u32;
            let full_immediate = (prev_hi_immediate << 16).wrapping_add(lo);
            reloc.target_section_offset = full_immediate
                .wrapping_add(rel_symbol_offset)
                .wrapping_sub(rel_section_vram);
            if prev_hi_count != 0 {
                if prev_hi_symbol != rel_symbol {
                    return Err(format!(
                        "Paired HI16 and LO16 relocations have different symbols\n  LO16 reloc index {i} in section {section_name} referencing symbol {} with offset 0x{:08X}",
                        reloc.symbol_index, reloc.address
                    ));
                }
                // Set the previous HI16 relocs' relocated addresses.
                for paired in &mut relocs[i - prev_hi_count..i] {
                    let paired_full_immediate = paired.target_section_offset.wrapping_add(lo);
                    paired.target_section_offset = paired_full_immediate
                        .wrapping_add(rel_symbol_offset)
                        .wrapping_sub(rel_section_vram);
                }
            } else if elf_config.unpaired_lo16_warnings {
                // An orphaned LO16. Per the ABI, the previous HI16 still supplies the addend,
                // so only warn.
                if prev_lo {
                    // Some linkers emit several LO16s in a row for the same symbol.
                    if prev_hi_symbol != rel_symbol {
                        eprintln!(
                            "[WARN] LO16 reloc index {i} in section {section_name} referencing symbol {} with offset 0x{:08X} follows LO16 with different symbol",
                            reloc.symbol_index, reloc.address
                        );
                    }
                } else {
                    eprintln!(
                        "[WARN] Unpaired LO16 reloc index {i} in section {section_name} referencing symbol {} with offset 0x{:08X}",
                        reloc.symbol_index, reloc.address
                    );
                }
            }
            prev_lo = true;
        } else {
            // A HI16 may follow another HI16 (GNU extension), but nothing else may.
            if reloc.kind != RelocType::R_MIPS_HI16 && prev_hi_count != 0 {
                return Err(format!(
                    "Unpaired HI16 reloc index {} in section {section_name} referencing symbol {} with offset 0x{:08X}",
                    i - 1,
                    relocs[i - 1].symbol_index,
                    relocs[i - 1].address
                ));
            }
            prev_lo = false;
        }

        if reloc.kind == RelocType::R_MIPS_HI16 {
            let rel_immediate = reloc_rom_word & 0xFFFF;
            if prev_hi_count == 0 {
                // The first HI16: store its immediate.
                prev_hi_immediate = rel_immediate;
                prev_hi_symbol = rel_symbol;
            } else if prev_hi_symbol != rel_symbol {
                // A HI16 following another HI16 must reference the same symbol.
                return Err(format!(
                    "HI16 reloc (index {i} symbol {rel_symbol} offset 0x{:08X}) follows another HI16 reloc with a different symbol (index {} symbol {prev_hi_symbol} offset 0x{:08X}) in section {section_name}",
                    reloc.address,
                    i - 1,
                    relocs[i - 1].address
                ));
            }
            // Temporary; the full offset is calculated when the LO16 pairs with it.
            reloc.target_section_offset = rel_immediate << 16;
            prev_hi_count += 1;
        } else {
            prev_hi_count = 0;
        }

        if reloc.kind == RelocType::R_MIPS_32 {
            // The addend is the existing word, applied at load time, so the offset is just
            // the symbol's section offset.
            reloc.target_section_offset = rel_symbol_offset;
            if reloc.reference_symbol {
                // Patch the word in the ROM to include the symbol's value.
                let target_vram = context.get_reference_section_vram(reloc.target_section);
                let updated = reloc_rom_word
                    .wrapping_add(target_vram)
                    .wrapping_add(reloc.target_section_offset);
                write_rom_word(&mut context.rom, reloc_rom_addr, updated.swap_bytes());
            }
        }

        if reloc.kind == RelocType::R_MIPS_26 {
            let rel_immediate = (reloc_rom_word & 0x03FF_FFFF) << 2;
            reloc.target_section_offset = if reloc.reference_symbol {
                // Reference symbol offsets are already final, so skip the R_MIPS_26 rule for
                // the upper 4 bits.
                rel_immediate
                    .wrapping_add(rel_symbol_offset)
                    .wrapping_sub(rel_section_vram)
            } else {
                let upper = context.sections[section_index].ram_addr & 0xF000_0000;
                rel_immediate
                    .wrapping_add(rel_symbol_offset)
                    .wrapping_add(upper)
                    .wrapping_sub(rel_section_vram)
            };
        }

        context.sections[section_index].relocs[i] = reloc;
    }
    Ok(())
}

/// `read_symbols`: creates functions (and, when dumping, data symbols) from the symbol
/// table. Returns whether the entrypoint was found.
fn read_symbols(
    context: &mut Context,
    elf: &Elf,
    symtab: &ElfSection,
    elf_config: &ElfParsingConfig,
    dumping_context: bool,
    data_syms: &mut DataSymbolMap,
) -> Result<bool, String> {
    let mut found_entrypoint_func = false;

    // Bss section to its non-bss section, used when dumping so patches and mods can
    // relocate symbols in bss. Only matters for relocatable sections.
    let mut bss_section_to_target_section: HashMap<u16, u16> = HashMap::new();
    if dumping_context {
        for (index, section) in context.sections.iter().enumerate() {
            if section.bss_section_index != u16::MAX {
                bss_section_to_target_section.insert(section.bss_section_index, index as u16);
            }
        }
    }

    for sym_index in 0..elf.symbol_count(symtab) {
        let symbol = elf
            .symbol(symtab, sym_index)
            .expect("symbol index in range");
        let mut name = symbol.name;
        let value = symbol.value;
        let mut size = symbol.size as usize;
        let mut kind = symbol.kind;
        let section_index = symbol.shndx;
        let mut ignored = false;
        let mut reimplemented = false;
        let mut recorded_symbol = false;

        if section_index == SHN_ABS && elf_config.use_absolute_symbols {
            let index = context.functions.len();
            context
                .functions_by_vram
                .entry(value)
                .or_default()
                .push(index);
            context.functions.push(Function::new(
                value,
                0,
                Vec::new(),
                name,
                0,
                true,
                reimplemented,
            ));
            continue;
        }

        if usize::from(section_index) < context.sections.len() {
            // TODO (from the C++): this never fires; the check is broken due to signedness.
            // (The 64-bit symbol value is compared with the sign-extended entrypoint.)
            if elf_config.has_entrypoint
                && u64::from(value) == elf_config.entrypoint_address as i64 as u64
                && kind == STT_FUNC
            {
                if found_entrypoint_func {
                    return Err(format!("Ambiguous entrypoint: {name}"));
                }
                found_entrypoint_func = true;
                println!("Found entrypoint, original name: {name}");
                size = 0x50; // A dummy size that should cover any entrypoint.
                name = "recomp_entrypoint".into();
            }

            // Size overrides.
            if let Some(&manual_size) = elf_config.manually_sized_funcs.get(&name) {
                size = manual_size;
                kind = STT_FUNC;
            }

            if !dumping_context {
                if REIMPLEMENTED_FUNCS.contains(&name.as_str()) {
                    reimplemented = true;
                    name.push_str("_recomp");
                    ignored = true;
                } else if IGNORED_FUNCS.contains(&name.as_str()) {
                    name.push_str("_recomp");
                    ignored = true;
                }
            }

            // Functions and untyped symbols (like a regular glabel) get function entries, so
            // calls to them can be resolved.
            if ignored || kind == STT_FUNC || kind == STT_NOTYPE || kind == STT_OBJECT {
                if !dumping_context && RENAMED_FUNCS.contains(&name.as_str()) {
                    name.push_str("_recomp");
                    ignored = false;
                }

                let section_offset =
                    value.wrapping_sub(elf.sections[usize::from(section_index)].addr);
                let mut vram = value;
                let mut num_instructions = if kind == STT_FUNC {
                    (size / 4) as u32
                } else {
                    0
                };
                let section = &mut context.sections[usize::from(section_index)];
                let rom_address = section_offset.wrapping_add(section.rom_addr);

                section.function_addrs.push(vram);
                context
                    .functions_by_vram
                    .entry(vram)
                    .or_default()
                    .push(context.functions.len());

                // Find the entrypoint by rom address, in case its value isn't its vram.
                if elf_config.has_entrypoint && rom_address == 0x1000 && kind == STT_FUNC {
                    vram = elf_config.entrypoint_address as u32;
                    found_entrypoint_func = true;
                    name = "recomp_entrypoint".into();
                    if size == 0 {
                        num_instructions = 0x50 / 4;
                    }
                }

                // Suffix local symbols to prevent name conflicts.
                if symbol.bind == STB_LOCAL {
                    name = format!("{name}_{rom_address:08X}");
                }

                if num_instructions > 0 {
                    context.section_functions[usize::from(section_index)]
                        .push(context.functions.len());
                    recorded_symbol = true;
                }
                context
                    .functions_by_name
                    .insert(name.clone(), context.functions.len());

                let words = read_rom_words(
                    &context.rom,
                    rom_address as usize,
                    num_instructions as usize,
                );
                context.functions.push(Function::new(
                    vram,
                    rom_address,
                    words,
                    name.clone(),
                    section_index,
                    ignored,
                    reimplemented,
                ));
            }
        }

        // Symbols that weren't recorded as functions become data symbols when dumping.
        if !recorded_symbol
            && dumping_context
            && !name.is_empty()
            && symbol.other & 3 != STV_INTERNAL
        {
            // Absolute symbols go in the absolute list.
            let mut target_section_index = section_index;
            if section_index == SHN_ABS {
                target_section_index = SECTION_ABSOLUTE;
            } else if usize::from(section_index) >= context.sections.len() {
                println!("Symbol \"{name}\" not in a valid section ({section_index})");
            }

            // Move bss symbols into the paired section if that section is relocatable.
            if let Some(&paired) = bss_section_to_target_section.get(&target_section_index) {
                if context
                    .sections
                    .get(usize::from(paired))
                    .is_some_and(|s| s.relocatable)
                {
                    target_section_index = paired;
                }
            }

            data_syms
                .entry_or_default(target_section_index)
                .push(DataSymbol { vram: value, name });
        }
    }

    Ok(found_entrypoint_func)
}

impl Context {
    /// Builds a context from an ELF. Returns the context and whether the entrypoint was found.
    pub fn from_elf_file(
        path: &Path,
        mut context: Context,
        elf_config: &ElfParsingConfig,
        for_dumping_context: bool,
        data_syms: &mut DataSymbolMap,
    ) -> Result<(Context, bool), String> {
        let file = std::fs::read(path).map_err(|_| "Elf file not found".to_owned())?;
        let elf = match Elf::parse(&file) {
            Ok(elf) => elf,
            Err(ElfError::NotFound) => return Err("Elf file not found".into()),
            Err(ElfError::WrongClass) => return Err("Incorrect elf class".into()),
            Err(ElfError::WrongEndianness) => return Err("Incorrect elf endianness".into()),
            Err(ElfError::Malformed(what)) => return Err(format!("Elf file not found ({what})")),
        };

        context
            .sections
            .resize(elf.sections.len(), Default::default());
        context
            .section_functions
            .resize(elf.sections.len(), Vec::new());

        let (symtab_index, mdebug_index) = read_sections(&mut context, elf_config, &elf)?;
        let found_entrypoint = read_symbols(
            &mut context,
            &elf,
            &elf.sections[symtab_index],
            elf_config,
            for_dumping_context,
            data_syms,
        )?;

        // Static symbols from an mdebug section, if requested.
        if elf_config.use_mdebug {
            let Some(mdebug_index) = mdebug_index else {
                return Err("\"use_mdebug\" set to true in config, but no mdebug section is present in the elf!".into());
            };
            let section = &elf.sections[mdebug_index];
            if !mdebug::parse_mdebug(
                elf_config,
                &section.data,
                section.offset,
                &mut context,
                data_syms,
            ) {
                return Err("Failed to parse mdebug section".into());
            }
        }

        Ok((context, found_entrypoint))
    }
}
