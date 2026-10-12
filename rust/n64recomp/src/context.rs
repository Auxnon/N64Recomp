//! The recompiler's view of a program (`include/recompiler/context.h`).
//!
//! Instruction words are kept the way the C++ keeps them: the ROM's bytes read as a native
//! (little-endian) `u32`, byteswapped when decoded. That keeps patch application
//! (`words[i] = byteswap(value)`) identical.

use std::collections::HashMap;

use recomp_core::cxx_hash::CxxHashMap;

/// A relocation type. A newtype rather than an enum because ELF relocations can carry any
/// type number; the C++ casts them to its enum unchecked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RelocType(pub u8);

impl RelocType {
    pub const R_MIPS_NONE: Self = Self(0);
    pub const R_MIPS_16: Self = Self(1);
    pub const R_MIPS_32: Self = Self(2);
    pub const R_MIPS_REL32: Self = Self(3);
    pub const R_MIPS_26: Self = Self(4);
    pub const R_MIPS_HI16: Self = Self(5);
    pub const R_MIPS_LO16: Self = Self(6);
    pub const R_MIPS_GPREL16: Self = Self(7);

    /// `reloc_names` in `main.cpp` (including the trailing space on `R_MIPS_NONE `).
    pub fn output_name(self) -> &'static str {
        const NAMES: [&str; 8] = [
            "R_MIPS_NONE ",
            "R_MIPS_16",
            "R_MIPS_32",
            "R_MIPS_REL32",
            "R_MIPS_26",
            "R_MIPS_HI16",
            "R_MIPS_LO16",
            "R_MIPS_GPREL16",
        ];
        NAMES.get(usize::from(self.0)).copied().unwrap_or_else(|| {
            // The C++ indexes past the end of its table here.
            panic!("relocation type {} has no name", self.0)
        })
    }

    pub fn from_name(name: &str) -> Self {
        match name {
            "R_MIPS_16" => Self::R_MIPS_16,
            "R_MIPS_32" => Self::R_MIPS_32,
            "R_MIPS_REL32" => Self::R_MIPS_REL32,
            "R_MIPS_26" => Self::R_MIPS_26,
            "R_MIPS_HI16" => Self::R_MIPS_HI16,
            "R_MIPS_LO16" => Self::R_MIPS_LO16,
            "R_MIPS_GPREL16" => Self::R_MIPS_GPREL16,
            _ => Self::R_MIPS_NONE,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Function {
    pub vram: u32,
    pub rom: u32,
    pub words: Vec<u32>,
    pub name: String,
    pub section_index: u16,
    pub ignored: bool,
    pub reimplemented: bool,
    pub stubbed: bool,
    /// Text inserted before an instruction index; -1 inserts at the start of the function.
    pub function_hooks: HashMap<i32, String>,
}

impl Function {
    pub fn new(
        vram: u32,
        rom: u32,
        words: Vec<u32>,
        name: String,
        section_index: u16,
        ignored: bool,
        reimplemented: bool,
    ) -> Self {
        Self {
            vram,
            rom,
            words,
            name,
            section_index,
            ignored,
            reimplemented,
            ..Default::default()
        }
    }

    pub fn size_bytes(&self) -> u32 {
        (self.words.len() * 4) as u32
    }
}

#[derive(Clone, Debug)]
pub struct JumpTable {
    pub vram: u32,
    pub addend_reg: u32,
    pub rom: u32,
    pub lw_vram: u32,
    pub addu_vram: u32,
    pub jr_vram: u32,
    pub section_index: u16,
    pub got_offset: Option<u32>,
    pub entries: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Reloc {
    pub address: u32,
    pub target_section_offset: u32,
    /// Only used for reference symbols and special section symbols.
    pub symbol_index: u32,
    pub target_section: u16,
    pub kind: RelocType,
    pub reference_symbol: bool,
}

// Special section indices.
pub const SECTION_ABSOLUTE: u16 = 0xFFFE;
pub const SECTION_IMPORT: u16 = 0xFFFD;
pub const SECTION_EVENT: u16 = 0xFFFC;

// Special section names.
pub const PATCH_SECTION_NAME: &str = ".recomp_patch";
pub const FORCED_PATCH_SECTION_NAME: &str = ".recomp_force_patch";
pub const EXPORT_SECTION_NAME: &str = ".recomp_export";
pub const EVENT_SECTION_NAME: &str = ".recomp_event";

#[derive(Clone, Debug)]
pub struct Section {
    pub rom_addr: u32,
    pub ram_addr: u32,
    pub size: u32,
    /// Not populated when using a symbol toml.
    pub bss_size: u32,
    /// Only used by the CLI (to find the size of static functions).
    pub function_addrs: Vec<u32>,
    pub relocs: Vec<Reloc>,
    pub name: String,
    pub bss_section_index: u16,
    pub executable: bool,
    pub relocatable: bool,
    pub has_mips32_relocs: bool,
    pub got_ram_addr: Option<u32>,
}

impl Default for Section {
    fn default() -> Self {
        Self {
            rom_addr: 0,
            ram_addr: 0,
            size: 0,
            bss_size: 0,
            function_addrs: Vec::new(),
            relocs: Vec::new(),
            name: String::new(),
            bss_section_index: u16::MAX,
            executable: false,
            relocatable: false,
            has_mips32_relocs: false,
            got_ram_addr: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ReferenceSection {
    pub rom_addr: u32,
    pub ram_addr: u32,
    pub size: u32,
    pub relocatable: bool,
}

#[derive(Clone, Debug)]
pub struct ReferenceSymbol {
    pub name: String,
    pub section_index: u16,
    pub section_offset: u32,
    pub is_function: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ElfParsingConfig {
    pub bss_section_suffix: String,
    /// Functions with manual size overrides.
    pub manually_sized_funcs: HashMap<String, usize>,
    /// The section names that were specified as relocatable.
    pub relocatable_sections: std::collections::HashSet<String>,
    /// Symbols to ignore.
    pub ignored_syms: std::collections::HashSet<String>,
    /// Manual mappings of mdebug file records to elf sections.
    pub mdebug_text_map: HashMap<String, String>,
    pub mdebug_data_map: HashMap<String, String>,
    pub mdebug_rodata_map: HashMap<String, String>,
    pub mdebug_bss_map: HashMap<String, String>,
    pub has_entrypoint: bool,
    pub entrypoint_address: i32,
    pub use_absolute_symbols: bool,
    pub unpaired_lo16_warnings: bool,
    pub all_sections_relocatable: bool,
    pub use_mdebug: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataSymbol {
    pub vram: u32,
    pub name: String,
}

/// `std::unordered_map<uint16_t, std::vector<DataSymbol>>`; mdebug parsing iterates it.
pub type DataSymbolMap = CxxHashMap<u16, Vec<DataSymbol>>;

#[derive(Clone, Copy, Debug)]
pub struct SymbolReference {
    /// Reference symbol section index, or a special section index such as `SECTION_IMPORT`.
    pub section_index: u16,
    pub symbol_index: usize,
}

/// Zero-sized symbols between 0x8F000000 and 0x90000000 are manually specified symbols for
/// use with patches.
pub fn is_manual_patch_symbol(vram: u32) -> bool {
    (0x8F00_0000..0x9000_0000).contains(&vram)
}

#[derive(Debug)]
pub struct Context {
    // Reference symbols (used for populating relocations for patches).
    reference_sections: Vec<ReferenceSection>,
    reference_symbols: Vec<ReferenceSymbol>,
    reference_symbols_by_name: HashMap<String, SymbolReference>,
    all_reference_sections_relocatable: bool,
    import_symbols: Vec<ReferenceSymbol>,
    event_symbols: Vec<ReferenceSymbol>,

    pub sections: Vec<Section>,
    pub functions: Vec<Function>,
    /// The functions (by index in `functions`) in each section.
    pub section_functions: Vec<Vec<usize>>,
    /// Every function at a given vram address.
    pub functions_by_vram: HashMap<u32, Vec<usize>>,
    /// Bss section index to the corresponding non-bss section index.
    pub bss_section_to_section: HashMap<u16, u16>,
    /// The ROM being recompiled; used for reading relocations and for the output binary.
    pub rom: Vec<u8>,
    /// Whether reference symbols should be validated when emitting function calls.
    pub skip_validating_reference_symbols: bool,
    /// Whether all function calls (excluding reference symbols) should go through lookup.
    pub use_lookup_for_all_function_calls: bool,
    /// Function name to index in `functions` (CLI only).
    pub functions_by_name: HashMap<String, usize>,
    /// Makes functions print their name the first time they're called.
    pub trace_mode: bool,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            reference_sections: Vec::new(),
            reference_symbols: Vec::new(),
            reference_symbols_by_name: HashMap::new(),
            all_reference_sections_relocatable: false,
            import_symbols: Vec::new(),
            event_symbols: Vec::new(),
            sections: Vec::new(),
            functions: Vec::new(),
            section_functions: Vec::new(),
            functions_by_vram: HashMap::new(),
            bss_section_to_section: HashMap::new(),
            rom: Vec::new(),
            skip_validating_reference_symbols: true,
            use_lookup_for_all_function_calls: false,
            functions_by_name: HashMap::new(),
            trace_mode: false,
        }
    }
}

impl Context {
    pub fn find_function_by_vram_section(&self, vram: u32, section_index: usize) -> Option<usize> {
        self.functions_by_vram
            .get(&vram)?
            .iter()
            .copied()
            .find(|&i| usize::from(self.functions[i].section_index) == section_index)
    }

    pub fn has_reference_symbols(&self) -> bool {
        !self.reference_symbols.is_empty()
            || !self.import_symbols.is_empty()
            || !self.event_symbols.is_empty()
    }

    pub fn is_regular_reference_section(&self, section_index: u16) -> bool {
        section_index != SECTION_IMPORT && section_index != SECTION_EVENT
    }

    pub fn find_reference_symbol(&self, name: &str) -> Option<SymbolReference> {
        self.reference_symbols_by_name.get(name).copied()
    }

    pub fn reference_symbol_exists(&self, name: &str) -> bool {
        self.reference_symbols_by_name.contains_key(name)
    }

    pub fn get_reference_symbol(
        &self,
        section_index: u16,
        symbol_index: usize,
    ) -> &ReferenceSymbol {
        match section_index {
            SECTION_IMPORT => &self.import_symbols[symbol_index],
            SECTION_EVENT => &self.event_symbols[symbol_index],
            _ => &self.reference_symbols[symbol_index],
        }
    }

    pub fn is_reference_section_relocatable(&self, section_index: u16) -> bool {
        if self.all_reference_sections_relocatable {
            return true;
        }
        match section_index {
            SECTION_ABSOLUTE => false,
            SECTION_IMPORT | SECTION_EVENT => true,
            _ => self.reference_sections[usize::from(section_index)].relocatable,
        }
    }

    /// Returns `false` for an invalid section index.
    pub fn add_reference_symbol(
        &mut self,
        name: &str,
        section_index: u16,
        vram: u32,
        is_function: bool,
    ) -> bool {
        let section_vram = if section_index == SECTION_ABSOLUTE {
            0
        } else if let Some(section) = self.reference_sections.get(usize::from(section_index)) {
            section.ram_addr
        } else {
            return false;
        };

        // emplace: the first symbol with a given name wins the lookup.
        let symbol_index = self.reference_symbols.len();
        self.reference_symbols_by_name
            .entry(name.to_owned())
            .or_insert(SymbolReference {
                section_index,
                symbol_index,
            });
        self.reference_symbols.push(ReferenceSymbol {
            name: name.to_owned(),
            section_index,
            section_offset: vram.wrapping_sub(section_vram),
            is_function,
        });
        true
    }

    pub fn get_reference_section_vram(&self, section_index: u16) -> u32 {
        if section_index == SECTION_ABSOLUTE || !self.is_regular_reference_section(section_index) {
            0
        } else {
            self.reference_sections[usize::from(section_index)].ram_addr
        }
    }

    pub fn reference_sections(&self) -> &[ReferenceSection] {
        &self.reference_sections
    }

    /// Imports sections and function symbols from `reference` into this context's
    /// reference sections and symbols.
    pub fn import_reference_context(&mut self, reference: &Context) -> bool {
        self.reference_sections = reference
            .sections
            .iter()
            .map(|section| ReferenceSection {
                rom_addr: section.rom_addr,
                ram_addr: section.ram_addr,
                size: section.size,
                relocatable: section.relocatable,
            })
            .collect();
        reference
            .functions
            .iter()
            .all(|func| self.add_reference_symbol(&func.name, func.section_index, func.vram, true))
    }
}

/// Reads the native-endian word at `addr`, like the C++'s `*(uint32_t*)(rom + addr)`.
pub fn read_rom_word(rom: &[u8], addr: usize) -> u32 {
    let bytes = rom
        .get(addr..addr + 4)
        .unwrap_or_else(|| panic!("ROM read at {addr:#X} is out of bounds"));
    u32::from_ne_bytes(bytes.try_into().unwrap())
}

/// `count` consecutive native-endian words starting at `addr`.
pub fn read_rom_words(rom: &[u8], addr: usize, count: usize) -> Vec<u32> {
    (0..count)
        .map(|i| read_rom_word(rom, addr + i * 4))
        .collect()
}

pub fn write_rom_word(rom: &mut [u8], addr: usize, word: u32) {
    rom[addr..addr + 4].copy_from_slice(&word.to_ne_bytes());
}

#[cfg(target_endian = "big")]
compile_error!("the recompiler's word handling assumes a little-endian host, like the C++");
