//! Instruction tables (`include/recompiler/operations.h`, `src/operations.cpp`).

use rabbitizer::InstrId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreOpType {
    Sd,
    Sdl,
    Sdr,
    Sw,
    Swl,
    Swr,
    Sh,
    Sb,
    Sdc1,
    Swc1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOpType {
    None,
    ToS32,
    ToU32,
    ToS64,
    ToU64,
    Lui,
    /// Mask to 5 bits.
    Mask5,
    /// Mask to 6 bits.
    Mask6,
    /// Functionally equivalent to ToS32; only exists for parity with old codegen.
    ToInt32,
    NegateFloat,
    NegateDouble,
    AbsFloat,
    AbsDouble,
    SqrtFloat,
    SqrtDouble,
    ConvertSFromW,
    ConvertWFromS,
    ConvertDFromW,
    ConvertWFromD,
    ConvertDFromS,
    ConvertSFromD,
    ConvertDFromL,
    ConvertLFromD,
    ConvertSFromL,
    ConvertLFromS,
    TruncateWFromS,
    TruncateWFromD,
    TruncateLFromS,
    TruncateLFromD,
    RoundWFromS,
    RoundWFromD,
    RoundLFromS,
    RoundLFromD,
    CeilWFromS,
    CeilWFromD,
    CeilLFromS,
    CeilLFromD,
    FloorWFromS,
    FloorWFromD,
    FloorLFromS,
    FloorLFromD,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOpType {
    // Addition/subtraction
    Add32,
    Sub32,
    Add64,
    Sub64,
    // Float arithmetic
    AddFloat,
    AddDouble,
    SubFloat,
    SubDouble,
    MulFloat,
    MulDouble,
    DivFloat,
    DivDouble,
    // Bitwise
    And64,
    Or64,
    Nor64,
    Xor64,
    Sll32,
    Sll64,
    Srl32,
    Srl64,
    Sra32,
    Sra64,
    // Comparisons
    Equal,
    NotEqual,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    EqualFloat,
    LessFloat,
    LessEqFloat,
    EqualDouble,
    LessDouble,
    LessEqDouble,
    // Loads
    Ld,
    Lw,
    Lwu,
    Lh,
    Lhu,
    Lb,
    Lbu,
    Ldl,
    Ldr,
    Lwl,
    Lwr,
    // Fixed result
    True,
    False,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    /// GPRs.
    Rd,
    Rs,
    Rt,
    /// FPRs.
    Fd,
    Fs,
    Ft,
    /// Double floats in FPRs.
    FdDouble,
    FsDouble,
    FtDouble,
    /// Raw low 32-bit values of FPRs, with handling for mips3 float mode.
    FdU32L,
    FsU32L,
    FtU32L,
    /// Raw high 32-bit values of FPRs, with handling for mips3 float mode.
    FdU32H,
    FsU32H,
    FtU32H,
    /// Raw 64-bit values of FPRs.
    FdU64,
    FsU64,
    FtU64,
    /// 16-bit immediate, unsigned.
    ImmU16,
    /// 16-bit immediate, signed.
    ImmS16,
    /// Shift amount.
    Sa,
    /// Shift amount plus 32.
    Sa32,
    /// Coprocessor 1 condition signal.
    Cop1cs,
    Hi,
    Lo,
    Zero,
}

impl Operand {
    /// Alias for `Rs` in loads and stores.
    pub const BASE: Operand = Operand::Rs;
}

#[derive(Clone, Copy, Debug)]
pub struct StoreOp {
    pub kind: StoreOpType,
    pub value_input: Operand,
}

#[derive(Clone, Copy, Debug)]
pub struct UnaryOp {
    pub operation: UnaryOpType,
    pub output: Operand,
    pub input: Operand,
    /// Whether the FR bit needs to be checked for odd float registers.
    pub check_fr: bool,
    /// Whether the input needs to be checked for being NaN.
    pub check_nan: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct BinaryOperands {
    /// Operation applied to each operand before the binary operation.
    pub operand_operations: [UnaryOpType; 2],
    /// The source of the input operands.
    pub operands: [Operand; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct BinaryOp {
    pub kind: BinaryOpType,
    pub output: Operand,
    pub operands: BinaryOperands,
    pub check_fr: bool,
    pub check_nan: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ConditionalBranchOp {
    /// The comparison to branch on.
    pub comparison: BinaryOpType,
    pub operands: BinaryOperands,
    /// Whether this branch links for returns.
    pub link: bool,
    /// Whether this branch is "likely" (skips the delay slot when not taken).
    pub likely: bool,
}

use Operand as O;
use UnaryOpType as U;

const fn unary(
    operation: UnaryOpType,
    output: Operand,
    input: Operand,
    check_fr: bool,
    check_nan: bool,
) -> UnaryOp {
    UnaryOp {
        operation,
        output,
        input,
        check_fr,
        check_nan,
    }
}

pub fn unary_op(id: InstrId) -> Option<UnaryOp> {
    use InstrId::*;
    Some(match id {
        cpu_lui => unary(U::Lui, O::Rt, O::ImmU16, false, false),
        cpu_mthi => unary(U::None, O::Hi, O::Rs, false, false),
        cpu_mtlo => unary(U::None, O::Lo, O::Rs, false, false),
        cpu_mfhi => unary(U::None, O::Rd, O::Hi, false, false),
        cpu_mflo => unary(U::None, O::Rd, O::Lo, false, false),
        cpu_mtc1 => unary(U::None, O::FsU32L, O::Rt, false, false),
        cpu_mfc1 => unary(U::ToInt32, O::Rt, O::FsU32L, false, false),
        cpu_dmtc1 => unary(U::None, O::FsU64, O::Rt, false, false),
        cpu_dmfc1 => unary(U::None, O::Rt, O::FsU64, false, false),
        // Float operations
        cpu_mov_s => unary(U::None, O::Fd, O::Fs, true, false),
        cpu_mov_d => unary(U::None, O::FdDouble, O::FsDouble, true, false),
        cpu_neg_s => unary(U::NegateFloat, O::Fd, O::Fs, true, true),
        cpu_neg_d => unary(U::NegateDouble, O::FdDouble, O::FsDouble, true, true),
        cpu_abs_s => unary(U::AbsFloat, O::Fd, O::Fs, true, true),
        cpu_abs_d => unary(U::AbsDouble, O::FdDouble, O::FsDouble, true, true),
        cpu_sqrt_s => unary(U::SqrtFloat, O::Fd, O::Fs, true, true),
        cpu_sqrt_d => unary(U::SqrtDouble, O::FdDouble, O::FsDouble, true, true),
        cpu_cvt_s_w => unary(U::ConvertSFromW, O::Fd, O::FsU32L, true, false),
        cpu_cvt_w_s => unary(U::ConvertWFromS, O::FdU32L, O::Fs, true, false),
        cpu_cvt_d_w => unary(U::ConvertDFromW, O::FdDouble, O::FsU32L, true, false),
        cpu_cvt_w_d => unary(U::ConvertWFromD, O::FdU32L, O::FsDouble, true, false),
        cpu_cvt_d_s => unary(U::ConvertDFromS, O::FdDouble, O::Fs, true, true),
        cpu_cvt_s_d => unary(U::ConvertSFromD, O::Fd, O::FsDouble, true, true),
        cpu_cvt_d_l => unary(U::ConvertDFromL, O::FdDouble, O::FsU64, true, false),
        cpu_cvt_l_d => unary(U::ConvertLFromD, O::FdU64, O::FsDouble, true, true),
        cpu_cvt_s_l => unary(U::ConvertSFromL, O::Fd, O::FsU64, true, false),
        cpu_cvt_l_s => unary(U::ConvertLFromS, O::FdU64, O::Fs, true, true),
        cpu_trunc_w_s => unary(U::TruncateWFromS, O::FdU32L, O::Fs, true, false),
        cpu_trunc_w_d => unary(U::TruncateWFromD, O::FdU32L, O::FsDouble, true, false),
        cpu_round_w_s => unary(U::RoundWFromS, O::FdU32L, O::Fs, true, false),
        cpu_round_w_d => unary(U::RoundWFromD, O::FdU32L, O::FsDouble, true, false),
        cpu_ceil_w_s => unary(U::CeilWFromS, O::FdU32L, O::Fs, true, false),
        cpu_ceil_w_d => unary(U::CeilWFromD, O::FdU32L, O::FsDouble, true, false),
        cpu_floor_w_s => unary(U::FloorWFromS, O::FdU32L, O::Fs, true, false),
        cpu_floor_w_d => unary(U::FloorWFromD, O::FdU32L, O::FsDouble, true, false),
        _ => return None,
    })
}

const fn operands(ops: [UnaryOpType; 2], inputs: [Operand; 2]) -> BinaryOperands {
    BinaryOperands {
        operand_operations: ops,
        operands: inputs,
    }
}

const fn binary(
    kind: BinaryOpType,
    output: Operand,
    ops: [UnaryOpType; 2],
    inputs: [Operand; 2],
) -> BinaryOp {
    BinaryOp {
        kind,
        output,
        operands: operands(ops, inputs),
        check_fr: false,
        check_nan: false,
    }
}

const fn float_binary(
    kind: BinaryOpType,
    output: Operand,
    inputs: [Operand; 2],
    check_fr: bool,
    check_nan: bool,
) -> BinaryOp {
    BinaryOp {
        kind,
        output,
        operands: operands([U::None, U::None], inputs),
        check_fr,
        check_nan,
    }
}

pub fn binary_op(id: InstrId) -> Option<BinaryOp> {
    use BinaryOpType as B;
    use InstrId::*;
    const NN: [UnaryOpType; 2] = [U::None, U::None];
    const RS_RT: [Operand; 2] = [O::Rs, O::Rt];
    const RS_IMMS: [Operand; 2] = [O::Rs, O::ImmS16];
    const RS_IMMU: [Operand; 2] = [O::Rs, O::ImmU16];
    const FS_FT: [Operand; 2] = [O::Fs, O::Ft];
    const FSD_FTD: [Operand; 2] = [O::FsDouble, O::FtDouble];
    const BASE_IMM: [Operand; 2] = [O::BASE, O::ImmS16];
    Some(match id {
        // Addition/subtraction
        cpu_addu | cpu_add => binary(B::Add32, O::Rd, NN, RS_RT),
        // negu is a pseudo op for subu
        cpu_negu | cpu_subu | cpu_sub => binary(B::Sub32, O::Rd, NN, RS_RT),
        cpu_daddu | cpu_dadd => binary(B::Add64, O::Rd, NN, RS_RT),
        cpu_dsubu | cpu_dsub => binary(B::Sub64, O::Rd, NN, RS_RT),
        // Addition/subtraction (immediate)
        cpu_addi | cpu_addiu => binary(B::Add32, O::Rt, NN, RS_IMMS),
        cpu_daddi | cpu_daddiu => binary(B::Add64, O::Rt, NN, RS_IMMS),
        // Bitwise
        cpu_and => binary(B::And64, O::Rd, NN, RS_RT),
        cpu_or => binary(B::Or64, O::Rd, NN, RS_RT),
        cpu_nor => binary(B::Nor64, O::Rd, NN, RS_RT),
        cpu_xor => binary(B::Xor64, O::Rd, NN, RS_RT),
        // Bitwise (immediate)
        cpu_andi => binary(B::And64, O::Rt, NN, RS_IMMU),
        cpu_ori => binary(B::Or64, O::Rt, NN, RS_IMMU),
        cpu_xori => binary(B::Xor64, O::Rt, NN, RS_IMMU),
        // Shifts
        cpu_sllv => binary(B::Sll32, O::Rd, [U::None, U::Mask5], [O::Rt, O::Rs]),
        cpu_dsllv => binary(B::Sll64, O::Rd, [U::None, U::Mask6], [O::Rt, O::Rs]),
        cpu_srlv => binary(B::Srl32, O::Rd, [U::ToU32, U::Mask5], [O::Rt, O::Rs]),
        cpu_dsrlv => binary(B::Srl64, O::Rd, [U::ToU64, U::Mask6], [O::Rt, O::Rs]),
        // Hardware bug: the input isn't masked to 32 bits before right shifting, so bits
        // from the upper half of the register bleed into the lower half.
        cpu_srav => binary(B::Sra32, O::Rd, [U::ToS64, U::Mask5], [O::Rt, O::Rs]),
        cpu_dsrav => binary(B::Sra64, O::Rd, [U::ToS64, U::Mask6], [O::Rt, O::Rs]),
        // Shifts (immediate)
        cpu_sll => binary(B::Sll32, O::Rd, NN, [O::Rt, O::Sa]),
        cpu_dsll => binary(B::Sll64, O::Rd, NN, [O::Rt, O::Sa]),
        cpu_dsll32 => binary(B::Sll64, O::Rd, NN, [O::Rt, O::Sa32]),
        cpu_srl => binary(B::Srl32, O::Rd, [U::ToU32, U::None], [O::Rt, O::Sa]),
        cpu_dsrl => binary(B::Srl64, O::Rd, [U::ToU64, U::None], [O::Rt, O::Sa]),
        cpu_dsrl32 => binary(B::Srl64, O::Rd, [U::ToU64, U::None], [O::Rt, O::Sa32]),
        // Same hardware bug as srav.
        cpu_sra => binary(B::Sra32, O::Rd, [U::ToS64, U::None], [O::Rt, O::Sa]),
        cpu_dsra => binary(B::Sra64, O::Rd, [U::ToS64, U::None], [O::Rt, O::Sa]),
        cpu_dsra32 => binary(B::Sra64, O::Rd, [U::ToS64, U::None], [O::Rt, O::Sa32]),
        // Comparisons
        cpu_slt => binary(B::Less, O::Rd, [U::ToS64, U::ToS64], RS_RT),
        cpu_sltu => binary(B::Less, O::Rd, [U::ToU64, U::ToU64], RS_RT),
        // Comparisons (immediate)
        cpu_slti => binary(B::Less, O::Rt, [U::ToS64, U::None], RS_IMMS),
        cpu_sltiu => binary(B::Less, O::Rt, [U::ToU64, U::None], RS_IMMS),
        // Float arithmetic
        cpu_add_s => float_binary(B::AddFloat, O::Fd, FS_FT, true, true),
        cpu_add_d => float_binary(B::AddDouble, O::FdDouble, FSD_FTD, true, true),
        cpu_sub_s => float_binary(B::SubFloat, O::Fd, FS_FT, true, true),
        cpu_sub_d => float_binary(B::SubDouble, O::FdDouble, FSD_FTD, true, true),
        cpu_mul_s => float_binary(B::MulFloat, O::Fd, FS_FT, true, true),
        cpu_mul_d => float_binary(B::MulDouble, O::FdDouble, FSD_FTD, true, true),
        cpu_div_s => float_binary(B::DivFloat, O::Fd, FS_FT, true, true),
        cpu_div_d => float_binary(B::DivDouble, O::FdDouble, FSD_FTD, true, true),
        // Float comparisons (single, ordered then unordered)
        cpu_c_f_s | cpu_c_sf_s | cpu_c_un_s | cpu_c_ngle_s => {
            float_binary(B::False, O::Cop1cs, FS_FT, true, false)
        }
        cpu_c_eq_s | cpu_c_seq_s | cpu_c_ueq_s | cpu_c_ngl_s => {
            float_binary(B::EqualFloat, O::Cop1cs, FS_FT, true, false)
        }
        cpu_c_olt_s | cpu_c_lt_s | cpu_c_ult_s | cpu_c_nge_s => {
            float_binary(B::LessFloat, O::Cop1cs, FS_FT, true, false)
        }
        cpu_c_ole_s | cpu_c_le_s | cpu_c_ule_s | cpu_c_ngt_s => {
            float_binary(B::LessEqFloat, O::Cop1cs, FS_FT, true, false)
        }
        // Float comparisons (double). rabbitizer names c.sf.d and c.seq.d c_df_d and c_deq_d.
        cpu_c_f_d | cpu_c_df_d | cpu_c_un_d | cpu_c_ngle_d => {
            float_binary(B::False, O::Cop1cs, FSD_FTD, true, false)
        }
        cpu_c_eq_d | cpu_c_deq_d | cpu_c_ueq_d | cpu_c_ngl_d => {
            float_binary(B::EqualDouble, O::Cop1cs, FSD_FTD, true, false)
        }
        cpu_c_olt_d | cpu_c_lt_d | cpu_c_ult_d | cpu_c_nge_d => {
            float_binary(B::LessDouble, O::Cop1cs, FSD_FTD, true, false)
        }
        cpu_c_ole_d | cpu_c_le_d | cpu_c_ule_d | cpu_c_ngt_d => {
            float_binary(B::LessEqDouble, O::Cop1cs, FSD_FTD, true, false)
        }
        // Loads
        cpu_ld => binary(B::Ld, O::Rt, NN, BASE_IMM),
        cpu_lw => binary(B::Lw, O::Rt, NN, BASE_IMM),
        cpu_lwu => binary(B::Lwu, O::Rt, NN, BASE_IMM),
        cpu_lh => binary(B::Lh, O::Rt, NN, BASE_IMM),
        cpu_lhu => binary(B::Lhu, O::Rt, NN, BASE_IMM),
        cpu_lb => binary(B::Lb, O::Rt, NN, BASE_IMM),
        cpu_lbu => binary(B::Lbu, O::Rt, NN, BASE_IMM),
        cpu_ldl => binary(B::Ldl, O::Rt, NN, BASE_IMM),
        cpu_ldr => binary(B::Ldr, O::Rt, NN, BASE_IMM),
        cpu_lwl => binary(B::Lwl, O::Rt, NN, BASE_IMM),
        cpu_lwr => binary(B::Lwr, O::Rt, NN, BASE_IMM),
        cpu_lwc1 => binary(B::Lw, O::FtU32L, NN, BASE_IMM),
        cpu_ldc1 => BinaryOp {
            check_fr: true,
            ..binary(B::Ld, O::FtU64, NN, BASE_IMM)
        },
        _ => return None,
    })
}

pub fn conditional_branch_op(id: InstrId) -> Option<ConditionalBranchOp> {
    use BinaryOpType as B;
    use InstrId::*;
    let op = |comparison, ops, inputs, link, likely| ConditionalBranchOp {
        comparison,
        operands: operands(ops, inputs),
        link,
        likely,
    };
    const NN: [UnaryOpType; 2] = [U::None, U::None];
    const SN: [UnaryOpType; 2] = [U::ToS64, U::None];
    const RS_RT: [Operand; 2] = [O::Rs, O::Rt];
    const RS_ZERO: [Operand; 2] = [O::Rs, O::Zero];
    const CS_ZERO: [Operand; 2] = [O::Cop1cs, O::Zero];
    Some(match id {
        cpu_beq => op(B::Equal, NN, RS_RT, false, false),
        cpu_beql => op(B::Equal, NN, RS_RT, false, true),
        cpu_bne => op(B::NotEqual, NN, RS_RT, false, false),
        cpu_bnel => op(B::NotEqual, NN, RS_RT, false, true),
        cpu_bgez => op(B::GreaterEq, SN, RS_ZERO, false, false),
        cpu_bgezl => op(B::GreaterEq, SN, RS_ZERO, false, true),
        cpu_bgtz => op(B::Greater, SN, RS_ZERO, false, false),
        cpu_bgtzl => op(B::Greater, SN, RS_ZERO, false, true),
        cpu_blez => op(B::LessEq, SN, RS_ZERO, false, false),
        cpu_blezl => op(B::LessEq, SN, RS_ZERO, false, true),
        cpu_bltz => op(B::Less, SN, RS_ZERO, false, false),
        cpu_bltzl => op(B::Less, SN, RS_ZERO, false, true),
        cpu_bgezal => op(B::GreaterEq, SN, RS_ZERO, true, false),
        cpu_bgezall => op(B::GreaterEq, SN, RS_ZERO, true, true),
        cpu_bltzal => op(B::Less, SN, RS_ZERO, true, false),
        cpu_bltzall => op(B::Less, SN, RS_ZERO, true, true),
        cpu_bc1f => op(B::Equal, NN, CS_ZERO, false, false),
        cpu_bc1fl => op(B::Equal, NN, CS_ZERO, false, true),
        cpu_bc1t => op(B::NotEqual, NN, CS_ZERO, false, false),
        cpu_bc1tl => op(B::NotEqual, NN, CS_ZERO, false, true),
        _ => return None,
    })
}

pub fn store_op(id: InstrId) -> Option<StoreOp> {
    use InstrId::*;
    let op = |kind, value_input| StoreOp { kind, value_input };
    Some(match id {
        cpu_sd => op(StoreOpType::Sd, O::Rt),
        cpu_sdl => op(StoreOpType::Sdl, O::Rt),
        cpu_sdr => op(StoreOpType::Sdr, O::Rt),
        cpu_sw => op(StoreOpType::Sw, O::Rt),
        cpu_swl => op(StoreOpType::Swl, O::Rt),
        cpu_swr => op(StoreOpType::Swr, O::Rt),
        cpu_sh => op(StoreOpType::Sh, O::Rt),
        cpu_sb => op(StoreOpType::Sb, O::Rt),
        cpu_sdc1 => op(StoreOpType::Sdc1, O::FtU64),
        cpu_swc1 => op(StoreOpType::Swc1, O::FtU32L),
        _ => return None,
    })
}
