//! RSPRecomp: statically recompiles RSP microcode into a C++ function.
//!
//! A port of `RSPRecomp/src/rsp_recomp.cpp` that writes byte-identical output. The structure
//! follows the C++ closely so the two can be compared side by side; `rust/golden/golden.sh`
//! checks the output.

#![deny(unsafe_code)]

mod config;

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use rabbitizer::{InstrCategory, InstrId, Instruction, OperandType};
use recomp_core::cxx_fmt::hex_alt;
use recomp_core::cxx_hash::CxxHashSet;

use config::Config;

const INSTR_SIZE: u32 = 4;
const RSP_MEM_MASK: u32 = 0x1FFF;

/// Appends formatted text to a `String` (which can't fail).
macro_rules! out {
    ($out:expr, $($arg:tt)*) => {{
        let _ = write!($out, $($arg)*);
    }};
}

// Instruction fields, decoded like rabbitizer's `RAB_INSTR_*_GET_*` macros. The C++ tool is
// built without `RAB_SANITY_CHECKS`, so its getters never check that an operand exists.

fn bits(instr: &Instruction, shift: u32, width: u32) -> u32 {
    (instr.raw() >> shift) & ((1 << width) - 1)
}

fn rs(instr: &Instruction) -> u32 {
    bits(instr, 21, 5)
}
fn rt(instr: &Instruction) -> u32 {
    bits(instr, 16, 5)
}
fn rd(instr: &Instruction) -> u32 {
    bits(instr, 11, 5)
}
fn sa(instr: &Instruction) -> u32 {
    bits(instr, 6, 5)
}
fn immediate(instr: &Instruction) -> u16 {
    bits(instr, 0, 16) as u16
}

/// Operands of instructions implemented by `rsp.<NAME>(...)` calls in `rsp_vu_impl.hpp`.
/// RSPRecomp can't use rabbitizer's operand types because it needs either a register
/// reference or a register index.
#[derive(Clone, Copy)]
enum RspOperand {
    None,
    Vt,
    VtIndex,
    Vd,
    Vs,
    VsIndex,
    De,
    Rt,
    Rs,
    Imm7,
}

fn vector_operands(id: InstrId) -> Option<[RspOperand; 3]> {
    use InstrId::*;
    use RspOperand::{De, Imm7, Rs, Rt, Vd, Vs, VsIndex, Vt, VtIndex};
    let none = RspOperand::None;
    Some(match id {
        // Vt, Rs, Imm (lwv isn't in rabbitizer)
        rsp_lbv | rsp_ldv | rsp_lfv | rsp_lhv | rsp_llv | rsp_lpv | rsp_lqv | rsp_lrv | rsp_lsv
        | rsp_luv | rsp_sbv | rsp_sdv | rsp_sfv | rsp_shv | rsp_slv | rsp_spv | rsp_sqv
        | rsp_srv | rsp_ssv | rsp_suv | rsp_swv => [Vt, Rs, Imm7],
        rsp_stv | rsp_ltv => [VtIndex, Rs, Imm7],

        // Vd, Vs, Vt
        rsp_vabs | rsp_vadd | rsp_vaddc | rsp_vand | rsp_vch | rsp_vcl | rsp_vcr | rsp_veq
        | rsp_vge | rsp_vlt | rsp_vmacf | rsp_vmacu | rsp_vmadh | rsp_vmadl | rsp_vmadm
        | rsp_vmadn | rsp_vmrg | rsp_vmudh | rsp_vmudl | rsp_vmudm | rsp_vmudn | rsp_vne
        | rsp_vnor | rsp_vnxor | rsp_vor | rsp_vsub | rsp_vsubc | rsp_vmulf | rsp_vmulu
        | rsp_vmulq | rsp_vnand | rsp_vxor => [Vd, Vs, Vt],
        rsp_vsar => [Vd, Vs, none],
        rsp_vmacq => [Vd, none, none],
        rsp_vrndn | rsp_vrndp => [Vd, VsIndex, Vt],

        // Vd, De, Vt
        rsp_vmov | rsp_vrcp | rsp_vrcpl | rsp_vrcph | rsp_vrsq | rsp_vrsql | rsp_vrsqh => {
            [Vd, De, Vt]
        }

        // Rt, Vs
        rsp_mfc2 | rsp_mtc2 => [Rt, Vs, none],

        rsp_vnop => [none, none, none],
        _ => return None,
    })
}

/// A GPR as the generated code names it: `r<n>`, or `0` for `$zero`.
fn gpr(reg: u32) -> String {
    if reg == 0 {
        "0".to_owned()
    } else {
        format!("r{reg}")
    }
}

// RSP COP0 registers.
const SP_MEM_ADDR: u32 = 0;
const SP_DRAM_ADDR: u32 = 1;
const SP_RD_LEN: u32 = 2;
const SP_WR_LEN: u32 = 3;
const SP_STATUS: u32 = 4;
const SP_DMA_FULL: u32 = 5;
const SP_DMA_BUSY: u32 = 6;
const SP_SEMAPHORE: u32 = 7;
const DPC_STATUS: u32 = 11;

#[derive(Debug)]
struct Unhandled(String);

fn expected_c0_reg_value(cop0_reg: u32) -> Result<u32, Unhandled> {
    match cop0_reg {
        SP_STATUS => Ok(0),    // None of the flags in RSP status are set
        SP_DMA_FULL => Ok(0),  // Pretend DMAs complete instantly
        SP_DMA_BUSY => Ok(0),  // Pretend DMAs complete instantly
        SP_SEMAPHORE => Ok(0), // Always acquire the semaphore
        DPC_STATUS => Ok(0), // Good enough for the microcodes that would be recompiled (i.e. non-graphics ones)
        _ => Err(Unhandled(format!("Unhandled mfc0: {cop0_reg}"))),
    }
}

fn c0_reg_write_action(cop0_reg: u32) -> Result<&'static str, Unhandled> {
    match cop0_reg {
        SP_SEMAPHORE => Ok(""), // Ignore semaphore functionality
        SP_STATUS => Ok(""),    // Ignore writes to the status flags since yielding is ignored
        SP_DRAM_ADDR => Ok("SET_DMA_DRAM"),
        SP_MEM_ADDR => Ok("SET_DMA_MEM"),
        SP_RD_LEN => Ok("DO_DMA_READ"),
        SP_WR_LEN => Ok("DO_DMA_WRITE"),
        _ => Err(Unhandled(format!("Unhandled mtc0: {cop0_reg}"))),
    }
}

fn is_c0_reg_write_dma_read(cop0_reg: u32) -> bool {
    cop0_reg == SP_RD_LEN
}

fn rsp_element(instr: &Instruction) -> Option<u32> {
    if instr.has_operand(OperandType::rsp_vt_elementhigh) {
        Some(bits(instr, 21, 4))
    } else if instr.has_operand(OperandType::rsp_vt_elementlow)
        || instr.has_operand(OperandType::rsp_vs_index)
    {
        Some(bits(instr, 7, 4))
    } else {
        None
    }
}

fn rsp_ignores_element(id: InstrId) -> bool {
    matches!(id, InstrId::rsp_vmacq | InstrId::rsp_vnop)
}

fn branch_target(instr: &Instruction) -> u32 {
    instr.branch_vram_generic() as u32 & RSP_MEM_MASK
}

#[derive(Default)]
struct BranchTargets {
    direct_targets: CxxHashSet<u32>,
    indirect_targets: CxxHashSet<u32>,
}

fn get_branch_targets(instrs: &[Instruction]) -> BranchTargets {
    let mut ret = BranchTargets::default();
    for instr in instrs {
        if instr.is_jump_with_address() || instr.is_branch() {
            ret.direct_targets.insert(branch_target(instr));
        }
        if instr.does_link() {
            ret.indirect_targets.insert(instr.vram + 2 * INSTR_SIZE);
        }
    }
    ret
}

#[derive(Default)]
struct ResumeTargets {
    non_delay_targets: CxxHashSet<u32>,
    delay_targets: CxxHashSet<u32>,
}

fn get_overlay_swap_resume_targets(instrs: &[Instruction], targets: &mut ResumeTargets) {
    let mut is_delay_slot = false;
    for instr in instrs {
        if instr.unique_id == InstrId::rsp_mtc0 && is_c0_reg_write_dma_read(rd(instr)) {
            targets.non_delay_targets.insert(instr.vram);
            if is_delay_slot {
                targets.delay_targets.insert(instr.vram);
            }
        }
        is_delay_slot = instr.has_delay_slot();
    }
}

/// What `process_instruction` needs besides the instruction itself.
struct FunctionContext<'a> {
    instructions: &'a [Instruction],
    branch_targets: &'a BranchTargets,
    unsupported_instructions: &'a CxxHashSet<u32>,
    resume_targets: &'a ResumeTargets,
    has_overlays: bool,
}

const INDENT: &str = "    ";

impl FunctionContext<'_> {
    /// Writes one instruction. Returns `Ok(false)` for an unhandled instruction, which the
    /// C++ reports and skips.
    fn process_instruction(
        &self,
        out: &mut String,
        instr_index: usize,
        indent: bool,
        in_delay_slot: bool,
    ) -> Result<bool, Unhandled> {
        let instr = &self.instructions[instr_index];
        let instr_vram = instr.vram;
        let instr_id = instr.unique_id;

        // Skip labels if we're duplicating an instruction into a delay slot
        if !in_delay_slot
            && (self.branch_targets.direct_targets.contains(instr_vram)
                || self.branch_targets.indirect_targets.contains(instr_vram))
        {
            out!(out, "L_{instr_vram:04X}:\n");
        }

        let branch_target = branch_target(instr);

        // Output a comment with the original instruction
        if instr.is_branch() || instr_id == InstrId::rsp_j {
            let label = format!("L_{branch_target:04X}");
            out!(out, "    // {}\n", instr.disassemble(Some(&label), 0));
        } else if instr_id == InstrId::rsp_jal {
            let label = format!("0x{branch_target:04X}");
            out!(out, "    // {}\n", instr.disassemble(Some(&label), 0));
        } else {
            out!(out, "    // {}\n", instr.disassemble(None, 0));
        }

        // `print_line`: an indented statement.
        let line = |out: &mut String, text: String| out!(out, "{INDENT}{text};\n");

        let has_next = instr_index < self.instructions.len() - 1;

        let unconditional_branch = |out: &mut String, text: String| -> Result<(), Unhandled> {
            if has_next {
                self.process_instruction(out, instr_index + 1, false, true)?;
            }
            out!(out, "{INDENT}{text};\n");
            Ok(())
        };

        let branch = |out: &mut String, condition: String, text: String| -> Result<(), Unhandled> {
            out!(out, "{INDENT}{condition} {{\n    ");
            if has_next {
                self.process_instruction(out, instr_index + 1, true, true)?;
            }
            out!(out, "        {text};\n    }}\n");
            Ok(())
        };

        if indent {
            out.push_str(INDENT);
        }

        // Replace unsupported instructions with early returns
        if self.unsupported_instructions.contains(instr_vram) {
            line(out, "return RspExitReason::Unsupported".into());
            if indent {
                out.push_str(INDENT);
            }
        }

        let (rd, rs, rt, sa) = (rd(instr), rs(instr), rt(instr), sa(instr));
        let base = rs;
        let imm = immediate(instr);
        let unsigned_imm = hex_alt(imm);
        let signed_imm = hex_alt(imm as i16);
        let (rd_, rs_, rt_, base_) = (gpr(rd), gpr(rs), gpr(rt), gpr(base));

        // If this instruction is in the vector operand table then emit the appropriate
        // function call for its implementation
        if let Some(operands) = vector_operands(instr_id) {
            let (vd, vs, vt) = (bits(instr, 6, 5), bits(instr, 11, 5), bits(instr, 16, 5));
            let operand_string = operands
                .iter()
                .filter_map(|operand| match operand {
                    RspOperand::Vt => Some(format!("rsp.vpu.r[{vt}]")),
                    RspOperand::VtIndex => Some(vt.to_string()),
                    RspOperand::Vd => Some(format!("rsp.vpu.r[{vd}]")),
                    RspOperand::Vs => Some(format!("rsp.vpu.r[{vs}]")),
                    RspOperand::VsIndex => Some(vs.to_string()),
                    RspOperand::De => Some((bits(instr, 11, 5) & 7).to_string()),
                    RspOperand::Rt => Some(rt_.clone()),
                    RspOperand::Rs => Some(rs_.clone()),
                    // Sign extend the 7-bit immediate
                    RspOperand::Imm7 => Some(hex_alt(((imm << 1) as u8 as i8) >> 1).to_string()),
                    RspOperand::None => None,
                })
                .collect::<Vec<_>>()
                .join(", ");
            let name = instr.opcode_name().to_uppercase();
            if rsp_ignores_element(instr_id) {
                line(out, format!("rsp.{name}({operand_string})"));
            } else {
                let element = rsp_element(instr).expect("vector instruction without an element");
                line(out, format!("rsp.{name}<{element}>({operand_string})"));
            }
        } else {
            // Otherwise, implement the instruction directly
            use InstrId::*;
            match instr_id {
                rsp_nop => out.push('\n'),
                // Arithmetic
                rsp_lui => line(out, format!("{rt_} = S32({unsigned_imm} << 16)")),
                rsp_add | rsp_addu => {
                    if rd == 0 {
                        out.push('\n');
                    } else {
                        line(out, format!("{rd_} = RSP_ADD32({rs_}, {rt_})"));
                    }
                }
                // negu is a pseudo instruction for subu x, 0, y
                rsp_negu | rsp_sub | rsp_subu => {
                    line(out, format!("{rd_} = RSP_SUB32({rs_}, {rt_})"))
                }
                rsp_addi | rsp_addiu => {
                    line(out, format!("{rt_} = RSP_ADD32({rs_}, {signed_imm})"))
                }
                rsp_and => {
                    if rd == 0 {
                        out.push('\n');
                    } else {
                        line(out, format!("{rd_} = {rs_} & {rt_}"));
                    }
                }
                rsp_andi => line(out, format!("{rt_} = {rs_} & {unsigned_imm}")),
                rsp_or => line(out, format!("{rd_} = {rs_} | {rt_}")),
                rsp_ori => line(out, format!("{rt_} = {rs_} | {unsigned_imm}")),
                rsp_nor => line(out, format!("{rd_} = ~({rs_} | {rt_})")),
                rsp_xor => line(out, format!("{rd_} = {rs_} ^ {rt_}")),
                rsp_xori => line(out, format!("{rt_} = {rs_} ^ {unsigned_imm}")),
                rsp_sll => line(out, format!("{rd_} = S32({rt_}) << {sa}")),
                rsp_sllv => line(out, format!("{rd_} = S32({rt_}) << ({rs_} & 31)")),
                rsp_sra => line(out, format!("{rd_} = S32(RSP_SIGNED({rt_}) >> {sa})")),
                rsp_srav => line(
                    out,
                    format!("{rd_} = S32(RSP_SIGNED({rt_}) >> ({rs_} & 31))"),
                ),
                rsp_srl => line(out, format!("{rd_} = S32(U32({rt_}) >> {sa})")),
                rsp_srlv => line(out, format!("{rd_} = S32(U32({rt_}) >> ({rs_} & 31))")),
                rsp_slt => line(
                    out,
                    format!("{rd_} = RSP_SIGNED({rs_}) < RSP_SIGNED({rt_}) ? 1 : 0"),
                ),
                rsp_slti => line(
                    out,
                    format!("{rt_} = RSP_SIGNED({rs_}) < {signed_imm} ? 1 : 0"),
                ),
                rsp_sltu => line(out, format!("{rd_} = {rs_} < {rt_} ? 1 : 0")),
                rsp_sltiu => line(out, format!("{rt_} = {rs_} < {signed_imm} ? 1 : 0")),
                // Loads
                // TODO ld
                rsp_lw => line(
                    out,
                    format!("{rt_} = RSP_MEM_W_LOAD({signed_imm}, {base_})"),
                ),
                rsp_lh => line(
                    out,
                    format!("{rt_} = RSP_MEM_H_LOAD({signed_imm}, {base_})"),
                ),
                rsp_lb => line(out, format!("{rt_} = RSP_MEM_B({signed_imm}, {base_})")),
                rsp_lhu => line(
                    out,
                    format!("{rt_} = RSP_MEM_HU_LOAD({signed_imm}, {base_})"),
                ),
                rsp_lbu => line(out, format!("{rt_} = RSP_MEM_BU({signed_imm}, {base_})")),
                // Stores
                rsp_sw => line(
                    out,
                    format!("RSP_MEM_W_STORE({signed_imm}, {base_}, {rt_})"),
                ),
                rsp_sh => line(
                    out,
                    format!("RSP_MEM_H_STORE({signed_imm}, {base_}, {rt_})"),
                ),
                rsp_sb => line(out, format!("RSP_MEM_B({signed_imm}, {base_}) = {rt_}")),
                // Branches
                rsp_j | rsp_b => unconditional_branch(out, format!("goto L_{branch_target:04X}"))?,
                rsp_jal => {
                    line(
                        out,
                        format!("{} = 0x{:04X}", gpr(31), instr_vram + 2 * INSTR_SIZE),
                    );
                    unconditional_branch(out, format!("goto L_{branch_target:04X}"))?;
                }
                rsp_jr => {
                    line(out, format!("jump_target = {rs_}"));
                    line(out, "debug_file = __FILE__; debug_line = __LINE__".into());
                    unconditional_branch(out, "goto do_indirect_jump".into())?;
                }
                rsp_jalr => {
                    line(
                        out,
                        format!(
                            "jump_target = {rs_}; {rd_} = 0x{:8X}",
                            instr_vram + 2 * INSTR_SIZE
                        ),
                    );
                    line(out, "debug_file = __FILE__; debug_line = __LINE__".into());
                    unconditional_branch(out, "goto do_indirect_jump".into())?;
                }
                rsp_bne => branch(
                    out,
                    format!("if ({rs_} != {rt_})"),
                    format!("goto L_{branch_target:04X}"),
                )?,
                rsp_beq => branch(
                    out,
                    format!("if ({rs_} == {rt_})"),
                    format!("goto L_{branch_target:04X}"),
                )?,
                rsp_bgez => branch(
                    out,
                    format!("if (RSP_SIGNED({rs_}) >= 0)"),
                    format!("goto L_{branch_target:04X}"),
                )?,
                rsp_bgtz => branch(
                    out,
                    format!("if (RSP_SIGNED({rs_}) > 0)"),
                    format!("goto L_{branch_target:04X}"),
                )?,
                rsp_blez => branch(
                    out,
                    format!("if (RSP_SIGNED({rs_}) <= 0)"),
                    format!("goto L_{branch_target:04X}"),
                )?,
                rsp_bltz => branch(
                    out,
                    format!("if (RSP_SIGNED({rs_}) < 0)"),
                    format!("goto L_{branch_target:04X}"),
                )?,
                rsp_break => line(out, "return RspExitReason::Broke".into()),
                rsp_mfc0 => line(out, format!("{rt_} = {}", expected_c0_reg_value(rd)?)),
                rsp_mtc0 => {
                    let write_action = c0_reg_write_action(rd)?;
                    if self.has_overlays && is_c0_reg_write_dma_read(rd) {
                        // DMA read, do overlay swap if reading into IMEM
                        out!(
                            out,
                            "    if (dma_mem_address & 0x1000) {{\n\
                             \x20       ctx->resume_address = 0x{instr_vram:04X};\n\
                             \x20       ctx->resume_delay = {in_delay_slot};\n\
                             \x20       goto do_overlay_swap;\n\
                             \x20   }}\n"
                        );
                    }
                    if !write_action.is_empty() {
                        line(out, format!("{write_action}({rt_})"));
                    }
                }
                _ => {
                    eprintln!("Unhandled instruction: {}", instr.opcode_name());
                    return Ok(false);
                }
            }
        }

        // Write overlay swap resume labels
        if in_delay_slot {
            if self.resume_targets.delay_targets.contains(instr_vram) {
                out!(out, "R_{instr_vram:04X}_delay:\n");
            }
        } else if self.resume_targets.non_delay_targets.contains(instr_vram) {
            out!(out, "R_{instr_vram:04X}:\n");
        }

        Ok(true)
    }
}

fn write_indirect_jumps(
    out: &mut String,
    branch_targets: &BranchTargets,
    output_function_name: &str,
) {
    out!(
        out,
        "do_indirect_jump:\n    switch ((jump_target | 0x1000) & {}) {{ \n",
        hex_alt(RSP_MEM_MASK)
    );
    for target in branch_targets.indirect_targets.iter() {
        out!(out, "        case 0x{target:04X}: goto L_{target:04X};\n");
    }
    out!(
        out,
        concat!(
            "    }}\n",
            "    printf(\"Unhandled jump target 0x%04X in microcode {name}, coming from [%s:%d]\\n\", jump_target, debug_file, debug_line);\n",
            "    printf(\"Register dump: r0  = %08X r1  = %08X r2  = %08X r3  = %08X r4  = %08X r5  = %08X r6  = %08X r7  = %08X\\n\"\n",
            "           \"               r8  = %08X r9  = %08X r10 = %08X r11 = %08X r12 = %08X r13 = %08X r14 = %08X r15 = %08X\\n\"\n",
            "           \"               r16 = %08X r17 = %08X r18 = %08X r19 = %08X r20 = %08X r21 = %08X r22 = %08X r23 = %08X\\n\"\n",
            "           \"               r24 = %08X r25 = %08X r26 = %08X r27 = %08X r28 = %08X r29 = %08X r30 = %08X r31 = %08X\\n\",\n",
            "           0, r1, r2, r3, r4, r5, r6, r7, r8, r9, r10, r11, r12, r13, r14, r15, r16,\n",
            "           r17, r18, r19, r20, r21, r22, r23, r24, r25, r26, r27, r28, r29, r30, r31);\n",
            "    return RspExitReason::UnhandledJumpTarget;\n",
        ),
        name = output_function_name
    );
}

fn write_overlay_swap_return(out: &mut String) {
    out.push_str(concat!(
        "do_overlay_swap:\n",
        "                    ctx->r1 = r1;   ctx->r2 = r2;   ctx->r3 = r3;   ctx->r4 = r4;   ctx->r5 = r5;   ctx->r6 = r6;   ctx->r7 = r7;\n",
        "    ctx->r8 = r8;   ctx->r9 = r9;   ctx->r10 = r10; ctx->r11 = r11; ctx->r12 = r12; ctx->r13 = r13; ctx->r14 = r14; ctx->r15 = r15;\n",
        "    ctx->r16 = r16; ctx->r17 = r17; ctx->r18 = r18; ctx->r19 = r19; ctx->r20 = r20; ctx->r21 = r21; ctx->r22 = r22; ctx->r23 = r23;\n",
        "    ctx->r24 = r24; ctx->r25 = r25; ctx->r26 = r26; ctx->r27 = r27; ctx->r28 = r28; ctx->r29 = r29; ctx->r30 = r30; ctx->r31 = r31;\n",
        "    ctx->dma_mem_address = dma_mem_address;\n",
        "    ctx->dma_dram_address = dma_dram_address;\n",
        "    ctx->jump_target = jump_target;\n",
        "    ctx->rsp = rsp;\n",
        "    return RspExitReason::SwapOverlay;\n",
    ));
}

struct FunctionPermutation {
    instrs: Vec<Instruction>,
    permutation: Vec<u32>,
}

struct Permutation {
    instr_words: Vec<u32>,
    permutation: Vec<u32>,
}

struct OverlaySlot {
    offset: u32,
    overlays: Vec<Vec<u32>>,
}

/// Advances `current` like an odometer whose digits have `option_lengths` values each.
/// Returns `false` once every combination has been produced.
fn next_permutation(option_lengths: &[u32], current: &mut [u32]) -> bool {
    let mut i = current.len() - 1;
    current[i] += 1;
    while current[i] == option_lengths[i] {
        current[i] = 0;
        if i == 0 {
            return false;
        }
        current[i - 1] += 1;
        i -= 1;
    }
    true
}

fn permute(base_words: &[u32], overlay_slots: &[OverlaySlot]) -> Vec<Permutation> {
    let mut permutations = Vec::new();
    let mut current = vec![0u32; overlay_slots.len()];
    let slot_options: Vec<u32> = overlay_slots
        .iter()
        .map(|slot| slot.overlays.len() as u32)
        .collect();

    loop {
        let mut instr_words = base_words.to_vec();
        for (slot, &choice) in overlay_slots.iter().zip(&current) {
            let word_offset = (slot.offset / INSTR_SIZE) as usize;
            // The C++ copies past the end of the vector when an overlay overruns the text
            // (after only reserving capacity); those words never reach the output.
            for (dst, &word) in instr_words
                .iter_mut()
                .skip(word_offset)
                .zip(&slot.overlays[choice as usize])
            {
                *dst = word;
            }
        }
        permutations.push(Permutation {
            instr_words,
            permutation: current.clone(),
        });
        if !next_permutation(&slot_options, &mut current) {
            return permutations;
        }
    }
}

fn make_permutation_string(permutation: &[u32]) -> String {
    permutation.iter().map(u32::to_string).collect()
}

fn create_overlay_swap_function(
    out: &mut String,
    permutations: &[FunctionPermutation],
    config: &Config,
) {
    let name = &config.output_function_name;

    // Includes and permutation protos
    out!(
        out,
        "#include <map>\n\
         #include <vector>\n\n\
         using RspUcodePermutationFunc = RspExitReason(uint8_t* rdram, RspContext* ctx);\n\n\
         RspExitReason {name}_initial(uint8_t* rdram, RspContext* ctx);\n"
    );
    for permutation in permutations {
        out!(
            out,
            "RspExitReason {name}{}(uint8_t* rdram, RspContext* ctx);\n",
            make_permutation_string(&permutation.permutation)
        );
    }
    out.push('\n');

    // IMEM -> slot index mapping
    out.push_str("static const std::map<uint32_t, uint32_t> imemToSlot = {\n");
    for (i, slot) in config.overlay_slots.iter().enumerate() {
        out!(
            out,
            "    {{ 0x{:04X}, {i} }},\n",
            slot.text_address & RSP_MEM_MASK
        );
    }
    out.push_str("};\n\n");

    // ucode offset -> overlay index mapping (per slot)
    out.push_str("static const std::vector<std::map<uint32_t, uint32_t>> offsetToOverlay = {\n");
    for slot in &config.overlay_slots {
        out.push_str("    {\n");
        for (i, overlay) in slot.overlays.iter().enumerate() {
            out!(out, "        {{ 0x{:04X}, {i} }},\n", overlay.offset);
        }
        out.push_str("    },\n");
    }
    out.push_str("};\n\n");

    // Permutation function pointers
    out.push_str("static RspUcodePermutationFunc* permutations[] = {\n");
    for permutation in permutations {
        out!(
            out,
            "    {name}{},\n",
            make_permutation_string(&permutation.permutation)
        );
    }
    out.push_str("};\n\n");

    // Main function
    out!(
        out,
        "RspExitReason {name}(uint8_t* rdram, uint32_t ucode_addr) {{\n    RspContext ctx{{}};\n"
    );

    let slots_init = vec!["0"; config.overlay_slots.len()].join(", ");
    out!(out, "    uint32_t slots[] = {{{slots_init}}};\n\n");
    out!(
        out,
        "    RspExitReason exitReason = {name}_initial(rdram, &ctx);\n\n"
    );

    let perm_index = (0..config.overlay_slots.len())
        .map(|i| {
            let multiplier: u32 = config.overlay_slots[i + 1..]
                .iter()
                .map(|slot| slot.overlays.len() as u32)
                .product();
            format!("slots[{i}] * {multiplier}")
        })
        .collect::<Vec<_>>()
        .join(" + ");

    out!(
        out,
        "    while (exitReason == RspExitReason::SwapOverlay) {{\n\
         \x20       uint32_t slot = imemToSlot.at(ctx.dma_mem_address);\n\
         \x20       uint32_t overlay = offsetToOverlay.at(slot).at(ctx.dma_dram_address - ucode_addr);\n\
         \x20       slots[slot] = overlay;\n\
         \n\
         \x20       RspUcodePermutationFunc* permutationFunc = permutations[{perm_index}];\n\
         \x20       exitReason = permutationFunc(rdram, &ctx);\n\
         \x20   }}\n\n\
         \x20   return exitReason;\n\
         }}\n\n"
    );
}

fn create_function(
    out: &mut String,
    function_name: &str,
    instrs: &[Instruction],
    config: &Config,
    resume_targets: &ResumeTargets,
    is_permutation: bool,
    is_initial: bool,
) -> Result<(), Unhandled> {
    // Collect indirect jump targets (return addresses for linked jumps)
    let mut branch_targets = get_branch_targets(instrs);

    // Add any additional indirect branch targets that may not be found directly in the code
    // (e.g. from a jump table)
    branch_targets
        .indirect_targets
        .extend(config.extra_indirect_branch_targets.iter().copied());

    // Write function
    if is_permutation {
        out!(
            out,
            concat!(
                "RspExitReason {}(uint8_t* rdram, RspContext* ctx) {{\n",
                "    uint32_t                 r1 = ctx->r1,   r2 = ctx->r2,   r3 = ctx->r3,   r4 = ctx->r4,   r5 = ctx->r5,   r6 = ctx->r6,   r7 = ctx->r7;\n",
                "    uint32_t  r8 = ctx->r8,  r9 = ctx->r9,   r10 = ctx->r10, r11 = ctx->r11, r12 = ctx->r12, r13 = ctx->r13, r14 = ctx->r14, r15 = ctx->r15;\n",
                "    uint32_t r16 = ctx->r16, r17 = ctx->r17, r18 = ctx->r18, r19 = ctx->r19, r20 = ctx->r20, r21 = ctx->r21, r22 = ctx->r22, r23 = ctx->r23;\n",
                "    uint32_t r24 = ctx->r24, r25 = ctx->r25, r26 = ctx->r26, r27 = ctx->r27, r28 = ctx->r28, r29 = ctx->r29, r30 = ctx->r30, r31 = ctx->r31;\n",
                "    uint32_t dma_mem_address = ctx->dma_mem_address, dma_dram_address = ctx->dma_dram_address, jump_target = ctx->jump_target;\n",
                "    const char * debug_file = NULL; int debug_line = 0;\n",
                "    RSP rsp = ctx->rsp;\n",
            ),
            function_name
        );

        // Write jumps to resume targets
        if !is_initial {
            out.push_str("    if (ctx->resume_delay) {\n        switch (ctx->resume_address) {\n");
            for address in resume_targets.delay_targets.iter() {
                out!(
                    out,
                    "            case 0x{address:04X}: goto R_{address:04X}_delay;\n"
                );
            }
            out.push_str("        }\n    } else {\n        switch (ctx->resume_address) {\n");
            for address in resume_targets.non_delay_targets.iter() {
                out!(
                    out,
                    "            case 0x{address:04X}: goto R_{address:04X};\n"
                );
            }
            out!(
                out,
                "        }}\n    }}\n    printf(\"Unhandled resume target 0x%04X (delay slot: %d) in microcode {}\\n\", ctx->resume_address, ctx->resume_delay);\n    return RspExitReason::UnhandledResumeTarget;\n",
                config.output_function_name
            );
        }

        out.push_str("    r1 = 0xFC0;\n");
    } else {
        out!(
            out,
            concat!(
                "RspExitReason {}(uint8_t* rdram, [[maybe_unused]] uint32_t ucode_addr) {{\n",
                "    uint32_t           r1 = 0,  r2 = 0,  r3 = 0,  r4 = 0,  r5 = 0,  r6 = 0,  r7 = 0;\n",
                "    uint32_t  r8 = 0,  r9 = 0, r10 = 0, r11 = 0, r12 = 0, r13 = 0, r14 = 0, r15 = 0;\n",
                "    uint32_t r16 = 0, r17 = 0, r18 = 0, r19 = 0, r20 = 0, r21 = 0, r22 = 0, r23 = 0;\n",
                "    uint32_t r24 = 0, r25 = 0, r26 = 0, r27 = 0, r28 = 0, r29 = 0, r30 = 0, r31 = 0;\n",
                "    uint32_t dma_mem_address = 0, dma_dram_address = 0, jump_target = 0;\n",
                "    const char * debug_file = NULL; int debug_line = 0;\n",
                "    RSP rsp{{}};\n",
                "    r1 = 0xFC0;\n",
            ),
            function_name
        );
    }

    // Write each instruction
    let context = FunctionContext {
        instructions: instrs,
        branch_targets: &branch_targets,
        unsupported_instructions: &config.unsupported_instructions,
        resume_targets,
        has_overlays: is_permutation,
    };
    for instr_index in 0..instrs.len() {
        context.process_instruction(out, instr_index, false, false)?;
    }

    // Terminate instruction code with a return to indicate that the microcode has run past its end
    out.push_str("    return RspExitReason::ImemOverrun;\n");

    // Write the section containing the indirect jump table
    write_indirect_jumps(out, &branch_targets, &config.output_function_name);

    // Write routine for returning for an overlay swap
    if is_permutation {
        write_overlay_swap_return(out);
    }

    // End the file
    out.push_str("}\n");
    Ok(())
}

/// Reads `len` bytes at `offset` as big-endian words. Like the C++ (which reads into a
/// zeroed vector), whatever lies past the end of the file reads as zero.
fn read_words(rom: &[u8], offset: u32, len: u32) -> Vec<u32> {
    let start = (offset as usize).min(rom.len());
    let bytes = &rom[start..];
    (0..(len / INSTR_SIZE) as usize)
        .map(|i| {
            let mut word = [0u8; 4];
            for (dst, src) in word.iter_mut().zip(bytes.iter().skip(i * 4)) {
                *dst = *src;
            }
            u32::from_be_bytes(word)
        })
        .collect()
}

fn decode(words: &[u32], text_address: u32) -> Vec<Instruction> {
    let start = text_address & RSP_MEM_MASK;
    words
        .iter()
        .zip((start..).step_by(INSTR_SIZE as usize))
        .map(|(&word, vram)| Instruction::new(word, vram, InstrCategory::RSP))
        .collect()
}

/// The C++ disables these pseudo instructions so disassembly comments show the real ones.
#[allow(unsafe_code)]
fn configure_rabbitizer() {
    // SAFETY: rabbitizer's global config is only read while decoding, and this runs once on
    // the main thread before anything is decoded.
    unsafe {
        let cfg = &raw mut rabbitizer::config::RabbitizerConfig_Cfg;
        (*cfg).pseudos.pseudo_move = false;
        (*cfg).pseudos.pseudo_beqz = false;
        (*cfg).pseudos.pseudo_bnez = false;
        (*cfg).pseudos.pseudo_not = false;
    }
}

fn run(config: &Config) -> Result<String, String> {
    let rom =
        std::fs::read(&config.rom_file_path).map_err(|_| "Failed to open rom file".to_owned())?;

    let instr_words = read_words(&rom, config.text_offset, config.text_size);
    let overlay_slots: Vec<OverlaySlot> = config
        .overlay_slots
        .iter()
        .map(|slot| OverlaySlot {
            offset: slot.text_address.wrapping_sub(config.text_address) & RSP_MEM_MASK,
            overlays: slot
                .overlays
                .iter()
                .map(|overlay| read_words(&rom, config.text_offset + overlay.offset, overlay.size))
                .collect(),
        })
        .collect();

    // Create overlay permutations
    let permutations = if overlay_slots.is_empty() {
        Vec::new()
    } else {
        permute(&instr_words, &overlay_slots)
    };

    configure_rabbitizer();

    // Decode the instruction words into instructions
    let instrs = decode(&instr_words, config.text_address);
    let func_permutations: Vec<FunctionPermutation> = permutations
        .into_iter()
        .map(|permutation| FunctionPermutation {
            instrs: decode(&permutation.instr_words, config.text_address),
            permutation: permutation.permutation,
        })
        .collect();

    // Determine all possible overlay swap resume targets
    let mut resume_targets = ResumeTargets::default();
    for permutation in &func_permutations {
        get_overlay_swap_resume_targets(&permutation.instrs, &mut resume_targets);
    }

    let mut out =
        String::from("#include \"librecomp/rsp.hpp\"\n#include \"librecomp/rsp_vu_impl.hpp\"\n");

    // Write function(s)
    let name = &config.output_function_name;
    let result = if overlay_slots.is_empty() {
        create_function(
            &mut out,
            name,
            &instrs,
            config,
            &resume_targets,
            false,
            false,
        )
    } else {
        create_overlay_swap_function(&mut out, &func_permutations, config);
        create_function(
            &mut out,
            &format!("{name}_initial"),
            &instrs,
            config,
            &ResumeTargets::default(),
            true,
            true,
        )
        .and_then(|()| {
            func_permutations.iter().try_for_each(|permutation| {
                let function_name = format!(
                    "{name}{}",
                    make_permutation_string(&permutation.permutation)
                );
                create_function(
                    &mut out,
                    &function_name,
                    &permutation.instrs,
                    config,
                    &resume_targets,
                    true,
                    false,
                )
            })
        })
    };
    result.map_err(|Unhandled(msg)| msg)?;
    Ok(out)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        println!("Usage: {} [config file]", args[0]);
        return ExitCode::SUCCESS;
    }

    let config = match config::read_config(Path::new(&args[1])) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Error reading config file {}:\n{err}", args[1]);
            return ExitCode::FAILURE;
        }
    };

    let output = match run(&config) {
        Ok(output) => output,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(parent) = config.output_file_path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            eprintln!("Failed to create {}: {err}", parent.display());
            return ExitCode::FAILURE;
        }
    }
    if let Err(err) = std::fs::write(&config.output_file_path, output) {
        eprintln!(
            "Failed to write {}: {err}",
            config.output_file_path.display()
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
