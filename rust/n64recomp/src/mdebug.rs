//! Static symbols from a MIPS mdebug section (`src/mdebug.h`, `src/mdebug.cpp`).

use std::collections::HashMap;

use crate::context::{
    Context, DataSymbol, DataSymbolMap, ElfParsingConfig, Function, read_rom_words,
};

const MAGIC: u16 = 0x7009;

// Symbol types (ST_*) and storage classes (SC_*).
const ST_GLOBAL: u32 = 1;
const ST_STATIC: u32 = 2;
const ST_PROC: u32 = 6;
const ST_END: u32 = 8;
const ST_STATICPROC: u32 = 14;
const SC_BSS: u32 = 3;
const SC_RDATA: u32 = 15;

fn be16(data: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(data[at..at + 2].try_into().unwrap())
}

fn be32(data: &[u8], at: usize) -> i32 {
    i32::from_be_bytes(data[at..at + 4].try_into().unwrap())
}

/// The fields of the symbolic header (HDRR) that are used, with offsets made relative to
/// the section instead of the file.
struct Header {
    magic: u16,
    vstamp: u16,
    isym_max: i32,
    cb_sym_offset: i32,
    cb_ss_offset: i32,
    ifd_max: i32,
    cb_fd_offset: i32,
}

impl Header {
    fn read(data: &[u8], mdebug_offset: u32) -> Header {
        let relocated = |at| be32(data, at).wrapping_sub(mdebug_offset as i32);
        Header {
            magic: be16(data, 0),
            vstamp: be16(data, 2),
            isym_max: be32(data, 32),
            cb_sym_offset: relocated(36),
            cb_ss_offset: relocated(60),
            ifd_max: be32(data, 72),
            cb_fd_offset: relocated(76),
        }
    }
}

/// The used fields of a file descriptor (FDR, 72 bytes).
struct FileDescriptor {
    rss: i32,
    iss_base: i32,
    isym_base: i32,
    csym: i32,
}

const FDR_SIZE: usize = 72;
const SYMR_SIZE: usize = 12;

/// A local symbol (SYMR).
struct SymbolRecord {
    iss: i32,
    value: i32,
    bits: u32,
}

impl SymbolRecord {
    fn st(&self) -> u32 {
        self.bits >> 26 & 0b11_1111
    }

    fn sc(&self) -> u32 {
        self.bits >> 21 & 0b1_1111
    }
}

/// A NUL-terminated string at `at` in the section.
fn c_string(data: &[u8], at: usize) -> String {
    let rest = &data[at..];
    let len = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    String::from_utf8_lossy(&rest[..len]).into_owned()
}

#[derive(Clone, Default)]
struct MDebugSymbol {
    name: String,
    address: u32,
    size: u32,
    is_func: bool,
    is_static: bool,
    is_bss: bool,
    is_rodata: bool,
    ignored: bool,
}

struct MDebugFile {
    filename: String,
    symbols: Vec<MDebugSymbol>,
}

fn read_files(config: &ElfParsingConfig, data: &[u8], mdebug_offset: u32) -> Vec<MDebugFile> {
    let header = Header::read(data, mdebug_offset);
    if header.magic != MAGIC || header.vstamp != 0 {
        eprintln!(
            "Warning: Found an mdebug section with bad magic value or version (magic={} version={}). Skipping.",
            header.magic, header.vstamp
        );
        return Vec::new();
    }

    let fdrs: Vec<FileDescriptor> = (0..header.ifd_max as usize)
        .map(|i| {
            let at = header.cb_fd_offset as usize + i * FDR_SIZE;
            FileDescriptor {
                rss: be32(data, at + 4),
                iss_base: be32(data, at + 8),
                isym_base: be32(data, at + 16),
                csym: be32(data, at + 20),
            }
        })
        .collect();
    let symrs: Vec<SymbolRecord> = (0..header.isym_max as usize)
        .map(|i| {
            let at = header.cb_sym_offset as usize + i * SYMR_SIZE;
            SymbolRecord {
                iss: be32(data, at),
                value: be32(data, at + 4),
                bits: be32(data, at + 8) as u32,
            }
        })
        .collect();

    let mut files = Vec::with_capacity(fdrs.len());
    for fdr in &fdrs {
        let string_at = |index: i32| {
            (header.cb_ss_offset as usize)
                .wrapping_add(fdr.iss_base as usize)
                .wrapping_add(index as usize)
        };
        let mut file = MDebugFile {
            filename: c_string(data, string_at(fdr.rss)),
            symbols: Vec::new(),
        };

        // Like the C++, `pending` is reused after being moved into the list: its name is
        // emptied and its other fields keep their old values. One consequence: a procedure
        // is flushed (with size 0) at its first local symbol, so its END sets the size of an
        // already-added symbol's leftover copy, which goes nowhere.
        let mut pending = MDebugSymbol::default();
        let mut pending_ready = false;
        let flush = |pending: &mut MDebugSymbol,
                     pending_ready: &mut bool,
                     symbols: &mut Vec<MDebugSymbol>| {
            if *pending_ready {
                pending.ignored = config.ignored_syms.contains(&pending.name);
                let mut sym = pending.clone();
                sym.name = std::mem::take(&mut pending.name);
                symbols.push(sym);
            }
            *pending_ready = false;
        };

        let first = fdr.isym_base as usize;
        for symr in &symrs[first..first + fdr.csym as usize] {
            match symr.st() {
                ST_PROC | ST_STATICPROC => {
                    flush(&mut pending, &mut pending_ready, &mut file.symbols);
                    if symr.value != 0 {
                        pending.name = c_string(data, string_at(symr.iss));
                        pending.address = symr.value as u32;
                        pending.size = 0;
                        pending.is_func = true;
                        pending.is_static = symr.st() == ST_STATICPROC;
                        pending.is_bss = false;
                        pending.is_rodata = false;
                        pending_ready = true;
                    }
                }
                ST_END => {
                    if pending.is_func {
                        pending.size = symr.value as u32;
                    }
                    flush(&mut pending, &mut pending_ready, &mut file.symbols);
                }
                ST_GLOBAL | ST_STATIC => {
                    flush(&mut pending, &mut pending_ready, &mut file.symbols);
                    if symr.value != 0 {
                        pending.name = c_string(data, string_at(symr.iss));
                        pending.address = symr.value as u32;
                        pending.size = 0;
                        pending.is_func = false;
                        pending.is_static = symr.st() == ST_STATIC;
                        pending.is_bss = symr.sc() == SC_BSS;
                        pending.is_rodata = symr.sc() == SC_RDATA;
                        pending_ready = true;
                    }
                }
                _ => flush(&mut pending, &mut pending_ready, &mut file.symbols),
            }
        }
        flush(&mut pending, &mut pending_ready, &mut file.symbols);
        files.push(file);
    }
    files
}

fn sanitize_section_name(name: &str) -> String {
    name.trim_start_matches('.')
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Looks up a manual mapping of `filename` to a section, failing if the section doesn't exist.
fn mapped_section(
    map: &HashMap<String, String>,
    filename: &str,
    sections_by_name: &HashMap<String, u16>,
    what: &str,
) -> Result<Option<u16>, ()> {
    let Some(target) = map.get(filename) else {
        return Ok(None);
    };
    match sections_by_name.get(target) {
        Some(&index) => Ok(Some(index)),
        None => {
            println!(
                "{what} section for mdebug source file \"{filename}\" is mapped to section \"{target}\", which doesn't exist in the elf"
            );
            Err(())
        }
    }
}

fn populate_context(
    config: &ElfParsingConfig,
    files: &[MDebugFile],
    context: &mut Context,
    data_syms: &mut DataSymbolMap,
) -> bool {
    let num_files = files.len();
    let mut file_text_sections = vec![u16::MAX; num_files];

    // Section name to elf section index (the first section with a name wins).
    let mut sections_by_name: HashMap<String, u16> = HashMap::new();
    for (index, section) in context.sections.iter().enumerate() {
        sections_by_name
            .entry(section.name.clone())
            .or_insert(index as u16);
    }

    // First pass: map each file to the elf section its .text (and data) went into.
    for (file_index, file) in files.iter().enumerate() {
        if file.symbols.is_empty() {
            continue;
        }
        let (mut has_funcs, mut has_data, mut has_rodata, mut has_bss) =
            (false, false, false, false);
        // The section of this file's .text, found through its global functions.
        let mut file_text_section: i32 = -1;
        let (mut min_data, mut max_data) = (0u32, 0u32);
        let (mut min_rodata, mut max_rodata) = (0u32, 0u32);
        let (mut min_bss, mut max_bss) = (0u32, 0u32);
        let widen = |min: &mut u32, max: &mut u32, sym: &MDebugSymbol| {
            *min = if *min == 0 {
                sym.address
            } else {
                (*min).min(sym.address)
            };
            *max = (*max).max(sym.address.wrapping_add(sym.size));
        };
        for sym in &file.symbols {
            if sym.ignored {
                continue;
            }
            if sym.is_func {
                has_funcs = true;
                if !sym.is_static {
                    if let Some(&index) = context.functions_by_name.get(&sym.name) {
                        file_text_section = i32::from(context.functions[index].section_index);
                    }
                }
            } else if sym.address != 0 {
                if sym.is_bss {
                    has_bss = true;
                    widen(&mut min_bss, &mut max_bss, sym);
                } else if sym.is_rodata {
                    has_rodata = true;
                    widen(&mut min_rodata, &mut max_rodata, sym);
                } else {
                    has_data = true;
                    widen(&mut min_data, &mut max_data, sym);
                }
            }
        }

        if !has_funcs {
            continue;
        }

        // Manual .text mapping.
        match mapped_section(
            &config.mdebug_text_map,
            &file.filename,
            &sections_by_name,
            ".text",
        ) {
            Ok(Some(index)) => file_text_section = i32::from(index),
            Ok(None) => {}
            Err(()) => return false,
        }
        if file_text_section == -1 {
            println!(
                "Couldn't determine elf section of mdebug info for file {}",
                file.filename
            );
            return false;
        }
        file_text_sections[file_index] = file_text_section as u16;
        let text_section = &context.sections[file_text_section as usize];
        let in_text = |min: u32, max: u32| {
            !(min < text_section.ram_addr
                || max > text_section.ram_addr.wrapping_add(text_section.size))
        };

        // The data, rodata and bss sections are only validated here; statics are placed in
        // the .text section below. (The C++ uses the .data mapping for all three.)
        if has_data {
            match mapped_section(
                &config.mdebug_data_map,
                &file.filename,
                &sections_by_name,
                ".data",
            ) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    if !in_text(min_data, max_data) {
                        println!(
                            "File {} has static data in mdebug which did not overlap with section {}",
                            file.filename, text_section.name
                        );
                        return false;
                    }
                }
                Err(()) => return false,
            }
        }
        if has_rodata {
            match mapped_section(
                &config.mdebug_data_map,
                &file.filename,
                &sections_by_name,
                ".rodata",
            ) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    if !in_text(min_rodata, max_rodata) {
                        println!(
                            "File {} has static rodata in mdebug which did not overlap with section {}",
                            file.filename, text_section.name
                        );
                        return false;
                    }
                }
                Err(()) => return false,
            }
        }
        if has_bss {
            match mapped_section(
                &config.mdebug_data_map,
                &file.filename,
                &sections_by_name,
                ".bss",
            ) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    if text_section.bss_section_index == u16::MAX {
                        println!(
                            "File {} has static bss in mdebug but no paired bss section. Use the \"bss_section_suffix\" option to pair bss sections.",
                            file.filename
                        );
                        return false;
                    }
                    let bss_section =
                        &context.sections[usize::from(text_section.bss_section_index)];
                    if min_bss < bss_section.ram_addr
                        || max_bss > bss_section.ram_addr.wrapping_add(bss_section.size)
                    {
                        println!(
                            "File {} has static bss in mdebug which did not overlap with bss section {}",
                            file.filename, bss_section.name
                        );
                        return false;
                    }
                }
                Err(()) => return false,
            }
        }
    }

    // Second pass: assign static symbols to sections, so the third pass can rename them.
    // Every static symbol goes in its file's .text section.
    let mut symbol_name_to_sections: HashMap<&str, Vec<u16>> = HashMap::new();
    let mut file_symbol_sections: Vec<Vec<u16>> = Vec::with_capacity(num_files);
    for (file_index, file) in files.iter().enumerate() {
        let mut sections = vec![0u16; file.symbols.len()];
        for (sym_index, sym) in file.symbols.iter().enumerate() {
            if sym.ignored || !sym.is_static {
                continue;
            }
            let sym_section = file_text_sections[file_index];
            symbol_name_to_sections
                .entry(&sym.name)
                .or_default()
                .push(sym_section);
            sections[sym_index] = sym_section;
        }
        file_symbol_sections.push(sections);
    }

    // Data symbol name to section (the first one in data_syms iteration order wins).
    let mut datasyms_by_name: HashMap<String, u16> = HashMap::new();
    for (section_index, syms) in data_syms.iter() {
        for sym in syms {
            datasyms_by_name
                .entry(sym.name.clone())
                .or_insert(section_index);
        }
    }

    // Third pass: add the symbols, renaming them to avoid conflicts.
    for (file_index, file) in files.iter().enumerate() {
        for (sym_index, sym) in file.symbols.iter().enumerate() {
            if sym.ignored || !sym.is_static {
                continue;
            }
            let sym_section = file_symbol_sections[file_index][sym_index];
            // Skip symbols with no section (static data in a file without functions).
            if sym_section == u16::MAX {
                continue;
            }

            let mut already_exists = false;
            let mut already_exists_in_section = false;

            if let Some(&index) = context.functions_by_name.get(&sym.name) {
                already_exists = true;
                if sym_section == context.functions[index].section_index {
                    already_exists_in_section = true;
                }
            }
            if let Some(&section) = datasyms_by_name.get(&sym.name) {
                already_exists = true;
                if sym_section == section {
                    already_exists_in_section = true;
                }
            }
            if let Some(section_list) = symbol_name_to_sections.get(sym.name.as_str()) {
                // This symbol itself is always in the list.
                if section_list.iter().filter(|&&s| s == sym_section).count() > 1 {
                    already_exists_in_section = true;
                } else if section_list.len() > 1 {
                    already_exists = true;
                }
            }

            let section_name =
                sanitize_section_name(&context.sections[usize::from(sym_section)].name);
            let mut output_name = sym.name.clone();
            if already_exists_in_section {
                output_name += &format!("_{section_name}_{:08X}", sym.address);
                println!(
                    "Renamed static symbol \"{}\" to \"{output_name}\"",
                    sym.name
                );
            } else if already_exists {
                output_name += &format!("_{section_name}");
                println!(
                    "Renamed static symbol \"{}\" to \"{output_name}\"",
                    sym.name
                );
            }

            if sym.is_func {
                let section = &context.sections[usize::from(sym_section)];
                let rom_address = sym
                    .address
                    .wrapping_sub(section.ram_addr)
                    .wrapping_add(section.rom_addr);
                let words =
                    read_rom_words(&context.rom, rom_address as usize, (sym.size / 4) as usize);
                let index = context.functions.len();
                context
                    .functions_by_vram
                    .entry(sym.address)
                    .or_default()
                    .push(index);
                context.section_functions[usize::from(sym_section)].push(index);
                // TODO (from the C++): read ignored/reimplemented/stubbed from the config.
                context.functions.push(Function::new(
                    sym.address,
                    rom_address,
                    words,
                    output_name,
                    sym_section,
                    false,
                    false,
                ));
            } else {
                data_syms.entry_or_default(sym_section).push(DataSymbol {
                    vram: sym.address,
                    name: output_name,
                });
            }
        }
    }

    true
}

pub fn parse_mdebug(
    config: &ElfParsingConfig,
    section_data: &[u8],
    section_offset: u32,
    context: &mut Context,
    data_syms: &mut DataSymbolMap,
) -> bool {
    let files = read_files(config, section_data, section_offset);
    populate_context(config, &files, context, data_syms)
}
