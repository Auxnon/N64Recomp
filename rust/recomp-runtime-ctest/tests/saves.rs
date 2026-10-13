//! The Rust save commands (EEPROM, FlashRAM, Controller Pak) against librecomp's C++, each on
//! its own copy of guest memory and its own stand-in save buffer (`c/save_backend.cpp`). A
//! random sequence of calls runs through both; after every call the save bytes, guest memory,
//! registers and `osSendMesg` log must match.

use recomp_abi::{RDRAM_BASE, reg};
use recomp_runtime::{eep, flash, pak};
use recomp_runtime_ctest::harness::{Pair, Rng, RustFn, pointer};
use recomp_runtime_ctest::*;

const WORDS: usize = 16 * 1024; // 64 KB of guest memory
/// Where the stack arguments of `osFlashReadArray` go.
const STACK: u64 = 0x100;
/// Buffers are drawn from above this offset.
const DATA: u64 = 0x1000;

fn context(rng: &mut Rng) -> recomp_abi::RecompContext {
    recomp_runtime_ctest::harness::context(rng, STACK)
}

#[test]
fn save_commands_match_the_cpp() {
    let mut rng = Rng(0xC0FF_EE11);
    let pair = Pair::new(&mut rng, WORDS);
    unsafe { ctest_set_save_type(5) }; // AllowAll
    let data_end = WORDS as u64 * 4 - 4 * 128;
    for step in 0..20_000 {
        let mut ctx = context(&mut rng);
        let buffer = DATA + rng.below(data_end - DATA) / 4 * 4;
        let second = DATA + rng.below(data_end - DATA) / 4 * 4;
        match step % 16 {
            0 => {
                ctx.r[reg::A1] = rng.next(); // only the low byte is the block
                ctx.r[reg::A2] = pointer(&mut rng, buffer);
                if rng.below(2) == 0 {
                    pair.call(
                        "osEepromWrite",
                        ref_osEepromWrite,
                        eep::osEepromWrite_recomp,
                        &ctx,
                    );
                } else {
                    pair.call(
                        "osEepromRead",
                        ref_osEepromRead,
                        eep::osEepromRead_recomp,
                        &ctx,
                    );
                }
            }
            1 => {
                ctx.r[reg::A1] = rng.next();
                ctx.r[reg::A2] = pointer(&mut rng, buffer);
                ctx.r[reg::A3] = 8 * (1 + rng.below(32));
                if rng.below(2) == 0 {
                    pair.call(
                        "osEepromLongWrite",
                        ref_osEepromLongWrite,
                        eep::osEepromLongWrite_recomp,
                        &ctx,
                    );
                } else {
                    pair.call(
                        "osEepromLongRead",
                        ref_osEepromLongRead,
                        eep::osEepromLongRead_recomp,
                        &ctx,
                    );
                }
            }
            2 => {
                ctx.r[reg::A2] = pointer(&mut rng, buffer);
                ctx.r[reg::A3] = rng.next();
                pair.call(
                    "osFlashWriteBuffer",
                    ref_osFlashWriteBuffer,
                    flash::osFlashWriteBuffer_recomp,
                    &ctx,
                );
            }
            3 => {
                ctx.r[reg::A0] = rng.below(1024) | (rng.next() << 32);
                pair.call(
                    "osFlashWriteArray",
                    ref_osFlashWriteArray,
                    flash::osFlashWriteArray_recomp,
                    &ctx,
                );
            }
            4 => {
                let pages = 1 + rng.below(4);
                ctx.r[reg::A2] = rng.below(1024 - pages);
                ctx.r[reg::A3] = pointer(&mut rng, buffer);
                pair.write(ctx.r[reg::SP] + 0x10, pages as u32);
                pair.write(ctx.r[reg::SP] + 0x14, rng.next() as u32);
                pair.call(
                    "osFlashReadArray",
                    ref_osFlashReadArray,
                    flash::osFlashReadArray_recomp,
                    &ctx,
                );
            }
            5 => {
                // Pages past the end are refused.
                ctx.r[reg::A0] = rng.below(1100);
                if rng.below(2) == 0 {
                    pair.call(
                        "osFlashSectorErase",
                        ref_osFlashSectorErase,
                        flash::osFlashSectorErase_recomp,
                        &ctx,
                    );
                } else {
                    pair.call(
                        "osFlashSectorEraseThrough",
                        ref_osFlashSectorEraseThrough,
                        flash::osFlashSectorEraseThrough_recomp,
                        &ctx,
                    );
                }
            }
            6 if rng.below(50) == 0 => {
                if rng.below(2) == 0 {
                    pair.call(
                        "osFlashAllErase",
                        ref_osFlashAllErase,
                        flash::osFlashAllErase_recomp,
                        &ctx,
                    );
                } else {
                    pair.call(
                        "osFlashAllEraseThrough",
                        ref_osFlashAllEraseThrough,
                        flash::osFlashAllEraseThrough_recomp,
                        &ctx,
                    );
                }
            }
            7 => {
                ctx.r[reg::A0] = pointer(&mut rng, buffer);
                pair.call(
                    "osFlashReadStatus",
                    ref_osFlashReadStatus,
                    flash::osFlashReadStatus_recomp,
                    &ctx,
                );
            }
            8 => {
                ctx.r[reg::A0] = pointer(&mut rng, buffer);
                ctx.r[reg::A1] = pointer(&mut rng, second);
                pair.call(
                    "osFlashReadId",
                    ref_osFlashReadId,
                    flash::osFlashReadId_recomp,
                    &ctx,
                );
            }
            9 => {
                let calls: [(&str, RuntimeFn, RustFn); 4] = [
                    ("osFlashInit", ref_osFlashInit, flash::osFlashInit_recomp),
                    (
                        "osFlashClearStatus",
                        ref_osFlashClearStatus,
                        flash::osFlashClearStatus_recomp,
                    ),
                    (
                        "osFlashCheckEraseEnd",
                        ref_osFlashCheckEraseEnd,
                        flash::osFlashCheckEraseEnd_recomp,
                    ),
                    (
                        "osFlashChange",
                        ref_osFlashChange,
                        flash::osFlashChange_recomp,
                    ),
                ];
                let (name, reference, rust) = calls[rng.below(4) as usize];
                pair.call(name, reference, rust, &ctx);
            }
            10 => {
                // `_arg` uses the whole register, so junk in the upper half crashes the C++.
                ctx.r[reg::A1] = RDRAM_BASE + buffer;
                ctx.r[reg::A2] = RDRAM_BASE + second;
                pair.call(
                    "osPfsNumFiles",
                    ref_osPfsNumFiles,
                    pak::osPfsNumFiles_recomp,
                    &ctx,
                );
            }
            11 => {
                let calls: [(&str, RuntimeFn, RustFn); 9] = [
                    ("osPfsInitPak", ref_osPfsInitPak, pak::osPfsInitPak_recomp),
                    (
                        "osPfsFreeBlocks",
                        ref_osPfsFreeBlocks,
                        pak::osPfsFreeBlocks_recomp,
                    ),
                    (
                        "osPfsAllocateFile",
                        ref_osPfsAllocateFile,
                        pak::osPfsAllocateFile_recomp,
                    ),
                    (
                        "osPfsDeleteFile",
                        ref_osPfsDeleteFile,
                        pak::osPfsDeleteFile_recomp,
                    ),
                    (
                        "osPfsFileState",
                        ref_osPfsFileState,
                        pak::osPfsFileState_recomp,
                    ),
                    (
                        "osPfsFindFile",
                        ref_osPfsFindFile,
                        pak::osPfsFindFile_recomp,
                    ),
                    (
                        "osPfsReadWriteFile",
                        ref_osPfsReadWriteFile,
                        pak::osPfsReadWriteFile_recomp,
                    ),
                    ("osPfsChecker", ref_osPfsChecker, pak::osPfsChecker_recomp),
                    (
                        "osPfsRepairId",
                        ref_osPfsRepairId,
                        pak::osPfsRepairId_recomp,
                    ),
                ];
                let (name, reference, rust) = calls[rng.below(9) as usize];
                pair.call(name, reference, rust, &ctx);
            }
            _ => {
                // Probe under each save type, then back to AllowAll for the rest.
                for save_type in 0..6 {
                    unsafe { ctest_set_save_type(save_type) };
                    pair.call(
                        "osEepromProbe",
                        ref_osEepromProbe,
                        eep::osEepromProbe_recomp,
                        &ctx,
                    );
                }
                unsafe { ctest_set_save_type(5) };
            }
        }
    }
    // The sequence did write and erase something.
    let (save, messages) = backend(true);
    assert!(save.iter().any(|&b| b != 0 && b != 0xFF));
    assert!(!messages.is_empty());
}
