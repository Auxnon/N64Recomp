//! The recompiler config file (`src/config.h`, `src/config.cpp`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use toml::Value;

use crate::tomlx::{
    TomlError, array_of, bool_of, concat_if_not_empty, err, get, i32_of, parse_file, string_of,
    u32_of,
};

#[derive(Clone, Debug)]
pub struct InstructionPatch {
    pub func_name: String,
    pub vram: i32,
    pub value: u32,
}

#[derive(Clone, Debug)]
pub struct FunctionTextHook {
    pub func_name: String,
    pub before_vram: i32,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct FunctionSize {
    pub func_name: String,
    pub size_bytes: u32,
}

#[derive(Clone, Debug)]
pub struct ManualFunction {
    pub func_name: String,
    pub section_name: String,
    pub vram: u32,
    pub size: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Config {
    pub entrypoint: i32,
    pub functions_per_output_file: i32,
    pub has_entrypoint: bool,
    pub uses_mips3_float_mode: bool,
    pub single_file_output: bool,
    pub use_absolute_symbols: bool,
    pub unpaired_lo16_warnings: bool,
    pub use_mdebug: bool,
    pub trace_mode: bool,
    pub allow_exports: bool,
    pub strict_patch_mode: bool,
    pub elf_path: PathBuf,
    pub symbols_file_path: PathBuf,
    pub func_reference_syms_file_path: PathBuf,
    pub data_reference_syms_file_paths: Vec<PathBuf>,
    pub rom_file_path: PathBuf,
    pub output_func_path: PathBuf,
    pub relocatable_sections_path: PathBuf,
    pub output_binary_path: PathBuf,
    pub stubbed_funcs: Vec<String>,
    pub ignored_funcs: Vec<String>,
    pub renamed_funcs: Vec<String>,
    pub instruction_patches: Vec<InstructionPatch>,
    pub function_hooks: Vec<FunctionTextHook>,
    pub manual_func_sizes: Vec<FunctionSize>,
    pub manual_functions: Vec<ManualFunction>,
    pub bss_section_suffix: String,
    pub recomp_include: String,
    // Manual mappings of mdebug file records to elf sections.
    pub mdebug_text_map: HashMap<String, String>,
    pub mdebug_data_map: HashMap<String, String>,
    pub mdebug_rodata_map: HashMap<String, String>,
    pub mdebug_bss_map: HashMap<String, String>,
}

fn get_manual_funcs(array: &[Value]) -> Result<Vec<ManualFunction>, TomlError> {
    array
        .iter()
        .map(|el| {
            let el = Some(el).filter(|el| el.is_table());
            match (
                string_of(get(el, "name")),
                string_of(get(el, "section")),
                u32_of(get(el, "vram")),
                u32_of(get(el, "size")),
            ) {
                (Some(func_name), Some(section_name), Some(vram), Some(size)) => {
                    Ok(ManualFunction {
                        func_name,
                        section_name,
                        vram,
                        size,
                    })
                }
                _ => err("Missing required value in manual_funcs array"),
            }
        })
        .collect()
}

fn get_data_syms_paths(array: &[Value], basedir: &Path) -> Result<Vec<PathBuf>, TomlError> {
    array
        .iter()
        .map(|el| match el.as_str() {
            Some(path) => Ok(concat_if_not_empty(basedir, path)),
            None => err("Invalid type for data reference symbol file entry"),
        })
        .collect()
}

/// `stubs` throws on non-strings; `ignored` and `renamed` skip them.
fn get_string_list(
    patches: Option<&Value>,
    key: &str,
    strict: bool,
) -> Result<Vec<String>, TomlError> {
    let Some(array) = array_of(get(patches, key)) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(array.len());
    for el in array {
        match el.as_str() {
            Some(s) => out.push(s.to_owned()),
            None if strict => return err("Invalid stubbed function"),
            None => {}
        }
    }
    Ok(out)
}

fn get_func_sizes(array: &[Value]) -> Result<Vec<FunctionSize>, TomlError> {
    array
        .iter()
        .map(|el| {
            if !el.is_table() {
                return err("Missing required value in function_sizes array");
            }
            let el = Some(el);
            match (string_of(get(el, "name")), u32_of(get(el, "size"))) {
                (Some(func_name), Some(size_bytes)) => {
                    if size_bytes & 3 != 0 {
                        return err("Function size is not divisible by 4");
                    }
                    Ok(FunctionSize {
                        func_name,
                        size_bytes,
                    })
                }
                _ => err("Manually sized function is missing required value(s)"),
            }
        })
        .collect()
}

fn get_instruction_patches(patches: Option<&Value>) -> Result<Vec<InstructionPatch>, TomlError> {
    let Some(array) = array_of(get(patches, "instruction")) else {
        return Ok(Vec::new());
    };
    array
        .iter()
        .map(|el| {
            if !el.is_table() {
                return err("Invalid instruction patch entry");
            }
            let el = Some(el);
            let (Some(vram), Some(func_name), Some(value)) = (
                u32_of(get(el, "vram")),
                string_of(get(el, "func")),
                u32_of(get(el, "value")),
            ) else {
                return err("Instruction patch is missing required value(s)");
            };
            if vram & 3 != 0 {
                return err("Instruction patch is not word-aligned");
            }
            Ok(InstructionPatch {
                func_name,
                vram: vram as i32,
                value,
            })
        })
        .collect()
}

fn get_function_hooks(patches: Option<&Value>) -> Result<Vec<FunctionTextHook>, TomlError> {
    let Some(array) = array_of(get(patches, "hook")) else {
        return Ok(Vec::new());
    };
    array
        .iter()
        .map(|el| {
            if !el.is_table() {
                return err("Invalid function hook entry");
            }
            let el = Some(el);
            let before_vram = u32_of(get(el, "before_vram"));
            let (Some(func_name), Some(text)) =
                (string_of(get(el, "func")), string_of(get(el, "text")))
            else {
                return err("Function hook is missing required value(s)");
            };
            if before_vram.is_some_and(|v| v & 3 != 0) {
                return err("before_vram is not word-aligned");
            }
            Ok(FunctionTextHook {
                func_name,
                before_vram: before_vram.map_or(0, |v| v as i32),
                text,
            })
        })
        .collect()
}

fn get_mdebug_mappings(array: &[Value], config: &mut Config) -> Result<(), TomlError> {
    for el in array {
        if !el.is_table() {
            return err("Invalid mdebug file mappings entry");
        }
        let el = Some(el);
        let (Some(filename), Some(input_section), Some(output_section)) = (
            string_of(get(el, "filename")),
            string_of(get(el, "input_section")),
            string_of(get(el, "output_section")),
        ) else {
            return err("Mdebug file mappings entry is missing required value(s)");
        };
        let map = match input_section.as_str() {
            ".text" => &mut config.mdebug_text_map,
            ".data" => &mut config.mdebug_data_map,
            ".rodata" => &mut config.mdebug_rodata_map,
            ".bss" => &mut config.mdebug_bss_map,
            _ => return err("Invalid input section in mdebug file mapping entry"),
        };
        // emplace: the first mapping for a file wins.
        map.entry(filename).or_insert(output_section);
    }
    Ok(())
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, TomlError> {
        let data = Value::Table(parse_file(path)?);
        let basedir = path.parent().unwrap_or(Path::new(""));
        let input = get(Some(&data), "input");
        let mut config = Config::default();

        // Input section (required)
        if let Some(entrypoint) = get(input, "entrypoint") {
            match u32_of(Some(entrypoint)) {
                Some(value) => {
                    config.entrypoint = value as i32;
                    config.has_entrypoint = true;
                }
                None => return err("Invalid entrypoint"),
            }
        }

        let path_field =
            |key: &str| string_of(get(input, key)).map(|p| concat_if_not_empty(basedir, &p));
        if let Some(p) = path_field("elf_path") {
            config.elf_path = p;
        }
        if let Some(p) = path_field("symbols_file_path") {
            config.symbols_file_path = p;
        }
        if let Some(p) = path_field("rom_file_path") {
            config.rom_file_path = p;
        }
        config.output_func_path = match path_field("output_func_path") {
            Some(p) => p,
            None => return err("Missing output_func_path in config file"),
        };
        config.relocatable_sections_path =
            path_field("relocatable_sections_path").unwrap_or_default();
        config.uses_mips3_float_mode =
            bool_of(get(input, "uses_mips3_float_mode")).unwrap_or(false);
        config.bss_section_suffix =
            string_of(get(input, "bss_section_suffix")).unwrap_or_else(|| ".bss".into());
        config.single_file_output = bool_of(get(input, "single_file_output")).unwrap_or(false);
        config.use_absolute_symbols = bool_of(get(input, "use_absolute_symbols")).unwrap_or(false);

        // Manual functions (optional)
        if let Some(array) = array_of(get(input, "manual_funcs")) {
            config.manual_functions = get_manual_funcs(array)?;
        }

        // Manual function sizes (optional)
        if let Some(array) = array_of(get(input, "function_sizes")) {
            config.manual_func_sizes = get_func_sizes(array)?;
        }

        // Output binary path when using an elf file input (optional)
        config.output_binary_path = path_field("output_binary_path").unwrap_or_default();
        config.unpaired_lo16_warnings =
            bool_of(get(input, "unpaired_lo16_warnings")).unwrap_or(true);
        config.use_mdebug = bool_of(get(input, "use_mdebug")).unwrap_or(false);

        if let Some(array) = array_of(get(input, "mdebug_file_mappings")) {
            get_mdebug_mappings(array, &mut config)?;
        }

        config.recomp_include = string_of(get(input, "recomp_include"))
            .unwrap_or_else(|| "#include \"recomp.h\"".into());

        config.functions_per_output_file = match i32_of(get(input, "functions_per_output_file")) {
            Some(n) if n <= 0 => return err("Invalid functions_per_output_file value"),
            Some(n) => n,
            None => 50,
        };

        // Patches section (optional)
        let patches = get(Some(&data), "patches").filter(|p| p.is_table());
        if patches.is_some() {
            config.stubbed_funcs = get_string_list(patches, "stubs", true)?;
            config.ignored_funcs = get_string_list(patches, "ignored", false)?;
            config.renamed_funcs = get_string_list(patches, "renamed", false)?;
            config.instruction_patches = get_instruction_patches(patches)?;
            config.function_hooks = get_function_hooks(patches)?;
        }

        // Use trace mode if enabled (optional)
        config.trace_mode = bool_of(get(input, "trace_mode")).unwrap_or(false);
        if config.trace_mode {
            config.recomp_include.push_str("\n#include \"trace.h\"");
        }

        // Function reference symbols file (optional)
        if let Some(p) = string_of(get(input, "func_reference_syms_file")) {
            if !config.symbols_file_path.as_os_str().is_empty() {
                return err("Reference symbol files can only be used in elf input mode");
            }
            config.func_reference_syms_file_path = concat_if_not_empty(basedir, &p);
        }

        // Data reference symbols files (optional)
        if let Some(array) = array_of(get(input, "data_reference_syms_files")) {
            if !config.symbols_file_path.as_os_str().is_empty() {
                return err("Reference symbol files can only be used in elf input mode");
            }
            if config.func_reference_syms_file_path.as_os_str().is_empty() {
                return err(
                    "Data reference symbol files can only be used if a function reference symbol file is also in use",
                );
            }
            config.data_reference_syms_file_paths = get_data_syms_paths(array, basedir)?;
        }

        config.allow_exports = bool_of(get(input, "allow_exports")).unwrap_or(false);
        // Strict patch mode defaults to on when a function reference symbol file is provided.
        config.strict_patch_mode = bool_of(get(input, "strict_patch_mode"))
            .unwrap_or(!config.func_reference_syms_file_path.as_os_str().is_empty());

        Ok(config)
    }
}
