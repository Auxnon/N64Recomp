//! Instruction decoding helpers.

use rabbitizer::{InstrCategory, Instruction};

/// Instruction fields, decoded like rabbitizer's `RAB_INSTR_GET_*` macros. The C++ tool is
/// built without `RAB_SANITY_CHECKS`, so its getters never check that an operand exists;
/// the Rust bindings' getters panic instead, so the fields are decoded here.
#[derive(Clone, Copy, Debug)]
pub struct Fields {
    pub rs: u32,
    pub rt: u32,
    pub rd: u32,
    pub sa: u32,
    pub fs: u32,
    pub ft: u32,
    pub fd: u32,
    /// Also the COP1 control register (`cop1cs`) and the COP0 register (`cop0d`).
    pub cop: u32,
    pub imm: u16,
}

impl Fields {
    pub fn of(instr: &Instruction) -> Self {
        let word = instr.raw();
        let bits = |shift: u32, width: u32| (word >> shift) & ((1 << width) - 1);
        Fields {
            rs: bits(21, 5),
            rt: bits(16, 5),
            rd: bits(11, 5),
            sa: bits(6, 5),
            fs: bits(11, 5),
            ft: bits(16, 5),
            fd: bits(6, 5),
            cop: bits(11, 5),
            imm: bits(0, 16) as u16,
        }
    }
}

/// Decodes a CPU instruction from a word as stored in [`crate::context::Function::words`].
pub fn decode_cpu(stored_word: u32, vram: u32) -> Instruction {
    Instruction::new(stored_word.swap_bytes(), vram, InstrCategory::CPU)
}

/// The C++ disables these pseudo instructions so disassembly comments show the real ones.
#[allow(unsafe_code)]
pub fn configure_rabbitizer() {
    // SAFETY: rabbitizer's global config is only read while decoding, and this runs once on
    // the main thread before anything is decoded.
    unsafe {
        let cfg = &raw mut rabbitizer::config::RabbitizerConfig_Cfg;
        (*cfg).pseudos.pseudo_move = false;
        (*cfg).pseudos.pseudo_beqz = false;
        (*cfg).pseudos.pseudo_bnez = false;
        (*cfg).pseudos.pseudo_not = false;
        (*cfg).pseudos.pseudo_bal = false;
    }
}
