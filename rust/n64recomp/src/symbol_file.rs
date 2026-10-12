//! Symbol toml files (`Context::from_symbol_file`, `read_data_reference_syms` in
//! `src/config.cpp`).

use std::collections::HashMap;
use std::path::Path;

use toml::Value;

use crate::context::{
    Context, Function, Reloc, RelocType, SECTION_ABSOLUTE, Section, read_rom_word,
};
use crate::tomlx::{TomlError, array_of, err, get, parse_file, string_of, u32_of, u64_of};

impl Context {
    /// Builds a context from a symbol file. With an empty `rom`, functions get no words.
    pub fn from_symbol_file(
        path: &Path,
        rom: Vec<u8>,
        with_relocs: bool,
    ) -> Result<Context, TomlError> {
        let data = Value::Table(parse_file(path)?);
        let Some(sections) = array_of(get(Some(&data), "section")) else {
            return err("missing [[section]] array");
        };

        let mut ret = Context::default();
        ret.section_functions.resize(sections.len(), Vec::new());

        for el in sections {
            if !el.is_table() {
                return err("Invalid section entry");
            }
            let el = Some(el);
            let (Some(rom_addr), Some(ram_addr), Some(size), Some(name)) = (
                u32_of(get(el, "rom")),
                u32_of(get(el, "vram")),
                u32_of(get(el, "size")),
                string_of(get(el, "name")),
            ) else {
                return err("Section entry missing required field(s)");
            };

            let section_index = ret.sections.len() as u16;
            let mut section = Section {
                rom_addr,
                ram_addr,
                size,
                name,
                got_ram_addr: u32_of(get(el, "got_address")),
                executable: true,
                ..Default::default()
            };

            // Read functions for the section.
            let Some(functions) = array_of(get(el, "functions")) else {
                return err("Invalid functions array");
            };
            for func_el in functions {
                let function_index = ret.functions.len();
                if !func_el.is_table() {
                    return err("Invalid function symbol entry");
                }
                let func_el = Some(func_el);
                let (Some(name), Some(vram), Some(func_size)) = (
                    string_of(get(func_el, "name")),
                    u32_of(get(func_el, "vram")),
                    u32_of(get(func_el, "size")),
                ) else {
                    return err("Function symbol entry is missing required field(s)");
                };

                let func_rom = vram
                    .wrapping_sub(section.ram_addr)
                    .wrapping_add(section.rom_addr);
                if vram & 3 != 0 {
                    return err("Function's vram address isn't word aligned");
                }
                if func_rom & 3 != 0 {
                    return err("Function's rom address isn't word aligned");
                }

                // Read the function's words if a rom was provided.
                let mut words = Vec::new();
                if !rom.is_empty() {
                    let end = func_rom.wrapping_add(func_size) as usize;
                    if end > rom.len() {
                        return err("Function is out of bounds of the provided rom");
                    }
                    let mut addr = func_rom as usize;
                    while addr < end {
                        words.push(read_rom_word(&rom, addr));
                        addr += 4;
                    }
                }

                section.function_addrs.push(vram);
                ret.functions_by_name.insert(name.clone(), function_index);
                ret.functions_by_vram
                    .entry(vram)
                    .or_default()
                    .push(function_index);
                ret.section_functions[usize::from(section_index)].push(function_index);
                ret.functions.push(Function {
                    vram,
                    rom: func_rom,
                    words,
                    name,
                    section_index,
                    ..Default::default()
                });
            }

            // Check if relocs exist for the section and read them if so.
            if let Some(relocs) = array_of(get(el, "relocs")) {
                // The section has relocs, so it's relocatable.
                section.relocatable = true;
                if with_relocs {
                    for reloc_el in relocs {
                        if !reloc_el.is_table() {
                            return err("Invalid reloc entry");
                        }
                        let reloc_el = Some(reloc_el);
                        let (Some(vram), Some(target_vram), Some(type_string)) = (
                            u32_of(get(reloc_el, "vram")),
                            u32_of(get(reloc_el, "target_vram")),
                            string_of(get(reloc_el, "type")),
                        ) else {
                            return err("Reloc entry missing required field(s)");
                        };
                        let kind = RelocType::from_name(&type_string);
                        if ![
                            RelocType::R_MIPS_HI16,
                            RelocType::R_MIPS_LO16,
                            RelocType::R_MIPS_26,
                            RelocType::R_MIPS_32,
                        ]
                        .contains(&kind)
                        {
                            return err("Invalid reloc entry type");
                        }
                        section.relocs.push(Reloc {
                            address: vram,
                            target_section_offset: target_vram.wrapping_sub(section.ram_addr),
                            symbol_index: u32::MAX,
                            target_section: section_index,
                            kind,
                            reference_symbol: false,
                        });
                    }
                }
            }

            ret.sections.push(section);
        }

        ret.rom = rom;
        Ok(ret)
    }

    /// Reads a data symbol file into this context's reference symbols.
    pub fn read_data_reference_syms(&mut self, path: &Path) -> Result<(), TomlError> {
        let data = Value::Table(parse_file(path)?);
        let Some(sections) = array_of(get(Some(&data), "section")) else {
            return err("missing [[section]] array");
        };

        // Map ROM addresses to reference sections, so function and data reference symbols
        // use the same section indices. emplace: the first section at an address wins.
        let mut sections_by_vrom: HashMap<u32, u16> = HashMap::new();
        for (index, section) in self.reference_sections().iter().enumerate() {
            sections_by_vrom
                .entry(section.rom_addr)
                .or_insert(index as u16);
        }

        for el in sections {
            if !el.is_table() {
                return err("Invalid section entry");
            }
            let el = Some(el);
            let rom_addr = u64_of(get(el, "rom"));
            let (Some(vram), Some(size), Some(_name)) = (
                u32_of(get(el, "vram")),
                u32_of(get(el, "size")),
                string_of(get(el, "name")),
            ) else {
                return err("Section entry missing required field(s)");
            };

            let section_index = match rom_addr {
                // Non-relocatable bss section or absolute symbols.
                None => SECTION_ABSOLUTE,
                Some(rom) if rom > 0xFFFF_FFFF => return err("Section has invalid ROM address"),
                // Sections missing from the function reference file are treated as absolute.
                Some(rom) => sections_by_vrom
                    .get(&(rom as u32))
                    .copied()
                    .unwrap_or(SECTION_ABSOLUTE),
            };

            // Sanity check this section against the matching function reference section.
            if section_index != SECTION_ABSOLUTE {
                let reference = self.reference_sections()[usize::from(section_index)];
                if reference.ram_addr != vram {
                    return err(
                        "Section vram address differs from matching ROM address section in the function symbol reference file",
                    );
                }
                if reference.size != size {
                    return err(
                        "Section size address differs from matching ROM address section in the function symbol reference file",
                    );
                }
            }

            let Some(symbols) = array_of(get(el, "symbols")) else {
                return err("Invalid symbols array");
            };
            for sym_el in symbols {
                if !sym_el.is_table() {
                    return err("Invalid data symbol entry");
                }
                let sym_el = Some(sym_el);
                let (Some(name), Some(sym_vram)) =
                    (string_of(get(sym_el, "name")), u32_of(get(sym_el, "vram")))
                else {
                    return err("Reference data symbol entry is missing required field(s)");
                };
                if !self.add_reference_symbol(&name, section_index, sym_vram, false) {
                    return err(
                        "Internal error: Failed to add reference symbol to context. Please report this issue.",
                    );
                }
            }
        }
        Ok(())
    }
}
