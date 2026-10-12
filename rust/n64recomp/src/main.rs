//! The `N64Recomp` command line tool (`src/main.cpp`).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use n64recomp::config::{Config, ManualFunction};
use n64recomp::context::{
    Context, DataSymbol, DataSymbolMap, EVENT_SECTION_NAME, EXPORT_SECTION_NAME, ElfParsingConfig,
    FORCED_PATCH_SECTION_NAME, Function, PATCH_SECTION_NAME, RelocType, SECTION_ABSOLUTE,
    SECTION_EVENT, is_manual_patch_symbol, read_rom_words,
};
use n64recomp::generator::CGenerator;
use n64recomp::instr::configure_rabbitizer;
use n64recomp::recompilation::recompile_function;
use n64recomp::symbol_lists::{IGNORED_FUNCS, REIMPLEMENTED_FUNCS, RENAMED_FUNCS};
use recomp_core::cxx_path;
use recomp_core::cxx_sort;

/// Appends formatted text to a `String` (which can't fail).
macro_rules! out {
    ($out:expr, $($arg:tt)*) => {{
        let _ = write!($out, $($arg)*);
    }};
}

/// Prints an error and exits, like the C++'s `exit_failure`.
fn exit_failure(msg: impl AsRef<str>) -> ! {
    eprint!("{}", msg.as_ref());
    std::process::exit(1);
}

fn path_str(path: &Path) -> &str {
    path.to_str()
        .unwrap_or_else(|| exit_failure(format!("Path is not valid UTF-8: {}\n", path.display())))
}

fn write_file(path: &Path, contents: &[u8]) {
    if let Err(err) = std::fs::write(path, contents) {
        exit_failure(format!("Failed to write {}: {err}\n", path.display()));
    }
}

fn add_manual_functions(context: &mut Context, manual_funcs: &[ManualFunction]) {
    // Section name to section index.
    let mut section_indices_by_name: HashMap<&str, usize> = HashMap::new();
    for (index, section) in context.sections.iter().enumerate() {
        section_indices_by_name
            .entry(section.name.as_str())
            .or_insert(index);
    }
    let section_indices_by_name: HashMap<String, usize> = section_indices_by_name
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();

    for def in manual_funcs {
        let Some(&section_index) = section_indices_by_name.get(&def.section_name) else {
            exit_failure(format!(
                "Manual function {} specified with section {}, which doesn't exist!\n",
                def.func_name, def.section_name
            ));
        };
        if context.functions_by_name.contains_key(&def.func_name) {
            exit_failure(format!(
                "Manual function {} already exists!\n",
                def.func_name
            ));
        }
        if def.size & 3 != 0 {
            exit_failure(format!(
                "Manual function {} has a size that isn't divisible by 4!\n",
                def.func_name
            ));
        }

        let section = &context.sections[section_index];
        let section_offset = def.vram.wrapping_sub(section.ram_addr);
        let rom_address = section_offset.wrapping_add(section.rom_addr);
        let words = read_rom_words(
            &context.rom,
            section.rom_addr.wrapping_add(section_offset) as usize,
            (def.size / 4) as usize,
        );

        let function_index = context.functions.len();
        context.functions.push(Function::new(
            def.vram,
            rom_address,
            words,
            def.func_name.clone(),
            section_index as u16,
            false,
            false,
        ));
        context.section_functions[section_index].push(function_index);
        // The C++ records the function index here, not its address.
        context.sections[section_index]
            .function_addrs
            .push(function_index as u32);
        context
            .functions_by_vram
            .entry(def.vram)
            .or_default()
            .push(function_index);
        context
            .functions_by_name
            .insert(def.func_name.clone(), function_index);
    }
}

/// Whitespace-separated entries, like reading a file with `>>`.
fn read_list_file(path: &Path) -> Option<Vec<String>> {
    let text = std::fs::read(path).ok()?;
    Some(
        String::from_utf8_lossy(&text)
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect(),
    )
}

/// Writes one function to its own file. Leaves an identical existing file untouched, so
/// it doesn't need to be rebuilt.
fn recompile_single_function(
    context: &Context,
    func_index: usize,
    recomp_include: &str,
    output_path: &Path,
    static_funcs_out: &mut [Vec<u32>],
) -> bool {
    let mut generator = CGenerator::new();
    out!(generator.out, "{recomp_include}\n\n");
    if !recompile_function(&mut generator, context, func_index, static_funcs_out, false) {
        return false;
    }

    // The C++ writes a .tmp file and either deletes it (when the output is unchanged) or
    // renames it over the output; the result is the same.
    if std::fs::read(output_path).is_ok_and(|existing| existing == generator.out.as_bytes()) {
        return true;
    }
    let temp_path = PathBuf::from(cxx_path::replace_extension(path_str(output_path), ".tmp"));
    if std::fs::write(&temp_path, &generator.out).is_err() {
        eprintln!("Failed to open file for writing: {}", temp_path.display());
        return false;
    }
    if let Err(err) = std::fs::rename(&temp_path, output_path) {
        exit_failure(format!("Failed to rename {}: {err}\n", temp_path.display()));
    }
    true
}

fn dump_context(context: &Context, data_syms: &DataSymbolMap, func_path: &Path, data_path: &Path) {
    let mut func_out = String::from("# Autogenerated from an ELF via N64Recomp\n");
    let mut data_out = String::from("# Autogenerated from an ELF via N64Recomp\n");

    let print_section = |out: &mut String, name: &str, rom_addr: u32, ram_addr: u32, size: u32| {
        if rom_addr == u32::MAX {
            out!(
                out,
                "[[section]]\nname = \"{name}\"\nvram = 0x{ram_addr:08X}\nsize = 0x{size:X}\n\n"
            );
        } else {
            out!(
                out,
                "[[section]]\nname = \"{name}\"\nrom = 0x{rom_addr:08X}\nvram = 0x{ram_addr:08X}\nsize = 0x{size:X}\n\n"
            );
        }
    };

    let print_symbols = |out: &mut String, syms: &[DataSymbol]| {
        let mut sorted: Vec<&DataSymbol> = syms.iter().collect();
        cxx_sort::sort_by(&mut sorted, |a, b| (a.vram, &a.name) < (b.vram, &b.name));
        out.push_str("symbols = [\n");
        for sym in sorted {
            out!(
                out,
                "    {{ name = \"{}\", vram = 0x{:08X} }},\n",
                sym.name,
                sym.vram
            );
        }
        out.push_str("]\n\n");
    };

    for (section_index, section) in context.sections.iter().enumerate() {
        let section_funcs = &context.section_functions[section_index];
        if !section_funcs.is_empty() {
            print_section(
                &mut func_out,
                &section.name,
                section.rom_addr,
                section.ram_addr,
                section.size,
            );

            // Relocs into this section (or its bss) go in the function context file.
            if !section.relocs.is_empty() {
                let mut sorted_relocs: Vec<_> = section
                    .relocs
                    .iter()
                    .filter(|reloc| {
                        (usize::from(reloc.target_section) == section_index
                            || reloc.target_section == section.bss_section_index)
                            // TODO (from the C++): MIPS32 relocs for TLB mapping support.
                            && matches!(reloc.kind, RelocType::R_MIPS_HI16 | RelocType::R_MIPS_LO16 | RelocType::R_MIPS_26)
                    })
                    .collect();
                cxx_sort::sort_by(&mut sorted_relocs, |a, b| a.address < b.address);

                func_out.push_str("relocs = [\n");
                for reloc in sorted_relocs {
                    out!(
                        func_out,
                        "    {{ type = \"{}\", vram = 0x{:08X}, target_vram = 0x{:08X} }},\n",
                        reloc.kind.output_name(),
                        reloc.address,
                        reloc.target_section_offset.wrapping_add(section.ram_addr)
                    );
                }
                func_out.push_str("]\n\n");
            }

            let mut sorted_funcs: Vec<&Function> = section_funcs
                .iter()
                .map(|&i| &context.functions[i])
                .collect();
            cxx_sort::sort_by(&mut sorted_funcs, |a, b| {
                (a.vram, &a.name) < (b.vram, &b.name)
            });
            func_out.push_str("functions = [\n");
            for func in sorted_funcs {
                out!(
                    func_out,
                    "    {{ name = \"{}\", vram = 0x{:08X}, size = 0x{:X} }},\n",
                    func.name,
                    func.vram,
                    func.words.len() * 4
                );
            }
            func_out.push_str("]\n\n");
        }

        if let Some(syms) = data_syms
            .get(section_index as u16)
            .filter(|syms| !syms.is_empty())
        {
            print_section(
                &mut data_out,
                &section.name,
                section.rom_addr,
                section.ram_addr,
                section.size,
            );
            print_symbols(&mut data_out, syms);
        }
    }

    if let Some(syms) = data_syms
        .get(SECTION_ABSOLUTE)
        .filter(|syms| !syms.is_empty())
    {
        print_section(&mut data_out, "ABSOLUTE_SYMS", u32::MAX, 0, 0);
        print_symbols(&mut data_out, syms);
    }

    write_file(func_path, func_out.as_bytes());
    write_file(data_path, data_out.as_bytes());
}

fn read_file(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

/// The output files: one per function, one per `functions_per_output_file` functions, or
/// a single file.
struct Output<'a> {
    config: &'a Config,
    /// The file being written in batched or single file mode.
    current: Option<(PathBuf, String)>,
    output_file_count: usize,
    cur_file_function_count: i32,
}

impl Output<'_> {
    fn batched(&self) -> bool {
        self.config.single_file_output || self.config.functions_per_output_file > 1
    }

    fn file_header(&self) -> String {
        let mut header = format!("{}\n#include \"funcs.h\"\n\n", self.config.recomp_include);
        // Rename the base event index when exports are allowed.
        if self.config.allow_exports {
            header.push_str(
                "extern uint32_t builtin_base_event_index;\n#define base_event_index builtin_base_event_index\n\n",
            );
        }
        header
    }

    fn flush(&mut self) {
        if let Some((path, contents)) = self.current.take() {
            write_file(&path, contents.as_bytes());
        }
    }

    fn open(&mut self, path: PathBuf) {
        self.flush();
        let header = self.file_header();
        self.current = Some((path, header));
    }

    fn open_new_output_file(&mut self) {
        let path = self
            .config
            .output_func_path
            .join(format!("funcs_{}.c", self.output_file_count));
        self.open(path);
        self.cur_file_function_count = 0;
        self.output_file_count += 1;
    }

    fn recompile(
        &mut self,
        context: &Context,
        func_index: usize,
        static_funcs: &mut [Vec<u32>],
    ) -> bool {
        if !self.batched() {
            let path = self
                .config
                .output_func_path
                .join(format!("{}.c", context.functions[func_index].name));
            return recompile_single_function(
                context,
                func_index,
                &self.config.recomp_include,
                &path,
                static_funcs,
            );
        }
        let mut generator = CGenerator::new();
        let result = recompile_function(&mut generator, context, func_index, static_funcs, false);
        self.current
            .as_mut()
            .expect("an output file is open")
            .1
            .push_str(&generator.out);
        if !self.config.single_file_output {
            self.cur_file_function_count += 1;
            if self.cur_file_function_count >= self.config.functions_per_output_file {
                self.open_new_output_file();
            }
        }
        result
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} <config file> [--dump-context]", args[0]);
        return ExitCode::SUCCESS;
    }
    let config_path = &args[1];
    let mut dumping_context = false;
    for arg in &args[2..] {
        if arg == "--dump-context" {
            dumping_context = true;
        } else {
            println!("Unknown argument \"{arg}\"");
            return ExitCode::FAILURE;
        }
    }

    let config = match Config::load(Path::new(config_path)) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Syntax error parsing toml: {err}");
            exit_failure(format!("Failed to load config file: {config_path}\n"));
        }
    };

    configure_rabbitizer();

    let relocatable_sections_ordered = if config.relocatable_sections_path.as_os_str().is_empty() {
        Vec::new()
    } else {
        read_list_file(&config.relocatable_sections_path).unwrap_or_else(|| {
            exit_failure(format!(
                "Failed to load the relocatable section list file: {}\n",
                config.relocatable_sections_path.display()
            ))
        })
    };

    let relocatable_sections: HashSet<String> =
        relocatable_sections_ordered.iter().cloned().collect();
    let ignored_syms_set: HashSet<String> = config.ignored_funcs.iter().cloned().collect();

    let has_elf = !config.elf_path.as_os_str().is_empty();
    let has_symbols = !config.symbols_file_path.as_os_str().is_empty();
    if has_elf && has_symbols {
        exit_failure("Config file cannot provide both an elf and a symbols file\n");
    }

    let mut context = Context::default();

    if has_elf {
        // Data symbols by section, only used when dumping context.
        let mut data_syms = DataSymbolMap::new();

        // Import reference symbols, if provided.
        if !config.func_reference_syms_file_path.as_os_str().is_empty() {
            // The function reference file has the same format as a recompilation symbol file.
            let reference = match Context::from_symbol_file(
                &config.func_reference_syms_file_path,
                Vec::new(),
                false,
            ) {
                Ok(reference) => reference,
                Err(err) => {
                    eprintln!("Syntax error parsing toml: {err}");
                    exit_failure("Failed to load provided function reference symbol file\n");
                }
            };
            if !context.import_reference_context(&reference) {
                exit_failure(
                    "Internal error: Failed to import reference context. Please report this issue.\n",
                );
            }

            for path in &config.data_reference_syms_file_paths {
                if let Err(err) = context.read_data_reference_syms(path) {
                    eprintln!("Syntax error parsing toml: {err}");
                    exit_failure(format!(
                        "Failed to load provided data reference symbol file: {}\n",
                        path.display()
                    ));
                }
            }
        }

        let mut elf_config = ElfParsingConfig {
            bss_section_suffix: config.bss_section_suffix.clone(),
            relocatable_sections,
            ignored_syms: ignored_syms_set,
            mdebug_text_map: config.mdebug_text_map.clone(),
            mdebug_data_map: config.mdebug_data_map.clone(),
            mdebug_rodata_map: config.mdebug_rodata_map.clone(),
            mdebug_bss_map: config.mdebug_bss_map.clone(),
            has_entrypoint: config.has_entrypoint,
            entrypoint_address: config.entrypoint,
            use_absolute_symbols: config.use_absolute_symbols,
            unpaired_lo16_warnings: config.unpaired_lo16_warnings,
            all_sections_relocatable: false,
            use_mdebug: config.use_mdebug,
            ..Default::default()
        };
        for size in &config.manual_func_sizes {
            elf_config
                .manually_sized_funcs
                .entry(size.func_name.clone())
                .or_insert(size.size_bytes as usize);
        }

        let (loaded, found_entrypoint_func) = match Context::from_elf_file(
            &config.elf_path,
            context,
            &elf_config,
            dumping_context,
            &mut data_syms,
        ) {
            Ok(result) => result,
            Err(err) => {
                println!("{err}");
                exit_failure("Failed to parse elf\n");
            }
        };
        context = loaded;

        add_manual_functions(&mut context, &config.manual_functions);

        if config.has_entrypoint && !found_entrypoint_func {
            exit_failure("Could not find entrypoint function\n");
        }

        if dumping_context {
            println!("Dumping context");
            // Sort the data syms by address so the output is nicer.
            for section_index in data_syms.keys().collect::<Vec<_>>() {
                if let Some(syms) = data_syms.get_mut(section_index) {
                    cxx_sort::sort_by(syms, |a, b| a.vram < b.vram);
                }
            }
            dump_context(
                &context,
                &data_syms,
                Path::new("dump.toml"),
                Path::new("data_dump.toml"),
            );
            return ExitCode::SUCCESS;
        }
    } else if has_symbols {
        if config.rom_file_path.as_os_str().is_empty() {
            exit_failure("A ROM file must be provided when using a symbols file\n");
        }
        if dumping_context {
            exit_failure("Cannot dump context when using a symbols file\n");
        }

        let rom = read_file(&config.rom_file_path);
        if rom.is_empty() {
            exit_failure(format!(
                "Failed to load ROM file: {}\n",
                config.rom_file_path.display()
            ));
        }
        context = match Context::from_symbol_file(&config.symbols_file_path, rom, true) {
            Ok(context) => context,
            Err(err) => {
                eprintln!("Syntax error parsing toml: {err}");
                exit_failure("Failed to load symbols file\n");
            }
        };

        let rename_function = |context: &mut Context, func_index: usize, new_name: String| {
            let old_name =
                std::mem::replace(&mut context.functions[func_index].name, new_name.clone());
            context.functions_by_name.remove(&old_name);
            context.functions_by_name.insert(new_name, func_index);
        };

        for func_index in 0..context.functions.len() {
            let name = context.functions[func_index].name.clone();
            if REIMPLEMENTED_FUNCS.contains(&name.as_str()) {
                rename_function(&mut context, func_index, format!("{name}_recomp"));
                context.functions[func_index].reimplemented = true;
                context.functions[func_index].ignored = true;
            } else if IGNORED_FUNCS.contains(&name.as_str()) {
                rename_function(&mut context, func_index, format!("{name}_recomp"));
                context.functions[func_index].ignored = true;
            } else if RENAMED_FUNCS.contains(&name.as_str()) {
                rename_function(&mut context, func_index, format!("{name}_recomp"));
                context.functions[func_index].ignored = false;
            }
        }

        if config.has_entrypoint {
            let candidates = context
                .functions_by_vram
                .get(&(config.entrypoint as u32))
                .cloned()
                .unwrap_or_default();
            let entrypoint = candidates
                .into_iter()
                .find(|&i| context.functions[i].rom == 0x1000);
            match entrypoint {
                Some(func_index) => {
                    rename_function(&mut context, func_index, "recomp_entrypoint".into())
                }
                None => exit_failure("No entrypoint provided in symbol file\n"),
            }
        }
    } else {
        exit_failure("Config file must provide either an elf or a symbols file\n");
    }

    println!("Function count: {}", context.functions.len());

    if let Err(err) = std::fs::create_dir_all(&config.output_func_path) {
        exit_failure(format!(
            "Failed to create {}: {err}\n",
            config.output_func_path.display()
        ));
    }

    let mut func_header = format!(
        "{}\n\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\n\n",
        config.recomp_include
    );

    let mut static_funcs_by_section: Vec<Vec<u32>> = vec![Vec::new(); context.sections.len()];

    if let Ok(dir) = std::env::current_dir() {
        println!("Working dir: {}", dir.display());
    }

    // Stub, ignore and rename functions from the config. Missing functions are errors, so
    // typos and renames between versions don't fail silently.
    for name in &config.stubbed_funcs {
        let Some(&index) = context.functions_by_name.get(name) else {
            exit_failure(format!(
                "Function {name} is stubbed out in the config file but does not exist!"
            ));
        };
        context.functions[index].stubbed = true;
    }
    for name in &config.ignored_funcs {
        let Some(&index) = context.functions_by_name.get(name) else {
            exit_failure(format!(
                "Function {name} is set as ignored in the config file but does not exist!"
            ));
        };
        context.functions[index].ignored = true;
    }
    for name in &config.renamed_funcs {
        let Some(&index) = context.functions_by_name.get(name) else {
            exit_failure(format!(
                "Function {name} is set as renamed in the config file but does not exist!"
            ));
        };
        context.functions[index].name.push_str("_recomp");
    }

    context.trace_mode = config.trace_mode;

    // Single-instruction patches. The bounds use the C++'s mix of int32 and size_t math.
    let contains_vram = |func: &Function, vram: i32| {
        let func_vram = func.vram as i32;
        let end = (func_vram as i64 as u64).wrapping_add(func.words.len() as u64 * 4);
        !(vram < func_vram || vram as i64 as u64 >= end)
    };
    let instruction_index = |func: &Function, vram: i32| {
        ((vram as i64 as u64).wrapping_sub(func.vram as i32 as i64 as u64) / 4) as usize
    };

    for patch in &config.instruction_patches {
        let Some(&index) = context.functions_by_name.get(&patch.func_name) else {
            exit_failure(format!(
                "Function {} has an instruction patch but does not exist!",
                patch.func_name
            ));
        };
        let func = &mut context.functions[index];
        if !contains_vram(func, patch.vram) {
            exit_failure(format!(
                "Function {} has an instruction patch for vram 0x{:08X} but doesn't contain that vram address!",
                patch.func_name, patch.vram as u32
            ));
        }
        let word_index = instruction_index(func, patch.vram);
        func.words[word_index] = patch.value.swap_bytes();
    }

    // Function hooks.
    for hook in &config.function_hooks {
        let Some(&index) = context.functions_by_name.get(&hook.func_name) else {
            exit_failure(format!(
                "Function {} has a function hook but does not exist!",
                hook.func_name
            ));
        };
        let func = &mut context.functions[index];
        if !contains_vram(func, hook.before_vram) {
            exit_failure(format!(
                "Function {} has a function hook for vram 0x{:08X} but doesn't contain that vram address!",
                hook.func_name, hook.before_vram as u32
            ));
        }
        // No before_vram means the start of the function (-1).
        let hook_index = if hook.before_vram != 0 {
            instruction_index(func, hook.before_vram) as i32
        } else {
            -1
        };
        if func.function_hooks.contains_key(&hook_index) {
            exit_failure(format!(
                "Function {} already has a function hook for vram 0x{:08X}!",
                hook.func_name, hook.before_vram as u32
            ));
        }
        func.function_hooks.insert(hook_index, hook.text.clone());
    }

    let single_file_path = config.output_func_path.join(cxx_path::replace_extension(
        cxx_path::stem(path_str(&config.elf_path)),
        ".c",
    ));
    let mut output = Output {
        config: &config,
        current: None,
        output_file_count: 0,
        cur_file_function_count: 0,
    };
    if config.single_file_output {
        output.open(single_file_path.clone());
    } else if config.functions_per_output_file > 1 {
        output.open_new_output_file();
    }

    let mut function_index_to_event_index: HashMap<usize, usize> = HashMap::new();

    // With exports enabled, point relocs at event functions to the events instead.
    if config.allow_exports {
        let event_section = context
            .sections
            .iter()
            .enumerate()
            .find(|(_, section)| section.name == EVENT_SECTION_NAME)
            .map(|(index, section)| (index, section.ram_addr));

        if let Some((event_section_index, event_section_vram)) = event_section {
            for section_index in 0..context.sections.len() {
                for reloc_index in 0..context.sections[section_index].relocs.len() {
                    let reloc = context.sections[section_index].relocs[reloc_index];
                    // Event symbols come from the elf itself, so skip reference symbols. Also
                    // skip R_MIPS_NONE, produced for non-relocatable reference sections.
                    if reloc.reference_symbol || reloc.kind == RelocType::R_MIPS_NONE {
                        continue;
                    }
                    if usize::from(reloc.target_section) != event_section_index {
                        continue;
                    }
                    let target_vram = reloc.target_section_offset.wrapping_add(event_section_vram);
                    let Some(func_index) =
                        context.find_function_by_vram_section(target_vram, event_section_index)
                    else {
                        exit_failure(format!(
                            "Failed to find event function with vram {target_vram}.\n"
                        ));
                    };
                    // Only R_MIPS_26 relocs may reference an event.
                    if reloc.kind != RelocType::R_MIPS_26 {
                        exit_failure(format!(
                            "Function {} is an import and cannot have its address taken.\n",
                            context.functions[func_index].name
                        ));
                    }
                    let next_event_index = function_index_to_event_index.len();
                    let event_index = *function_index_to_event_index
                        .entry(func_index)
                        .or_insert(next_event_index);

                    let reloc = &mut context.sections[section_index].relocs[reloc_index];
                    reloc.target_section_offset = 0;
                    reloc.symbol_index = event_index as u32;
                    reloc.target_section = SECTION_EVENT;
                    reloc.reference_symbol = true;
                }
            }
        }
    }

    let mut export_function_indices: Vec<usize> = Vec::new();
    let mut failed_strict_mode = false;

    for i in 0..context.functions.len() {
        let func = &context.functions[i];
        if !func.ignored && !func.words.is_empty() {
            out!(
                func_header,
                "void {}(uint8_t* rdram, recomp_context* ctx);\n",
                func.name
            );
            let func_section = &context.sections[usize::from(func.section_index)];

            if config.strict_patch_mode {
                let in_patch_section = func_section.name == PATCH_SECTION_NAME
                    || func_section.name == FORCED_PATCH_SECTION_NAME;
                let reference_symbol_found = context.reference_symbol_exists(&func.name);
                if in_patch_section && !reference_symbol_found {
                    eprintln!(
                        "Function {} is marked as a replacement, but no function with the same name was found in the reference symbols!",
                        func.name
                    );
                    failed_strict_mode = true;
                    continue;
                } else if !in_patch_section && reference_symbol_found {
                    eprintln!(
                        "Function {} is not marked as a replacement, but a function with the same name was found in the reference symbols!",
                        func.name
                    );
                    failed_strict_mode = true;
                    continue;
                }
            }
            if config.allow_exports && func_section.name == EXPORT_SECTION_NAME {
                export_function_indices.push(i);
            }

            if !output.recompile(&context, i, &mut static_funcs_by_section) {
                eprintln!("Error recompiling {}", context.functions[i].name);
                output.flush();
                std::process::exit(1);
            }
        } else if func.reimplemented {
            out!(
                func_header,
                "void {}(uint8_t* rdram, recomp_context* ctx);\n",
                func.name
            );
        }
    }

    if failed_strict_mode {
        // Like the C++: close the current file, then remove the single-file output (which
        // is the wrong path in batched mode, so batched output is left in place).
        if output.batched() {
            output.flush();
            let _ = std::fs::remove_file(&single_file_path);
        }
        exit_failure("Strict mode validation failed!\n");
    }

    // Create the static functions found during recompilation (calls into the middle of
    // sections with no symbol), which can find more statics as they're recompiled.
    for section_index in 0..context.sections.len() {
        context.sections[section_index]
            .function_addrs
            .sort_unstable();
        let mut statics_set: BTreeSet<u32> = static_funcs_by_section[section_index]
            .iter()
            .copied()
            .collect();
        let mut section_statics: Vec<u32> = statics_set.iter().copied().collect();

        let mut static_func_index = 0;
        while static_func_index < section_statics.len() {
            let static_func_addr = section_statics[static_func_index];
            let section = &context.sections[section_index];
            let section_funcs = &section.function_addrs;

            // The static ends at the next non-static function, or the end of the section.
            let mut cur_func_end = section.size.wrapping_add(section.ram_addr);
            if let Some(&next) = section_funcs.iter().find(|&&addr| addr >= static_func_addr) {
                cur_func_end = next;
            }
            // Don't overlap any later known statics.
            for &checked in &statics_set {
                if checked > static_func_addr && checked < cur_func_end {
                    cur_func_end = checked;
                }
            }

            let rom_addr = static_func_addr
                .wrapping_sub(section.ram_addr)
                .wrapping_add(section.rom_addr);
            let num_words = (cur_func_end.wrapping_sub(static_func_addr) / 4) as usize;
            let words = read_rom_words(&context.rom, rom_addr as usize, num_words);

            let new_func_index = context.functions.len();
            context.functions.push(Function::new(
                static_func_addr,
                rom_addr,
                words,
                format!("static_{section_index}_{static_func_addr:08X}"),
                section_index as u16,
                false,
                false,
            ));
            out!(
                func_header,
                "void {}(uint8_t* rdram, recomp_context* ctx);\n",
                context.functions[new_func_index].name
            );

            let prev_num_statics = static_funcs_by_section[section_index].len();
            let result = output.recompile(&context, new_func_index, &mut static_funcs_by_section);

            // Queue any new statics found while recompiling this one.
            for &new_static_vram in &static_funcs_by_section[section_index][prev_num_statics..] {
                if statics_set.insert(new_static_vram) {
                    section_statics.push(new_static_vram);
                }
            }

            if !result {
                eprintln!(
                    "Error recompiling {}",
                    context.functions[new_func_index].name
                );
                output.flush();
                std::process::exit(1);
            }
            static_func_index += 1;
        }
    }

    if config.has_entrypoint {
        let rom_name =
            cxx_path::replace_extension(cxx_path::filename(path_str(&config.elf_path)), ".z64");
        let lookup = format!(
            "{}\n\ngpr get_entrypoint_address() {{ return (gpr)(int32_t)0x{:08X}u; }}\n\nconst char* get_rom_name() {{ return \"{rom_name}\"; }}\n\n",
            config.recomp_include, config.entrypoint as u32
        );
        write_file(
            &config.output_func_path.join("lookup.cpp"),
            lookup.as_bytes(),
        );
    }

    let overlays = write_overlays(
        &config,
        &context,
        &relocatable_sections_ordered,
        &export_function_indices,
        &function_index_to_event_index,
        &mut func_header,
    );
    write_file(
        &config.output_func_path.join("recomp_overlays.inl"),
        overlays.as_bytes(),
    );

    func_header.push_str("\n#ifdef __cplusplus\n}\n#endif\n");
    write_file(
        &config.output_func_path.join("funcs.h"),
        func_header.as_bytes(),
    );
    output.flush();

    if !config.output_binary_path.as_os_str().is_empty() {
        write_file(&config.output_binary_path, &context.rom);
    }

    ExitCode::SUCCESS
}

/// `recomp_overlays.inl`: the section table, relocations, overlay indices and exports.
fn write_overlays(
    config: &Config,
    context: &Context,
    relocatable_sections_ordered: &[String],
    export_function_indices: &[usize],
    function_index_to_event_index: &HashMap<usize, usize>,
    func_header: &mut String,
) -> String {
    let mut out = format!(
        "{}\n#include \"funcs.h\"\n#include \"librecomp/sections.h\"\n\n",
        config.recomp_include
    );
    let mut section_load_table = String::from("static SectionTableEntry section_table[] = {\n");
    let mut relocatable_section_indices: HashMap<&str, usize> = HashMap::new();
    let mut written_sections = 0usize;
    // Only relocs to non-absolute reference symbols, events or manual patch symbols are
    // emitted when reference symbols are in use.
    let reference_symbol_mode = !config.func_reference_syms_file_path.as_os_str().is_empty();

    for (section_index, section) in context.sections.iter().enumerate() {
        let section_funcs = &context.section_functions[section_index];
        if !(section.has_mips32_relocs || !section_funcs.is_empty()) {
            continue;
        }
        if section.relocatable {
            relocatable_section_indices
                .entry(section.name.as_str())
                .or_insert(written_sections);
        }
        let trimmed = section.name.trim_start_matches('.');
        let funcs_array = format!("section_{section_index}_{trimmed}_funcs");
        let relocs_array = if section.relocs.is_empty() {
            "nullptr".to_owned()
        } else {
            format!("section_{section_index}_{trimmed}_relocs")
        };
        let relocs_size = if section.relocs.is_empty() {
            "0".to_owned()
        } else {
            format!("ARRLEN({relocs_array})")
        };

        out!(
            section_load_table,
            "    {{ .rom_addr = 0x{:08X}, .ram_addr = 0x{:08X}, .size = 0x{:08X}, .funcs = {funcs_array}, .num_funcs = ARRLEN({funcs_array}), .relocs = {relocs_array}, .num_relocs = {relocs_size}, .index = {section_index} }},\n",
            section.rom_addr,
            section.ram_addr,
            section.size
        );

        out!(out, "static FuncEntry {funcs_array}[] = {{\n");
        for &func_index in section_funcs {
            let func = &context.functions[func_index];
            let func_size = if func.reimplemented {
                0
            } else {
                func.words.len() * 4
            };
            if func.reimplemented
                || (!func.name.is_empty() && !func.ignored && !func.words.is_empty())
            {
                out!(
                    out,
                    "    {{ .func = {}, .offset = 0x{:08X}, .rom_size = 0x{:08X} }},\n",
                    func.name,
                    func.rom.wrapping_sub(section.rom_addr),
                    func_size
                );
            }
        }
        out.push_str("};\n");

        if !section.relocs.is_empty() {
            out!(out, "static RelocEntry {relocs_array}[] = {{\n");
            for reloc in &section.relocs {
                let emit = if reference_symbol_mode {
                    let manual_patch_symbol = is_manual_patch_symbol(reloc.target_section_offset);
                    let is_absolute = reloc.target_section == SECTION_ABSOLUTE;
                    (reloc.reference_symbol && !is_absolute)
                        || reloc.target_section == SECTION_EVENT
                        || manual_patch_symbol
                } else {
                    true
                };
                if emit {
                    let target_section_offset = if reloc.target_section == SECTION_EVENT {
                        reloc.symbol_index
                    } else {
                        reloc.target_section_offset
                    };
                    out!(
                        out,
                        "    {{ .offset = 0x{:08X}, .target_section_offset = 0x{:08X}, .target_section = {}, .type = {} }}, \n",
                        reloc.address.wrapping_sub(section.ram_addr),
                        target_section_offset,
                        reloc.target_section,
                        reloc.kind.output_name()
                    );
                }
            }
            out.push_str("};\n");
        }

        written_sections += 1;
    }
    section_load_table.push_str("};\n");
    out.push_str(&section_load_table);

    out!(
        out,
        "const size_t num_sections = {};\n",
        context.sections.len()
    );

    out.push_str("static int overlay_sections_by_index[] = {\n");
    if relocatable_sections_ordered.is_empty() {
        out.push_str("    -1,\n");
    } else {
        for section in relocatable_sections_ordered {
            // "*" is an empty overlay.
            if section == "*" {
                out.push_str("    -1,\n");
            } else {
                let Some(index) = relocatable_section_indices.get(section.as_str()) else {
                    exit_failure(format!(
                        "Failed to find written section index of relocatable section: {section}\n"
                    ));
                };
                out!(out, "    {index},\n");
            }
        }
    }
    out.push_str("};\n");

    if config.allow_exports {
        // The exported function table, ending with a dummy entry (C has no zero-size arrays).
        out.push_str("\nstatic FunctionExport export_table[] = {\n");
        for &func_index in export_function_indices {
            let func = &context.functions[func_index];
            out!(out, "    {{ \"{}\", 0x{:08X} }},\n", func.name, func.vram);
        }
        out.push_str("    { NULL, 0 }\n};\n");

        // The event table.
        let mut functions_by_event = vec![0usize; function_index_to_event_index.len()];
        for (&func_index, &event_index) in function_index_to_event_index {
            functions_by_event[event_index] = func_index;
        }
        out.push_str("\nstatic const char* event_names[] = {\n");
        for func_index in functions_by_event {
            out!(out, "    \"{}\",\n", context.functions[func_index].name);
        }
        out.push_str("    NULL\n};\n");

        // Manual patch symbols, sorted by vram.
        let mut manual_patch_syms: Vec<(u32, &str)> = context
            .functions
            .iter()
            .filter(|func| func.words.is_empty() && is_manual_patch_symbol(func.vram))
            .map(|func| (func.vram, func.name.as_str()))
            .collect();
        cxx_sort::sort_by(&mut manual_patch_syms, |a, b| a.0 < b.0);

        out.push_str("\nstatic const ManualPatchSymbol manual_patch_symbols[] = {\n");
        for (vram, name) in manual_patch_syms {
            out!(out, "    {{ 0x{vram:08X}, {name} }},\n");
            out!(
                func_header,
                "void {name}(uint8_t* rdram, recomp_context* ctx);\n"
            );
        }
        out.push_str("    { 0, NULL }\n};\n");
    }

    out
}
