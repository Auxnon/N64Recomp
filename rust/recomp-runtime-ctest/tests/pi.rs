//! The Rust PI exports (ROM and SRAM DMA, handle setup) against librecomp's C++
//! (`c/pi_reference.cpp`), on the same ROM, with the same harness as the save commands.

use recomp_abi::{RDRAM_BASE, reg};
use recomp_runtime::pi;
use recomp_runtime_ctest::harness::{Pair, Rng, RustFn, context};
use recomp_runtime_ctest::*;

/// Up to the PI handles librecomp keeps at 0x80800000, plus room past them.
const WORDS: usize = (8 * 1024 * 1024 + 0x1000) / 4;
const STACK: u64 = 0x100;
const DATA: u64 = 0x1000;
const DATA_END: u64 = 0x10_0000;
const ROM_SIZE: usize = 1024 * 1024;
/// Where the tests keep their own `OSPiHandle`s and `OSIoMesg`s.
const SRAM_HANDLE: u64 = 0x400;
const UNMAPPED_HANDLE: u64 = 0x480;
const MESG: u64 = 0x500;
const CART_HANDLE: u64 = 0x80_0000;

#[test]
fn pi_exports_match_the_cpp() {
    let mut rng = Rng(0x0005_EED0_FD1A);
    let pair = Pair::new(&mut rng, WORDS);
    unsafe { ctest_set_save_type(5) }; // AllowAll, so SRAM is allowed
    let rom: Vec<u8> = (0..ROM_SIZE).map(|_| rng.next_u64() as u8).collect();
    unsafe { ctest_set_rom(rom.as_ptr(), rom.len()) };

    // A cartridge handle first; the DMA calls below go through it.
    let ctx = context(&mut rng, STACK);
    pair.call(
        "osCartRomInit",
        ref_osCartRomInit,
        pi::osCartRomInit_recomp,
        &ctx,
    );
    pair.call(
        "osDriveRomInit",
        ref_osDriveRomInit,
        pi::osDriveRomInit_recomp,
        &ctx,
    );
    // Handles for SRAM (K1 of 0x08000000) and an unmapped region.
    pair.write(RDRAM_BASE + SRAM_HANDLE + 12, 0xA800_0000);
    pair.write(RDRAM_BASE + UNMAPPED_HANDLE + 12, 0xA500_0000);

    for step in 0..3_000 {
        let mut ctx = context(&mut rng, STACK);
        let dram = DATA + rng.below(DATA_END - DATA - 0x2000) / 8 * 8;
        let size = 2 * (1 + rng.below(0x800)) as u32;
        let rom_offset = rng.below(ROM_SIZE as u64 - u64::from(size)) as u32 & !1;
        match step % 6 {
            0 => {
                ctx.r[reg::A2] = 0; // read
                ctx.r[reg::A3] = u64::from(rom_offset);
                pair.write(ctx.r[reg::SP] + 0x10, (RDRAM_BASE + dram) as u32);
                pair.write(ctx.r[reg::SP] + 0x14, size);
                pair.write(ctx.r[reg::SP] + 0x18, rng.next_u64() as u32);
                pair.call(
                    "osPiStartDma",
                    ref_osPiStartDma,
                    pi::osPiStartDma_recomp,
                    &ctx,
                );
            }
            1..=3 => {
                // ROM read through the cart handle, SRAM read or write, or an unmapped region.
                let (handle, device, write) = match step % 6 {
                    1 => (CART_HANDLE, rom_offset, false),
                    2 => (
                        SRAM_HANDLE,
                        rng.below(0x8000 - u64::from(size)) as u32,
                        rng.below(2) == 1,
                    ),
                    _ => (UNMAPPED_HANDLE, rng.below(0x1000) as u32, rng.below(2) == 1),
                };
                pair.write(RDRAM_BASE + MESG + 4, rng.next_u64() as u32);
                pair.write(RDRAM_BASE + MESG + 8, (RDRAM_BASE + dram) as u32);
                pair.write(RDRAM_BASE + MESG + 12, device);
                pair.write(RDRAM_BASE + MESG + 16, size);
                ctx.r[reg::A0] = RDRAM_BASE + handle;
                ctx.r[reg::A1] = RDRAM_BASE + MESG;
                ctx.r[reg::A2] = u64::from(write);
                pair.call(
                    "osEPiStartDma",
                    ref_osEPiStartDma,
                    pi::osEPiStartDma_recomp,
                    &ctx,
                );
            }
            4 => {
                ctx.r[reg::A0] = RDRAM_BASE + CART_HANDLE;
                // Offset 0 is exactly the ROM base, which the C++ treats as not ROM.
                ctx.r[reg::A1] = if rng.below(10) == 0 {
                    0
                } else {
                    u64::from(rom_offset & !3)
                };
                // The C++ uses the whole register as the destination, so no junk upper half.
                ctx.r[reg::A2] = RDRAM_BASE + (dram & !3);
                pair.call("osEPiReadIo", ref_osEPiReadIo, pi::osEPiReadIo_recomp, &ctx);
            }
            _ => {
                let calls: [(&str, RuntimeFn, RustFn); 4] = [
                    ("osPiGetStatus", ref_osPiGetStatus, pi::osPiGetStatus_recomp),
                    (
                        "__osPiGetAccess",
                        ref___osPiGetAccess,
                        pi::__osPiGetAccess_recomp,
                    ),
                    (
                        "__osPiRelAccess",
                        ref___osPiRelAccess,
                        pi::__osPiRelAccess_recomp,
                    ),
                    (
                        "osCreatePiManager",
                        ref_osCreatePiManager,
                        pi::osCreatePiManager_recomp,
                    ),
                ];
                let (name, reference, rust) = calls[rng.below(4) as usize];
                pair.call(name, reference, rust, &ctx);
            }
        }
    }
    let (_, messages) = backend(true);
    assert!(
        messages.len() > 1000,
        "the transfers sent completion messages"
    );
}
