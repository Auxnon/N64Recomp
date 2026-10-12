//! Jump table detection (`src/analysis.cpp`).

use rabbitizer::{InstrId, Instruction};
use recomp_core::cxx_sort;

use crate::context::{Context, Function, JumpTable, read_rom_word};
use crate::instr::Fields;

const REG_GP: u32 = 28;
const REG_SP: u32 = 29;
const REG_RA: u32 = 31;

#[derive(Default)]
pub struct FunctionStats {
    pub jump_tables: Vec<JumpTable>,
}

/// Tracks what a register holds, to spot `lui`/`addiu`/`addu`/`lw`/`jr` jump table sequences.
/// (If 64-bit addressing is ever implemented, these will need to be 64-bit values.)
#[derive(Clone, Copy, Default)]
struct RegState {
    // For tracking a register that will be used to load from RAM.
    prev_lui: u32,
    prev_addiu_vram: u32,
    prev_addu_vram: u32,
    prev_addend_reg: u8,
    /// Offset of `lw rt, offset(gp)`.
    prev_got_offset: u32,
    valid_lui: bool,
    valid_addiu: bool,
    valid_addend: bool,
    valid_got_offset: bool,
    // For tracking a register that has been loaded from RAM.
    loaded_lw_vram: u32,
    loaded_addu_vram: u32,
    loaded_address: u32,
    loaded_addend_reg: u8,
    valid_loaded: bool,
    /// A valid load through the GOT.
    valid_got_loaded: bool,
}

impl RegState {
    fn invalidate(&mut self) {
        *self = RegState::default();
    }
}

fn analyze_instruction(
    instr: &Instruction,
    stats: &mut FunctionStats,
    reg_states: &mut [RegState; 32],
    stack_states: &mut Vec<RegState>,
    is_got_addr_defined: bool,
) -> bool {
    let f = Fields::of(instr);
    let (rd, rs, rt) = (f.rd as usize, f.rs as usize, f.rt as usize);
    let base = rs;
    let imm = f.imm;
    let mut temp = RegState::default();

    let check_move = |reg_states: &mut [RegState; 32]| {
        if rs == 0 {
            // rs is zero, so copy rt to rd
            reg_states[rd] = reg_states[rt];
        } else if rt == 0 {
            // rt is zero, so copy rs to rd
            reg_states[rd] = reg_states[rs];
        } else {
            // Not a move, invalidate rd
            reg_states[rd].invalidate();
        }
    };

    // Stack slot for `sw`/`lw` relative to $sp. Fails on misaligned or negative offsets.
    let stack_slot = |what: &str| -> Option<usize> {
        if imm & 3 != 0 {
            eprintln!(
                "Invalid alignment on offset for {what} stack: {}",
                imm as i16
            );
            return None;
        }
        if (imm as i16) < 0 {
            eprintln!("Negative offset for {what} stack: {}", imm as i16);
            return None;
        }
        Some(usize::from(imm) / 4)
    };

    match instr.unique_id {
        InstrId::cpu_lui => {
            // rt has been completely overwritten, so invalidate it
            reg_states[rt].invalidate();
            reg_states[rt].prev_lui = ((imm as i16 as i32) << 16) as u32;
            reg_states[rt].valid_lui = true;
        }
        InstrId::cpu_addiu => {
            // The target reg is a copy of the source reg plus an immediate.
            reg_states[rt] = reg_states[rs];
            // Set the addiu state if and only if there hasn't been an addiu already.
            if !reg_states[rt].valid_addiu {
                reg_states[rt].prev_addiu_vram = imm as i16 as u32;
                reg_states[rt].valid_addiu = true;
            } else {
                // Two or more consecutive addius, so invalidate the whole register.
                reg_states[rt].invalidate();
            }
        }
        InstrId::cpu_addu => {
            // rd has been completely overwritten, so invalidate it
            temp.invalidate();
            if reg_states[rs].valid_got_offset != reg_states[rt].valid_got_offset {
                // One register has a valid GOT offset; the other is the addend.
                let (got_reg, addend_reg) = if reg_states[rs].valid_got_offset {
                    (rs, rt)
                } else {
                    (rt, rs)
                };
                temp = reg_states[got_reg];
                temp.valid_addend = true;
                temp.prev_addend_reg = addend_reg as u8;
                temp.prev_addu_vram = instr.vram;
            } else if (rs == REG_GP as usize || rt == REG_GP as usize)
                && reg_states[rs].valid_got_loaded != reg_states[rt].valid_got_loaded
            {
                // `addu rd, rs, $gp` or `addu rd, $gp, rt` after a valid GOT load: the last
                // part of a position independent jump table call. Keep the register state.
                let loaded_reg = if reg_states[rs].valid_got_loaded {
                    rs
                } else {
                    rt
                };
                temp = reg_states[loaded_reg];
            }
            // Exactly one of the two addend register states should have a valid lui.
            else if reg_states[rs].valid_lui != reg_states[rt].valid_lui {
                let (lui_reg, addend_reg) = if reg_states[rs].valid_lui {
                    (rs, rt)
                } else {
                    (rt, rs)
                };
                temp = reg_states[lui_reg];
                temp.valid_addend = true;
                temp.prev_addend_reg = addend_reg as u8;
                temp.prev_addu_vram = instr.vram;
            } else {
                // Check if this is a move.
                check_move(reg_states);
            }
            reg_states[rd] = temp;
        }
        InstrId::cpu_daddu | InstrId::cpu_or => check_move(reg_states),
        InstrId::cpu_sw => {
            // A store to the stack copies rt's state into the stack slot.
            if base == REG_SP as usize {
                let Some(slot) = stack_slot("sw to") else {
                    return false;
                };
                if slot >= stack_states.len() {
                    stack_states.resize(slot + 1, RegState::default());
                }
                stack_states[slot] = reg_states[rt];
            }
        }
        InstrId::cpu_lw => {
            // rt has been completely overwritten, so invalidate it
            temp.invalidate();
            if base == REG_SP as usize {
                // A load from the stack copies the slot's state into rt.
                let Some(slot) = stack_slot("lw from") else {
                    return false;
                };
                if slot >= stack_states.len() {
                    stack_states.resize(slot + 1, RegState::default());
                }
                temp = stack_states[slot];
            } else if reg_states[base].valid_lui && reg_states[base].valid_addend {
                // Possibly a load from a jump table. Exactly one of the lw and the base reg
                // should have a valid lo16, but the lo16 may be zero by luck, so the only
                // invalid case is both.
                let nonzero_immediate = imm != 0;
                if !(nonzero_immediate && reg_states[base].valid_addiu) {
                    let lo16 = if nonzero_immediate {
                        imm as i16 as u32
                    } else {
                        reg_states[base].prev_addiu_vram
                    };
                    temp.valid_loaded = true;
                    temp.loaded_lw_vram = instr.vram;
                    temp.loaded_address = reg_states[base].prev_lui.wrapping_add(lo16);
                    temp.loaded_addend_reg = reg_states[base].prev_addend_reg;
                    temp.loaded_addu_vram = reg_states[base].prev_addu_vram;
                }
            } else if reg_states[base].valid_got_offset && reg_states[base].valid_addend {
                // Possibly a load from a position independent jump table. The address is
                // relative to the GOT entry for now; it's made absolute later.
                temp.valid_got_loaded = true;
                temp.loaded_lw_vram = instr.vram;
                temp.loaded_address = u32::from(imm);
                temp.loaded_addend_reg = reg_states[base].prev_addend_reg;
                temp.loaded_addu_vram = reg_states[base].prev_addu_vram;
                temp.prev_got_offset = reg_states[base].prev_got_offset;
            } else if base == REG_GP as usize && is_got_addr_defined {
                // lw from $gp reads the global offset table.
                temp.prev_got_offset = u32::from(imm);
                temp.valid_got_offset = true;
            }
            reg_states[rt] = temp;
        }
        InstrId::cpu_jr => {
            // Ignore jr $ra
            if rs != REG_RA as usize {
                let state = reg_states[rs];
                // A jr through a register loaded from a table is a jump table.
                if state.valid_loaded || state.valid_got_loaded {
                    stats.jump_tables.push(JumpTable {
                        vram: state.loaded_address,
                        addend_reg: u32::from(state.loaded_addend_reg),
                        rom: 0,
                        lw_vram: state.loaded_lw_vram,
                        addu_vram: state.loaded_addu_vram,
                        jr_vram: instr.vram,
                        // Filled in later.
                        section_index: 0,
                        got_offset: if state.valid_loaded {
                            None
                        } else {
                            Some(state.prev_got_offset)
                        },
                        entries: Vec::new(),
                    });
                }
                // TODO (from the C++): stricter validation on tail calls.
            }
        }
        _ => {
            if instr.modifies_rd() {
                reg_states[rd].invalidate();
            }
            if instr.modifies_rt() {
                reg_states[rt].invalidate();
            }
        }
    }
    true
}

pub fn analyze_function(
    context: &Context,
    func: &Function,
    instructions: &[Instruction],
) -> Option<FunctionStats> {
    let section = &context.sections[usize::from(func.section_index)];
    let got_ram_addr = section.got_ram_addr;

    // One state per register (r0 isn't used).
    let mut reg_states = [RegState::default(); 32];
    let mut stack_states = Vec::new();
    let mut stats = FunctionStats::default();

    // A linear scan ignores control flow, but it's enough to find jump tables.
    for instr in instructions {
        if !analyze_instruction(
            instr,
            &mut stats,
            &mut reg_states,
            &mut stack_states,
            got_ram_addr.is_some(),
        ) {
            return None;
        }
    }

    // Calculate absolute addresses for position-independent jump tables.
    if let Some(got_ram_addr) = got_ram_addr {
        let got_rom_addr = got_ram_addr.wrapping_add(func.rom).wrapping_sub(func.vram);
        for jtbl in &mut stats.jump_tables {
            if let Some(got_offset) = jtbl.got_offset {
                let got_word =
                    read_rom_word(&context.rom, got_rom_addr.wrapping_add(got_offset) as usize)
                        .swap_bytes();
                jtbl.vram = jtbl
                    .vram
                    .wrapping_add(section.ram_addr.wrapping_add(got_word));
            }
        }
    }

    // Sort jump tables by their address.
    cxx_sort::sort_by(&mut stats.jump_tables, |a, b| a.vram < b.vram);

    // Determine jump table sizes. The C++ computes the end in size_t, so it doesn't wrap.
    let func_end = u64::from(func.vram) + func.words.len() as u64 * 4;
    for i in 0..stats.jump_tables.len() {
        let end_address = stats
            .jump_tables
            .get(i + 1)
            .map_or(u32::MAX, |next| next.vram);
        let jtbl = &mut stats.jump_tables[i];

        // TODO (from the C++): this assumes the jump table is in the function's section.
        jtbl.rom = jtbl.vram.wrapping_add(func.rom).wrapping_sub(func.vram);
        jtbl.section_index = func.section_index;

        let mut vram = jtbl.vram;
        while vram < end_address {
            let rom_addr = vram.wrapping_add(func.rom).wrapping_sub(func.vram);
            let mut entry = read_rom_word(&context.rom, rom_addr as usize).swap_bytes();
            if let (Some(_), Some(got)) = (jtbl.got_offset, got_ram_addr) {
                // Position independent jump table entries are offsets from the GOT.
                entry = entry.wrapping_add(got);
            }
            // The table ends at the first entry outside the function.
            if entry < func.vram || u64::from(entry) >= func_end {
                break;
            }
            jtbl.entries.push(entry);
            vram = vram.wrapping_add(4);
        }

        if jtbl.entries.is_empty() {
            println!(
                "Failed to determine size of jump table at 0x{:08X} for instruction at 0x{:08X}",
                jtbl.vram, jtbl.jr_vram
            );
            return None;
        }
    }

    Some(stats)
}
