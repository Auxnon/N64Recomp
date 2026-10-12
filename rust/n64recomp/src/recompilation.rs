//! Function recompilation (`src/recompilation.cpp`).

use std::collections::{BTreeSet, HashSet};

use rabbitizer::{InstrId, Instruction};

use crate::analysis::{FunctionStats, analyze_function};
use crate::context::{
    Context, Function, RelocType, SECTION_ABSOLUTE, SECTION_EVENT, is_manual_patch_symbol,
};
use crate::generator::{Generator, InstructionContext};
use crate::instr::{Fields, decode_cpu};
use crate::operations::{
    Operand, StoreOpType, binary_op, conditional_branch_op, store_op, unary_op,
};

const COP0_STATUS: u32 = 12;
const REG_RA: u32 = 31;

enum JalResolution {
    NoMatch,
    Match(usize),
    CreateStatic,
    Ambiguous,
}

fn resolve_jal(
    context: &Context,
    cur_section_index: usize,
    target_func_vram: u32,
) -> JalResolution {
    // Skip resolution if all function calls should use lookup.
    if context.use_lookup_for_all_function_calls {
        return JalResolution::Ambiguous;
    }

    let cur_section = &context.sections[cur_section_index];
    let section_end = cur_section.ram_addr.wrapping_add(cur_section.size);
    let in_current_section =
        target_func_vram >= cur_section.ram_addr && target_func_vram < section_end;
    let mut exact_match_found = false;
    let mut matched_funcs: Vec<usize> = Vec::new();

    // Evaluate functions at the target address as candidates.
    if let Some(candidates) = context.functions_by_vram.get(&target_func_vram) {
        for &target_func_index in candidates {
            let target_func = &context.functions[target_func_index];

            // Skip zero-sized symbols, except manual patch symbols.
            if target_func.words.is_empty() && !is_manual_patch_symbol(target_func.vram) {
                continue;
            }

            // Accept a function in the same section immediately, since it must also be
            // loaded if the current function is.
            if usize::from(target_func.section_index) == cur_section_index {
                exact_match_found = true;
                matched_funcs.clear();
                matched_funcs.push(target_func_index);
                break;
            }

            // Functions in non-relocatable sections are candidates.
            if !context.sections[usize::from(target_func.section_index)].relocatable {
                matched_funcs.push(target_func_index);
            }
        }
    }

    if in_current_section {
        // Inside the current section, only exact matches count; otherwise create a static.
        if exact_match_found {
            JalResolution::Match(matched_funcs[0])
        } else {
            JalResolution::CreateStatic
        }
    } else {
        match matched_funcs.len() {
            // A static can't be created, as the target section is unknown.
            0 => JalResolution::NoMatch,
            1 => JalResolution::Match(matched_funcs[0]),
            // More than one match: resolve at runtime with a lookup.
            _ => JalResolution::Ambiguous,
        }
    }
}

/// Relocation data for the instruction being processed.
#[derive(Clone, Copy)]
struct InstrReloc {
    has_reloc: bool,
    section: u32,
    target_section_offset: u32,
    /// The reference symbol index (the C++ uses `(size_t)-1` for none).
    reference_symbol: Option<usize>,
    kind: RelocType,
}

/// What the per-instruction helpers need to know about the current instruction.
#[derive(Clone, Copy)]
struct Current {
    instr_index: usize,
    instr_vram: u32,
    reloc_index: usize,
    link_branch_index: i32,
    reloc: InstrReloc,
}

struct FunctionRecompiler<'a, G: Generator> {
    generator: &'a mut G,
    context: &'a Context,
    func: &'a Function,
    func_index: usize,
    func_vram_end: u32,
    stats: &'a FunctionStats,
    jtbl_lw_instructions: &'a HashSet<u32>,
    instructions: &'a [Instruction],
    tag_reference_relocs: bool,
    static_funcs_out: &'a mut [Vec<u32>],
}

impl<G: Generator> FunctionRecompiler<'_, G> {
    fn indent(&mut self) {
        self.generator.raw("    ");
    }

    fn process_delay_slot(&mut self, cur: &Current, use_indent: bool) -> bool {
        if cur.instr_index < self.instructions.len() - 1 {
            let relocs = &self.context.sections[usize::from(self.func.section_index)].relocs;
            let mut next_reloc_index = cur.reloc_index;
            let next_vram = cur.instr_vram.wrapping_add(4);
            if cur.reloc_index + 1 < relocs.len() && next_vram > relocs[cur.reloc_index].address {
                next_reloc_index += 1;
            }
            let (mut dummy_link, mut dummy_likely) = (false, false);
            if !self.process_instruction(
                cur.instr_index + 1,
                use_indent,
                false,
                cur.link_branch_index,
                next_reloc_index,
                &mut dummy_link,
                &mut dummy_likely,
            ) {
                return false;
            }
        }
        true
    }

    fn print_link_branch(&mut self, cur: &Current, needs_link_branch: bool) {
        if needs_link_branch {
            self.indent();
            self.generator
                .emit_goto(&format!("after_{}", cur.link_branch_index));
        }
    }

    fn print_return_with_delay_slot(&mut self, cur: &Current, needs_link_branch: bool) -> bool {
        if !self.process_delay_slot(cur, false) {
            return false;
        }
        self.indent();
        self.generator.emit_return(self.context, self.func_index);
        self.print_link_branch(cur, needs_link_branch);
        true
    }

    fn print_goto_with_delay_slot(
        &mut self,
        cur: &Current,
        needs_link_branch: bool,
        target: &str,
    ) -> bool {
        if !self.process_delay_slot(cur, false) {
            return false;
        }
        self.indent();
        self.generator.emit_goto(target);
        self.print_link_branch(cur, needs_link_branch);
        true
    }

    fn print_func_call_by_register(
        &mut self,
        cur: &Current,
        needs_link_branch: bool,
        reg: u32,
    ) -> bool {
        if !self.process_delay_slot(cur, false) {
            return false;
        }
        self.indent();
        self.generator.emit_function_call_by_register(reg);
        self.print_link_branch(cur, needs_link_branch);
        true
    }

    fn print_func_call_by_address(
        &mut self,
        cur: &Current,
        needs_link_branch: &mut bool,
        target_func_vram: u32,
        tail_call: bool,
        indent: bool,
    ) -> bool {
        let reloc = cur.reloc;
        // Event symbol: call the runtime to trigger the event.
        if reloc.section == u32::from(SECTION_EVENT) {
            *needs_link_branch = !tail_call;
            if indent {
                self.indent();
            }
            if !self.process_delay_slot(cur, false) {
                return false;
            }
            self.indent();
            self.generator
                .emit_trigger_event(reloc.reference_symbol.map_or(u32::MAX, |i| i as u32));
            self.print_link_branch(cur, *needs_link_branch);
            return true;
        }

        // Normal symbol or reference symbol.
        enum Call {
            ReferenceSymbol(usize),
            Lookup,
            Named(String),
            Function(usize),
        }
        let call = if let Some(symbol) = reloc.reference_symbol {
            if reloc.kind != RelocType::R_MIPS_26 {
                eprintln!(
                    "Unsupported reloc type {} on jal instruction in {}",
                    reloc.kind.0, self.func.name
                );
                return false;
            }
            if !self.context.skip_validating_reference_symbols {
                let ref_symbol = self
                    .context
                    .get_reference_symbol(reloc.section as u16, symbol);
                if ref_symbol.section_offset != reloc.target_section_offset {
                    eprintln!(
                        "Function {} uses a MIPS_R_26 addend, which is not supported yet",
                        self.func.name
                    );
                    return false;
                }
            }
            Call::ReferenceSymbol(symbol)
        } else {
            let mut target_section = usize::from(self.func.section_index);
            // With a reloc into a normal section, search that section for the target.
            if reloc.has_reloc && reloc.section < 65500 {
                target_section = reloc.section as usize;
            }
            match resolve_jal(self.context, target_section, target_func_vram) {
                JalResolution::NoMatch => {
                    eprintln!("No function found for jal target: 0x{target_func_vram:08X}");
                    return false;
                }
                JalResolution::Match(index) => Call::Function(index),
                JalResolution::CreateStatic => {
                    // Create a static function and add it to this section's statics.
                    self.static_funcs_out[usize::from(self.func.section_index)]
                        .push(target_func_vram);
                    Call::Named(format!(
                        "static_{}_{:08X}",
                        self.func.section_index, target_func_vram
                    ))
                }
                JalResolution::Ambiguous => {
                    if !self.context.use_lookup_for_all_function_calls {
                        eprintln!(
                            "[Info] Ambiguous jal target 0x{target_func_vram:08X} in function {}, falling back to function lookup",
                            self.func.name
                        );
                    }
                    // Relocation isn't needed here: this path never runs for targets in the
                    // current function's section.
                    Call::Lookup
                }
            }
        };

        *needs_link_branch = !tail_call;
        if indent {
            self.indent();
        }
        if !self.process_delay_slot(cur, false) {
            return false;
        }
        self.indent();
        match call {
            Call::ReferenceSymbol(symbol) => self.generator.emit_function_call_reference_symbol(
                self.context,
                reloc.section as u16,
                symbol,
                reloc.target_section_offset,
            ),
            Call::Lookup => self.generator.emit_function_call_lookup(target_func_vram),
            Call::Named(name) => self.generator.emit_named_function_call(&name),
            Call::Function(index) => self.generator.emit_function_call(self.context, index),
        }
        self.print_link_branch(cur, *needs_link_branch);
        true
    }

    fn print_branch(
        &mut self,
        cur: &Current,
        needs_link_branch: &mut bool,
        branch_target: u32,
    ) -> bool {
        // A branch outside the function may be a tail call.
        if branch_target < self.func.vram || branch_target >= self.func_vram_end {
            // FIXME (from the C++): how to deal with static functions?
            if self.context.functions_by_vram.contains_key(&branch_target) {
                println!("Tail call in {} to 0x{branch_target:08X}", self.func.name);
                if !self.print_func_call_by_address(
                    cur,
                    needs_link_branch,
                    branch_target,
                    true,
                    true,
                ) {
                    return false;
                }
                self.indent();
                self.generator.emit_return(self.context, self.func_index);
                return true;
            }
            eprintln!(
                "[Warn] Function {} is branching outside of the function (to 0x{branch_target:08X})",
                self.func.name
            );
        }

        if !self.process_delay_slot(cur, true) {
            return false;
        }
        self.indent();
        self.indent();
        self.generator.emit_goto(&format!("L_{branch_target:08X}"));
        if *needs_link_branch {
            self.indent();
            self.indent();
            self.generator
                .emit_goto(&format!("after_{}", cur.link_branch_index));
        }
        true
    }

    fn check_fr(&mut self, ctx: &InstructionContext, operand: Operand) {
        use Operand::*;
        match operand {
            Fd | FdDouble | FdU32L | FdU32H | FdU64 => self.generator.emit_check_fr(ctx.fd),
            Fs | FsDouble | FsU32L | FsU32H | FsU64 => self.generator.emit_check_fr(ctx.fs),
            Ft | FtDouble | FtU32L | FtU32H | FtU64 => self.generator.emit_check_fr(ctx.ft),
            // No MIPS3 float check needed for non-float operands.
            _ => {}
        }
    }

    fn check_nan(&mut self, ctx: &InstructionContext, operand: Operand) {
        use Operand::*;
        match operand {
            Fd => self.generator.emit_check_nan(ctx.fd, false),
            Fs => self.generator.emit_check_nan(ctx.fs, false),
            Ft => self.generator.emit_check_nan(ctx.ft, false),
            FdDouble => self.generator.emit_check_nan(ctx.fd, true),
            FsDouble => self.generator.emit_check_nan(ctx.fs, true),
            FtDouble => self.generator.emit_check_nan(ctx.ft, true),
            // No NaN checks needed for non-float operands.
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process_instruction(
        &mut self,
        instr_index: usize,
        indent: bool,
        emit_link_branch: bool,
        link_branch_index: i32,
        reloc_index: usize,
        needs_link_branch: &mut bool,
        is_branch_likely: &mut bool,
    ) -> bool {
        // Copy the shared references out so they don't borrow `self`.
        let (context, func, instructions, stats) =
            (self.context, self.func, self.instructions, self.stats);
        let section = &context.sections[usize::from(func.section_index)];
        let instr = &instructions[instr_index];
        *needs_link_branch = false;
        *is_branch_likely = false;
        let instr_vram = instr.vram;
        let mut instr_id = instr.unique_id;

        if let Some(hook) = func.function_hooks.get(&(instr_index as i32)) {
            self.generator.raw(&format!("    {hook}\n"));
            if indent {
                self.indent();
            }
        }

        // Output a comment with the original instruction.
        self.indent();
        let branch_vram = instr.branch_vram_generic() as u32;
        let disassembly = if instr.is_branch() || instr_id == InstrId::cpu_j {
            instr.disassemble(Some(&format!("L_{branch_vram:08X}")), 0)
        } else if instr_id == InstrId::cpu_jal {
            instr.disassemble(Some(&format!("0x{branch_vram:08X}")), 0)
        } else {
            instr.disassemble(None, 0)
        };
        self.generator
            .emit_comment(&format!("0x{instr_vram:08X}: {disassembly}"));

        // Loads of jump table entries become addiu, which leaves the entry's address in the
        // output register so the offset into the table can be computed.
        if self.jtbl_lw_instructions.contains(&instr_vram) {
            debug_assert_eq!(instr_id, InstrId::cpu_lw);
            instr_id = InstrId::cpu_addiu;
        }

        let mut reloc = InstrReloc {
            has_reloc: false,
            section: 0,
            target_section_offset: 0,
            reference_symbol: None,
            kind: RelocType::R_MIPS_NONE,
        };

        let f = Fields::of(instr);

        // Check if this instruction has a reloc.
        if !section.relocs.is_empty() && section.relocs[reloc_index].address == instr_vram {
            reloc.has_reloc = true;
            let r = &section.relocs[reloc_index];
            reloc.section = u32::from(r.target_section);

            // Only process the reloc if it targets a relocatable section or a reference symbol.
            let target_relocatable = !r.reference_symbol
                && reloc.section != u32::from(SECTION_ABSOLUTE)
                && context.sections[reloc.section as usize].relocatable;

            if target_relocatable || r.reference_symbol {
                reloc.kind = r.kind;
                reloc.target_section_offset = r.target_section_offset;
                // Ignore all relocs that aren't HI16, LO16 or 26.
                if matches!(
                    r.kind,
                    RelocType::R_MIPS_HI16 | RelocType::R_MIPS_LO16 | RelocType::R_MIPS_26
                ) && r.reference_symbol
                {
                    reloc.reference_symbol = Some(r.symbol_index as usize);
                    // Don't try to relocate special section symbols.
                    if context.is_regular_reference_section(r.target_section)
                        || reloc.section == u32::from(SECTION_ABSOLUTE)
                    {
                        // HI16/LO16 reference relocs to non-relocatable sections were already
                        // resolved by patching the immediate, so drop them here.
                        let ref_section_relocatable =
                            context.is_reference_section_relocatable(r.target_section);
                        if !ref_section_relocatable
                            && matches!(r.kind, RelocType::R_MIPS_HI16 | RelocType::R_MIPS_LO16)
                        {
                            reloc.kind = RelocType::R_MIPS_NONE;
                            reloc.reference_symbol = None;
                        }
                    }
                }

                // Repoint bss relocations at their non-bss counterpart section.
                if let Some(&target) = context.bss_section_to_section.get(&(reloc.section as u16)) {
                    reloc.section = u32::from(target);
                }
            }
        }

        let cur = Current {
            instr_index,
            instr_vram,
            reloc_index,
            link_branch_index,
            reloc,
        };

        if indent {
            self.indent();
        }

        let (rd, rs, rt) = (f.rd, f.rs, f.rt);
        let mut handled = true;

        use InstrId::*;
        match instr_id {
            cpu_nop => self.generator.raw("\n"),
            // Cop0 (limited functionality)
            cpu_mfc0 => {
                if f.cop != COP0_STATUS {
                    eprintln!("Unhandled cop0 register in mfc0: {}", f.cop);
                    return false;
                }
                self.indent();
                self.generator.emit_cop0_status_read(rt);
            }
            cpu_mtc0 => {
                if f.cop != COP0_STATUS {
                    eprintln!("Unhandled cop0 register in mtc0: {}", f.cop);
                    return false;
                }
                self.indent();
                self.generator.emit_cop0_status_write(rt);
            }
            // Arithmetic: an addu that belongs to a jump table load preserves the addend.
            cpu_add | cpu_addu => {
                if let Some(jtbl) = stats.jump_tables.iter().find(|j| j.addu_vram == instr_vram) {
                    self.indent();
                    self.generator
                        .emit_jtbl_addend_declaration(jtbl, jtbl.addend_reg);
                }
            }
            cpu_mult | cpu_dmult | cpu_multu | cpu_dmultu | cpu_div | cpu_ddiv | cpu_divu
            | cpu_ddivu => {
                self.indent();
                self.generator.emit_muldiv(instr_id, rs, rt);
            }
            // Branches
            cpu_jal => {
                if !self.print_func_call_by_address(
                    &cur,
                    needs_link_branch,
                    branch_vram,
                    false,
                    false,
                ) {
                    return false;
                }
            }
            cpu_jalr => {
                // jalr can only be handled with $ra as the return address register.
                if rd != REG_RA {
                    eprintln!("Invalid return address reg for jalr: f{rd}");
                    return false;
                }
                *needs_link_branch = true;
                // The C++ ignores this result.
                self.print_func_call_by_register(&cur, *needs_link_branch, rs);
            }
            cpu_j | cpu_b => {
                let branch_target = branch_vram;
                if branch_target == instr_vram {
                    self.indent();
                    self.generator.emit_pause_self();
                } else if branch_target >= self.func.vram && branch_target < self.func_vram_end {
                    // The C++ ignores this result.
                    self.print_goto_with_delay_slot(
                        &cur,
                        *needs_link_branch,
                        &format!("L_{branch_target:08X}"),
                    );
                } else if context.functions_by_vram.contains_key(&branch_target) {
                    // A tail call in the middle of the control flow.
                    // FIXME (from the C++): how to deal with static functions?
                    println!(
                        "[Info] Tail call in {} to 0x{branch_target:08X}",
                        self.func.name
                    );
                    if !self.print_func_call_by_address(
                        &cur,
                        needs_link_branch,
                        branch_target,
                        true,
                        false,
                    ) {
                        return false;
                    }
                    self.indent();
                    self.generator.emit_return(context, self.func_index);
                } else {
                    eprintln!(
                        "Unhandled branch in {} at 0x{instr_vram:08X} to 0x{branch_target:08X}",
                        self.func.name
                    );
                    return false;
                }
            }
            cpu_jr => {
                if rs == REG_RA {
                    // The C++ ignores this result.
                    self.print_return_with_delay_slot(&cur, *needs_link_branch);
                } else if let Some(jtbl) =
                    stats.jump_tables.iter().find(|j| j.jr_vram == instr_vram)
                {
                    if !self.process_delay_slot(&cur, false) {
                        return false;
                    }
                    self.indent();
                    self.generator.emit_switch(context, jtbl, rs);
                    for (entry_index, entry) in jtbl.entries.iter().enumerate() {
                        self.indent();
                        self.indent();
                        self.generator
                            .emit_case(entry_index, &format!("L_{entry:08X}"));
                    }
                    self.indent();
                    self.indent();
                    self.generator.emit_switch_error(instr_vram, jtbl.vram);
                    self.indent();
                    self.generator.emit_switch_close();
                } else {
                    println!("[Info] Indirect tail call in {}", self.func.name);
                    // The C++ ignores this result.
                    self.print_func_call_by_register(&cur, *needs_link_branch, rs);
                    self.indent();
                    self.generator.emit_return(context, self.func_index);
                }
            }
            cpu_syscall => {
                self.indent();
                self.generator.emit_syscall(instr_vram);
                // syscalls don't link, so treat them like a tail call.
                self.indent();
                self.generator.emit_return(context, self.func_index);
            }
            cpu_break => {
                self.indent();
                self.generator.emit_do_break(instr_vram);
            }
            // Cop1 rounding mode
            cpu_ctc1 => {
                if f.cop != 31 {
                    eprintln!("Invalid FP control register for ctc1: {}", f.cop);
                    return false;
                }
                self.indent();
                self.generator.emit_cop1_cs_write(rt);
            }
            cpu_cfc1 => {
                if f.cop != 31 {
                    eprintln!("Invalid FP control register for cfc1: {}", f.cop);
                    return false;
                }
                self.indent();
                self.generator.emit_cop1_cs_read(rt);
            }
            _ => handled = false,
        }

        let ictx = InstructionContext {
            rd,
            rs,
            rt,
            sa: f.sa,
            fd: f.fd,
            fs: f.fs,
            ft: f.ft,
            cop1_cs: f.cop,
            imm16: f.imm,
            reloc_tag_as_reference: reloc.reference_symbol.is_some() && self.tag_reference_relocs,
            reloc_type: reloc.kind,
            reloc_section_index: reloc.section,
            reloc_target_section_offset: reloc.target_section_offset,
        };

        if let Some(op) = binary_op(instr_id) {
            self.indent();
            if op.check_fr {
                self.check_fr(&ictx, op.output);
                self.check_fr(&ictx, op.operands.operands[0]);
                self.check_fr(&ictx, op.operands.operands[1]);
            }
            if op.check_nan {
                self.check_nan(&ictx, op.operands.operands[0]);
                self.check_nan(&ictx, op.operands.operands[1]);
                self.generator.raw("\n");
                self.indent();
            }
            self.generator.process_binary_op(&op, &ictx);
            handled = true;
        }

        if let Some(op) = unary_op(instr_id) {
            self.indent();
            if op.check_fr {
                self.check_fr(&ictx, op.output);
                self.check_fr(&ictx, op.input);
            }
            if op.check_nan {
                self.check_nan(&ictx, op.input);
                self.generator.raw("\n");
                self.indent();
            }
            self.generator.process_unary_op(&op, &ictx);
            handled = true;
        }

        if let Some(op) = conditional_branch_op(instr_id) {
            self.indent();
            // TODO (from the C++): combining the condition and target into one generator
            // call would allow better optimization in the runtime's JIT generator.
            self.generator.emit_branch_condition(&op, &ictx);
            self.indent();
            let ok = if op.link {
                self.print_func_call_by_address(&cur, needs_link_branch, branch_vram, false, false)
            } else {
                self.print_branch(&cur, needs_link_branch, branch_vram)
            };
            if !ok {
                return false;
            }
            self.indent();
            self.generator.emit_branch_close();
            *is_branch_likely = op.likely;
            handled = true;
        }

        if let Some(op) = store_op(instr_id) {
            self.indent();
            if op.kind == StoreOpType::Sdc1 {
                self.check_fr(&ictx, op.value_input);
            }
            self.generator.process_store_op(&op, &ictx);
            handled = true;
        }

        if !handled {
            eprintln!("Unhandled instruction: {}", instr.opcode_name());
            return false;
        }

        // TODO (from the C++): is this used?
        if emit_link_branch {
            self.indent();
            self.generator
                .emit_label(&format!("after_{link_branch_index}"));
        }

        true
    }
}

/// Recompiles one function into `generator`. Calls to functions that don't exist yet are
/// recorded in `static_funcs_out` (by section) so the caller can create them.
pub fn recompile_function<G: Generator>(
    generator: &mut G,
    context: &Context,
    func_index: usize,
    static_funcs_out: &mut [Vec<u32>],
    tag_reference_relocs: bool,
) -> bool {
    let func = &context.functions[func_index];
    generator.emit_function_start(&func.name, func_index);

    if context.trace_mode {
        generator.raw("    TRACE_ENTRY()\n");
    }

    // Skip analysis and recompilation if this function is stubbed.
    if !func.stubbed {
        // Sorted, deduplicated labels.
        let mut branch_labels: BTreeSet<u32> = BTreeSet::new();

        if let Some(hook) = func.function_hooks.get(&-1) {
            generator.raw(&format!("    {hook}\n"));
        }

        // First pass: decode each instruction and collect branch labels.
        let instructions: Vec<Instruction> = func
            .words
            .iter()
            .zip((func.vram..).step_by(4))
            .map(|(&word, vram)| decode_cpu(word, vram))
            .collect();
        for instr in &instructions {
            if instr.is_branch() || instr.unique_id == InstrId::cpu_j {
                branch_labels.insert(instr.branch_vram_generic() as u32);
            }
        }

        // Analyze the function.
        let Some(stats) = analyze_function(context, func, &instructions) else {
            eprintln!("Failed to analyze {}", func.name);
            return false;
        };

        // Jump table loads, and labels for every jump table entry.
        let mut jtbl_lw_instructions = HashSet::new();
        for jtbl in &stats.jump_tables {
            jtbl_lw_instructions.insert(jtbl.lw_vram);
            branch_labels.extend(jtbl.entries.iter().copied());
        }

        let mut recompiler = FunctionRecompiler {
            generator: &mut *generator,
            context,
            func,
            func_index,
            func_vram_end: func.vram.wrapping_add((func.words.len() * 4) as u32),
            stats: &stats,
            jtbl_lw_instructions: &jtbl_lw_instructions,
            instructions: &instructions,
            tag_reference_relocs,
            static_funcs_out,
        };

        // Second pass: emit code for each instruction, and the labels.
        let mut labels = branch_labels.iter().copied().peekable();
        let mut vram = func.vram;
        let mut num_link_branches = 0i32;
        let mut num_likely_branches = 0;
        let mut needs_link_branch = false;
        let mut in_likely_delay_slot = false;
        let relocs = &context.sections[usize::from(func.section_index)].relocs;
        let mut reloc_index = 0;
        for instr_index in 0..instructions.len() {
            let had_link_branch = needs_link_branch;
            let mut is_branch_likely = false;
            // In the delay slot of a likely branch, skip the instruction before any labels.
            if in_likely_delay_slot {
                recompiler
                    .generator
                    .emit_goto(&format!("skip_{num_likely_branches}"));
            }
            // Insert the next branch label if we're at (or past) it. Only one per instruction.
            if let Some(&label) = labels.peek() {
                if vram >= label {
                    recompiler.generator.emit_label(&format!("L_{label:08X}"));
                    labels.next();
                }
            }

            // Advance to the last reloc, or to the first at or after this instruction.
            while reloc_index + 1 < relocs.len() && relocs[reloc_index].address < vram {
                reloc_index += 1;
            }

            if !recompiler.process_instruction(
                instr_index,
                false,
                needs_link_branch,
                num_link_branches,
                reloc_index,
                &mut needs_link_branch,
                &mut is_branch_likely,
            ) {
                eprintln!("Error in recompiling {}, clearing output file", func.name);
                return false;
            }
            if had_link_branch {
                num_link_branches += 1;
            }
            // The likely branch's delay slot has been emitted, so emit its skip label.
            if in_likely_delay_slot {
                recompiler.generator.raw("    ");
                recompiler
                    .generator
                    .emit_label(&format!("skip_{num_likely_branches}"));
                num_likely_branches += 1;
            }
            in_likely_delay_slot = is_branch_likely;
            vram = vram.wrapping_add(4);
        }
    }

    generator.emit_function_end();
    true
}
