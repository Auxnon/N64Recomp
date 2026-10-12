//! Code generators (`include/recompiler/generator.h`, `src/cgenerator.cpp`).
//!
//! The recompiler walks a function's instructions and calls a [`Generator`] for each piece
//! of output. [`CGenerator`] writes the C that N64Recomp has always produced.

use std::fmt::Write as _;

use rabbitizer::InstrId;
use recomp_core::cxx_fmt::hex_alt;

use crate::context::{Context, JumpTable, RelocType};
use crate::operations::{
    BinaryOp, BinaryOpType, BinaryOperands, ConditionalBranchOp, Operand, StoreOp, StoreOpType,
    UnaryOp, UnaryOpType,
};

/// The decoded fields of the instruction being generated.
#[derive(Clone, Copy, Debug, Default)]
pub struct InstructionContext {
    pub rd: u32,
    pub rs: u32,
    pub rt: u32,
    pub sa: u32,
    pub fd: u32,
    pub fs: u32,
    pub ft: u32,
    pub cop1_cs: u32,
    pub imm16: u16,
    pub reloc_tag_as_reference: bool,
    pub reloc_type: RelocType,
    pub reloc_section_index: u32,
    pub reloc_target_section_offset: u32,
}

pub trait Generator {
    /// Writes text that the recompiler emits directly (indentation, hook text, blank lines).
    fn raw(&mut self, text: &str);

    fn process_binary_op(&mut self, op: &BinaryOp, ctx: &InstructionContext);
    fn process_unary_op(&mut self, op: &UnaryOp, ctx: &InstructionContext);
    fn process_store_op(&mut self, op: &StoreOp, ctx: &InstructionContext);
    fn emit_function_start(&mut self, function_name: &str, func_index: usize);
    fn emit_function_end(&mut self);
    fn emit_function_call_lookup(&mut self, addr: u32);
    fn emit_function_call_by_register(&mut self, reg: u32);
    /// `target_section_offset` can be deduced from the symbol when the full context is
    /// available, but live recompilation doesn't have the reference symbol list.
    fn emit_function_call_reference_symbol(
        &mut self,
        context: &Context,
        section_index: u16,
        symbol_index: usize,
        target_section_offset: u32,
    );
    fn emit_function_call(&mut self, context: &Context, function_index: usize);
    fn emit_named_function_call(&mut self, function_name: &str);
    fn emit_goto(&mut self, target: &str);
    fn emit_label(&mut self, label_name: &str);
    fn emit_jtbl_addend_declaration(&mut self, jtbl: &JumpTable, reg: u32);
    fn emit_branch_condition(&mut self, op: &ConditionalBranchOp, ctx: &InstructionContext);
    fn emit_branch_close(&mut self);
    fn emit_switch(&mut self, context: &Context, jtbl: &JumpTable, reg: u32);
    fn emit_case(&mut self, case_index: usize, target_label: &str);
    fn emit_switch_error(&mut self, instr_vram: u32, jtbl_vram: u32);
    fn emit_switch_close(&mut self);
    fn emit_return(&mut self, context: &Context, func_index: usize);
    fn emit_check_fr(&mut self, fpr: u32);
    fn emit_check_nan(&mut self, fpr: u32, is_double: bool);
    fn emit_cop0_status_read(&mut self, reg: u32);
    fn emit_cop0_status_write(&mut self, reg: u32);
    fn emit_cop1_cs_read(&mut self, reg: u32);
    fn emit_cop1_cs_write(&mut self, reg: u32);
    fn emit_muldiv(&mut self, instr_id: InstrId, reg1: u32, reg2: u32);
    fn emit_syscall(&mut self, instr_vram: u32);
    fn emit_do_break(&mut self, instr_vram: u32);
    fn emit_pause_self(&mut self);
    fn emit_trigger_event(&mut self, event_index: u32);
    fn emit_comment(&mut self, comment: &str);
}

/// Appends formatted text to a `String` (which can't fail).
macro_rules! out {
    ($out:expr, $($arg:tt)*) => {{
        let _ = write!($out, $($arg)*);
    }};
}

/// `func_string` / `infix_string` for each binary operation (`c_op_fields`).
fn notation(kind: BinaryOpType) -> (&'static str, &'static str) {
    use BinaryOpType::*;
    match kind {
        Add32 => ("ADD32", ""),
        Sub32 => ("SUB32", ""),
        Add64 => ("", "+"),
        Sub64 => ("", "-"),
        And64 => ("", "&"),
        AddFloat | AddDouble => ("", "+"),
        SubFloat | SubDouble => ("", "-"),
        MulFloat => ("MUL_S", ""),
        MulDouble => ("MUL_D", ""),
        DivFloat => ("DIV_S", ""),
        DivDouble => ("DIV_D", ""),
        Or64 => ("", "|"),
        Nor64 => ("~", "|"),
        Xor64 => ("", "^"),
        Sll32 => ("S32", "<<"),
        Sll64 => ("", "<<"),
        Srl32 => ("S32", ">>"),
        Srl64 => ("", ">>"),
        // The arithmetic part is handled by the first operand's unary op.
        Sra32 => ("S32", ">>"),
        Sra64 => ("", ">>"),
        Equal | EqualFloat | EqualDouble => ("", "=="),
        NotEqual => ("", "!="),
        Less | LessFloat | LessDouble => ("", "<"),
        LessEq | LessEqFloat | LessEqDouble => ("", "<="),
        Greater => ("", ">"),
        GreaterEq => ("", ">="),
        Ld => ("LD", ""),
        Lw => ("MEM_W", ""),
        Lwu => ("MEM_WU", ""),
        Lh => ("MEM_H", ""),
        Lhu => ("MEM_HU", ""),
        Lb => ("MEM_B", ""),
        Lbu => ("MEM_BU", ""),
        Ldl => ("do_ldl", ""),
        Ldr => ("do_ldr", ""),
        Lwl => ("do_lwl", ""),
        Lwr => ("do_lwr", ""),
        True | False => ("", ""),
    }
}

fn gpr_to_string(gpr_index: u32) -> String {
    if gpr_index == 0 {
        "0".to_owned()
    } else {
        format!("ctx->r{gpr_index}")
    }
}

fn fpr_to_string(fpr_index: u32) -> String {
    format!("ctx->f{fpr_index}.fl")
}

fn fpr_double_to_string(fpr_index: u32) -> String {
    format!("ctx->f{fpr_index}.d")
}

fn fpr_u32l_to_string(fpr_index: u32) -> String {
    if fpr_index & 1 != 0 {
        format!("ctx->f_odd[({fpr_index} - 1) * 2]")
    } else {
        format!("ctx->f{fpr_index}.u32l")
    }
}

fn fpr_u64_to_string(fpr_index: u32) -> String {
    format!("ctx->f{fpr_index}.u64")
}

fn unsigned_reloc(ctx: &InstructionContext) -> String {
    let prefix = if ctx.reloc_tag_as_reference {
        "REF_"
    } else {
        ""
    };
    let name = match ctx.reloc_type {
        RelocType::R_MIPS_HI16 => "RELOC_HI16",
        RelocType::R_MIPS_LO16 => "RELOC_LO16",
        // The C++ throws here.
        other => panic!("Unexpected reloc type {}", other.0),
    };
    format!(
        "{prefix}{name}({}, {})",
        ctx.reloc_section_index,
        hex_alt(ctx.reloc_target_section_offset)
    )
}

fn signed_reloc(ctx: &InstructionContext) -> String {
    format!("(int16_t){}", unsigned_reloc(ctx))
}

fn operand_string(operand: Operand, operation: UnaryOpType, ctx: &InstructionContext) -> String {
    let s = match operand {
        Operand::Rd => gpr_to_string(ctx.rd),
        Operand::Rs => gpr_to_string(ctx.rs),
        Operand::Rt => gpr_to_string(ctx.rt),
        Operand::Fd => fpr_to_string(ctx.fd),
        Operand::Fs => fpr_to_string(ctx.fs),
        Operand::Ft => fpr_to_string(ctx.ft),
        Operand::FdDouble => fpr_double_to_string(ctx.fd),
        Operand::FsDouble => fpr_double_to_string(ctx.fs),
        Operand::FtDouble => fpr_double_to_string(ctx.ft),
        Operand::FdU32L => fpr_u32l_to_string(ctx.fd),
        Operand::FsU32L => fpr_u32l_to_string(ctx.fs),
        Operand::FtU32L => fpr_u32l_to_string(ctx.ft),
        // Unused by any instruction; the C++ asserts (and leaves the string unchanged).
        Operand::FdU32H | Operand::FsU32H | Operand::FtU32H => {
            unreachable!("U32H operands are unused")
        }
        Operand::FdU64 => fpr_u64_to_string(ctx.fd),
        Operand::FsU64 => fpr_u64_to_string(ctx.fs),
        Operand::FtU64 => fpr_u64_to_string(ctx.ft),
        Operand::ImmU16 => {
            if ctx.reloc_type != RelocType::R_MIPS_NONE {
                unsigned_reloc(ctx)
            } else {
                hex_alt(ctx.imm16).to_string()
            }
        }
        Operand::ImmS16 => {
            if ctx.reloc_type != RelocType::R_MIPS_NONE {
                signed_reloc(ctx)
            } else {
                hex_alt(ctx.imm16 as i16).to_string()
            }
        }
        Operand::Sa => ctx.sa.to_string(),
        Operand::Sa32 => format!("({} + 32)", ctx.sa),
        Operand::Cop1cs => "c1cs".to_owned(),
        Operand::Hi => "hi".to_owned(),
        Operand::Lo => "lo".to_owned(),
        Operand::Zero => "0".to_owned(),
    };
    use UnaryOpType as U;
    match operation {
        U::None | U::ToU64 => s,
        U::ToS32 => format!("S32({s})"),
        U::ToU32 => format!("U32({s})"),
        U::ToS64 => format!("SIGNED({s})"),
        U::Lui => format!("S32({s} << 16)"),
        U::Mask5 => format!("({s} & 31)"),
        U::Mask6 => format!("({s} & 63)"),
        U::ToInt32 => format!("(int32_t){s}"),
        U::NegateFloat | U::NegateDouble => format!("-{s}"),
        U::AbsFloat => format!("fabsf({s})"),
        U::AbsDouble => format!("fabs({s})"),
        U::SqrtFloat => format!("sqrtf({s})"),
        U::SqrtDouble => format!("sqrt({s})"),
        U::ConvertSFromW => format!("CVT_S_W({s})"),
        U::ConvertWFromS => format!("CVT_W_S({s})"),
        U::ConvertDFromW => format!("CVT_D_W({s})"),
        U::ConvertWFromD => format!("CVT_W_D({s})"),
        U::ConvertDFromS => format!("CVT_D_S({s})"),
        U::ConvertSFromD => format!("CVT_S_D({s})"),
        U::ConvertDFromL => format!("CVT_D_L({s})"),
        U::ConvertLFromD => format!("CVT_L_D({s})"),
        U::ConvertSFromL => format!("CVT_S_L({s})"),
        U::ConvertLFromS => format!("CVT_L_S({s})"),
        U::TruncateWFromS => format!("TRUNC_W_S({s})"),
        U::TruncateWFromD => format!("TRUNC_W_D({s})"),
        U::TruncateLFromS => format!("TRUNC_L_S({s})"),
        U::TruncateLFromD => format!("TRUNC_L_D({s})"),
        // TODO (from the C++): these should use banker's rounding.
        U::RoundWFromS => format!("lroundf({s})"),
        U::RoundWFromD => format!("lround({s})"),
        U::RoundLFromS => format!("llroundf({s})"),
        U::RoundLFromD => format!("llround({s})"),
        U::CeilWFromS => format!("S32(ceilf({s}))"),
        U::CeilWFromD => format!("S32(ceil({s}))"),
        U::CeilLFromS => format!("S64(ceilf({s}))"),
        U::CeilLFromD => format!("S64(ceil({s}))"),
        U::FloorWFromS => format!("S32(floorf({s}))"),
        U::FloorWFromD => format!("S32(floor({s}))"),
        U::FloorLFromS => format!("S64(floorf({s}))"),
        U::FloorLFromD => format!("S64(floor({s}))"),
    }
}

fn binary_expr_string(
    kind: BinaryOpType,
    operands: &BinaryOperands,
    ctx: &InstructionContext,
    output: &str,
) -> String {
    let input_a = operand_string(operands.operands[0], operands.operand_operations[0], ctx);
    let input_b = operand_string(operands.operands[1], operands.operand_operations[1], ctx);
    let (func_string, infix_string) = notation(kind);
    let second_is_plain_zero = operands.operands[1] == Operand::Zero
        && operands.operand_operations[1] == UnaryOpType::None;

    use BinaryOpType as B;
    // These cases are only here for parity with the old recompiler output.
    if kind == B::Less
        && !(second_is_plain_zero
            || matches!(operands.operands[0], Operand::Fs | Operand::FsDouble))
    {
        format!("{input_a} {infix_string} {input_b} ? 1 : 0")
    } else if kind == B::Equal && second_is_plain_zero {
        format!("!{input_a}")
    } else if kind == B::NotEqual && second_is_plain_zero {
        input_a
    }
    // End of the parity cases.
    else if matches!(kind, B::Lwl | B::Lwr | B::Ldl | B::Ldr) {
        format!("{func_string}(rdram, {output}, {input_a}, {input_b})")
    } else if !func_string.is_empty() && !infix_string.is_empty() {
        format!("{func_string}({input_a} {infix_string} {input_b})")
    } else if !func_string.is_empty() {
        format!("{func_string}({input_a}, {input_b})")
    } else if !infix_string.is_empty() {
        format!("{input_a} {infix_string} {input_b}")
    } else if kind == B::True {
        "1".to_owned()
    } else if kind == B::False {
        "0".to_owned()
    } else {
        unreachable!("binary operation must have either a function or infix")
    }
}

/// Writes C, like N64Recomp's `CGenerator`.
#[derive(Default)]
pub struct CGenerator {
    pub out: String,
}

impl CGenerator {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Generator for CGenerator {
    fn raw(&mut self, text: &str) {
        self.out.push_str(text);
    }

    fn process_binary_op(&mut self, op: &BinaryOp, ctx: &InstructionContext) {
        let output = operand_string(op.output, UnaryOpType::None, ctx);
        let expression = binary_expr_string(op.kind, &op.operands, ctx, &output);
        out!(self.out, "{output} = {expression};\n");
    }

    fn process_unary_op(&mut self, op: &UnaryOp, ctx: &InstructionContext) {
        let output = operand_string(op.output, UnaryOpType::None, ctx);
        let input = operand_string(op.input, op.operation, ctx);
        out!(self.out, "{output} = {input};\n");
    }

    fn process_store_op(&mut self, op: &StoreOp, ctx: &InstructionContext) {
        let base = operand_string(Operand::BASE, UnaryOpType::None, ctx);
        let imm = operand_string(Operand::ImmS16, UnaryOpType::None, ctx);
        let value = operand_string(op.value_input, UnaryOpType::None, ctx);

        enum Syntax {
            Func,
            FuncWithRdram,
            Assignment,
        }
        let (func, syntax) = match op.kind {
            StoreOpType::Sd | StoreOpType::Sdc1 => ("SD", Syntax::Func),
            StoreOpType::Sdl => ("do_sdl", Syntax::FuncWithRdram),
            StoreOpType::Sdr => ("do_sdr", Syntax::FuncWithRdram),
            StoreOpType::Sw | StoreOpType::Swc1 => ("MEM_W", Syntax::Assignment),
            StoreOpType::Swl => ("do_swl", Syntax::FuncWithRdram),
            StoreOpType::Swr => ("do_swr", Syntax::FuncWithRdram),
            StoreOpType::Sh => ("MEM_H", Syntax::Assignment),
            StoreOpType::Sb => ("MEM_B", Syntax::Assignment),
        };
        match syntax {
            Syntax::Func => out!(self.out, "{func}({value}, {imm}, {base});\n"),
            Syntax::FuncWithRdram => out!(self.out, "{func}(rdram, {imm}, {base}, {value});\n"),
            Syntax::Assignment => out!(self.out, "{func}({imm}, {base}) = {value};\n"),
        }
    }

    fn emit_function_start(&mut self, function_name: &str, _func_index: usize) {
        out!(
            self.out,
            "RECOMP_FUNC void {function_name}(uint8_t* rdram, recomp_context* ctx) {{\n\
             \x20   uint64_t hi = 0, lo = 0, result = 0;\n\
             \x20   int c1cs = 0;\n"
        );
    }

    fn emit_function_end(&mut self) {
        self.out.push_str(";}\n");
    }

    fn emit_function_call_lookup(&mut self, addr: u32) {
        out!(self.out, "LOOKUP_FUNC(0x{addr:08X})(rdram, ctx);\n");
    }

    fn emit_function_call_by_register(&mut self, reg: u32) {
        out!(
            self.out,
            "LOOKUP_FUNC({})(rdram, ctx);\n",
            gpr_to_string(reg)
        );
    }

    fn emit_function_call_reference_symbol(
        &mut self,
        context: &Context,
        section_index: u16,
        symbol_index: usize,
        _target_section_offset: u32,
    ) {
        let sym = context.get_reference_symbol(section_index, symbol_index);
        out!(self.out, "{}(rdram, ctx);\n", sym.name);
    }

    fn emit_function_call(&mut self, context: &Context, function_index: usize) {
        out!(
            self.out,
            "{}(rdram, ctx);\n",
            context.functions[function_index].name
        );
    }

    fn emit_named_function_call(&mut self, function_name: &str) {
        out!(self.out, "{function_name}(rdram, ctx);\n");
    }

    fn emit_goto(&mut self, target: &str) {
        out!(self.out, "    goto {target};\n");
    }

    fn emit_label(&mut self, label_name: &str) {
        out!(self.out, "{label_name}:\n");
    }

    fn emit_jtbl_addend_declaration(&mut self, jtbl: &JumpTable, reg: u32) {
        out!(
            self.out,
            "gpr jr_addend_{:08X} = {};\n",
            jtbl.jr_vram,
            gpr_to_string(reg)
        );
    }

    fn emit_branch_condition(&mut self, op: &ConditionalBranchOp, ctx: &InstructionContext) {
        let expr = binary_expr_string(op.comparison, &op.operands, ctx, "");
        out!(self.out, "if ({expr}) {{\n");
    }

    fn emit_branch_close(&mut self) {
        self.out.push_str("}\n");
    }

    fn emit_switch(&mut self, _context: &Context, jtbl: &JumpTable, _reg: u32) {
        // TODO (from the C++): subtract the jump table address from the register instead.
        out!(
            self.out,
            "switch (jr_addend_{:08X} >> 2) {{\n",
            jtbl.jr_vram
        );
    }

    fn emit_case(&mut self, case_index: usize, target_label: &str) {
        out!(self.out, "case {case_index}: goto {target_label}; break;\n");
    }

    fn emit_switch_error(&mut self, instr_vram: u32, jtbl_vram: u32) {
        out!(
            self.out,
            "default: switch_error(__func__, 0x{instr_vram:08X}, 0x{jtbl_vram:08X});\n"
        );
    }

    fn emit_switch_close(&mut self) {
        self.out.push_str("}\n");
    }

    fn emit_return(&mut self, context: &Context, _func_index: usize) {
        if context.trace_mode {
            self.out.push_str("TRACE_RETURN()\n    ");
        }
        self.out.push_str("return;\n");
    }

    fn emit_check_fr(&mut self, fpr: u32) {
        out!(self.out, "CHECK_FR(ctx, {fpr});\n    ");
    }

    fn emit_check_nan(&mut self, fpr: u32, is_double: bool) {
        out!(
            self.out,
            "NAN_CHECK(ctx->f{fpr}.{}); ",
            if is_double { "d" } else { "fl" }
        );
    }

    fn emit_cop0_status_read(&mut self, reg: u32) {
        out!(
            self.out,
            "{} = cop0_status_read(ctx);\n",
            gpr_to_string(reg)
        );
    }

    fn emit_cop0_status_write(&mut self, reg: u32) {
        out!(self.out, "cop0_status_write(ctx, {});", gpr_to_string(reg));
    }

    fn emit_cop1_cs_read(&mut self, reg: u32) {
        out!(self.out, "{} = get_cop1_cs();\n", gpr_to_string(reg));
    }

    fn emit_cop1_cs_write(&mut self, reg: u32) {
        out!(self.out, "set_cop1_cs({});\n", gpr_to_string(reg));
    }

    fn emit_muldiv(&mut self, instr_id: InstrId, reg1: u32, reg2: u32) {
        let (a, b) = (gpr_to_string(reg1), gpr_to_string(reg2));
        match instr_id {
            InstrId::cpu_mult => out!(
                self.out,
                "result = S64(S32({a})) * S64(S32({b})); lo = S32(result >> 0); hi = S32(result >> 32);\n"
            ),
            InstrId::cpu_dmult => out!(self.out, "DMULT(S64({a}), S64({b}), &lo, &hi);\n"),
            InstrId::cpu_multu => out!(
                self.out,
                "result = U64(U32({a})) * U64(U32({b})); lo = S32(result >> 0); hi = S32(result >> 32);\n"
            ),
            InstrId::cpu_dmultu => out!(self.out, "DMULTU(U64({a}), U64({b}), &lo, &hi);\n"),
            // Cast to 64 bits before dividing to avoid an arithmetic exception for
            // s32(0x80000000) / -1.
            InstrId::cpu_div => out!(
                self.out,
                "lo = S32(S64(S32({a})) / S64(S32({b}))); hi = S32(S64(S32({a})) % S64(S32({b})));\n"
            ),
            InstrId::cpu_ddiv => out!(self.out, "DDIV(S64({a}), S64({b}), &lo, &hi);\n"),
            InstrId::cpu_divu => out!(
                self.out,
                "lo = S32(U32({a}) / U32({b})); hi = S32(U32({a}) % U32({b}));\n"
            ),
            InstrId::cpu_ddivu => out!(self.out, "DDIVU(U64({a}), U64({b}), &lo, &hi);\n"),
            _ => unreachable!("not a mult/div instruction"),
        }
    }

    fn emit_syscall(&mut self, instr_vram: u32) {
        out!(
            self.out,
            "recomp_syscall_handler(rdram, ctx, 0x{instr_vram:08X});\n"
        );
    }

    fn emit_do_break(&mut self, instr_vram: u32) {
        out!(self.out, "do_break({instr_vram});\n");
    }

    fn emit_pause_self(&mut self) {
        self.out.push_str("pause_self(rdram);\n");
    }

    fn emit_trigger_event(&mut self, event_index: u32) {
        out!(
            self.out,
            "recomp_trigger_event(rdram, ctx, base_event_index + {event_index});\n"
        );
    }

    fn emit_comment(&mut self, comment: &str) {
        out!(self.out, "// {comment}\n");
    }
}
