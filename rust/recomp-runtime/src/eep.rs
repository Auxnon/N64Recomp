//! EEPROM saves (librecomp's `eep.cpp`), through librecomp's save buffer.

use recomp_abi::{recomp_export, reg, GuestMemory, RecompContext};

use crate::cxx::{self, SaveType};

const EEPROM_BLOCK_SIZE: u32 = 8;
/// libultra's `EEPROM_TYPE_4K` and `EEPROM_TYPE_16K`.
const EEPROM_TYPE_4K: u32 = 1;
const EEPROM_TYPE_16K: u32 = 2;

fn require_eeprom() {
    if !cxx::eeprom_allowed() {
        cxx::fatal_error(c"Attempted to use EEPROM saving with other save type");
    }
}

/// `(eeprom block, guest buffer)` from `$a1`, `$a2`.
fn block_and_buffer(ctx: &RecompContext) -> (u32, i32) {
    (
        u32::from(ctx.r[reg::A1] as u8) * EEPROM_BLOCK_SIZE,
        ctx.r[reg::A2] as i32,
    )
}

fn transfer(
    mem: &GuestMemory,
    ctx: &mut RecompContext,
    count: u32,
    copy: fn(&GuestMemory, i32, u32, u32),
) {
    require_eeprom();
    let (offset, buffer) = block_and_buffer(ctx);
    copy(mem, buffer, offset, count);
    ctx.set_return_u32(0);
}

/// `$a3` as the C++ reads it: an `int32_t` byte count.
fn long_count(ctx: &RecompContext) -> u32 {
    ctx.r[reg::A3] as i32 as u32
}

recomp_export! {
    /// `s32 osEepromProbe(OSMesgQueue*)`
    fn osEepromProbe_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        let kind = match cxx::save_type() {
            SaveType::AllowAll | SaveType::Eep16k => EEPROM_TYPE_16K,
            SaveType::Eep4k => EEPROM_TYPE_4K,
            _ => 0,
        };
        ctx.set_return_u32(kind);
    }

    /// `s32 osEepromWrite(OSMesgQueue*, u8 address, u8* buffer)`: one 8-byte block.
    fn osEepromWrite_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        transfer(mem, ctx, EEPROM_BLOCK_SIZE, cxx::save_write);
    }

    /// `s32 osEepromLongWrite(OSMesgQueue*, u8 address, u8* buffer, int bytes)`
    fn osEepromLongWrite_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        let count = long_count(ctx);
        transfer(mem, ctx, count, cxx::save_write);
    }

    /// `s32 osEepromRead(OSMesgQueue*, u8 address, u8* buffer)`: one 8-byte block.
    fn osEepromRead_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        transfer(mem, ctx, EEPROM_BLOCK_SIZE, cxx::save_read);
    }

    /// `s32 osEepromLongRead(OSMesgQueue*, u8 address, u8* buffer, int bytes)`
    fn osEepromLongRead_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        let count = long_count(ctx);
        transfer(mem, ctx, count, cxx::save_read);
    }
}
