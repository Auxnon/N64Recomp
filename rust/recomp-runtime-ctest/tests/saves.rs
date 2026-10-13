//! The Rust save commands (EEPROM, FlashRAM, Controller Pak) against librecomp's C++, each on
//! its own copy of guest memory and its own stand-in save buffer (`c/save_backend.cpp`). A
//! random sequence of calls runs through both; after every call the save bytes, guest memory,
//! registers and `osSendMesg` log must match.

use std::sync::atomic::{AtomicU32, Ordering};

use recomp_abi::{GuestMemory, RDRAM_BASE, RecompContext, install_guest_memory, reg};
use recomp_runtime::{eep, flash, pak};
use recomp_runtime_ctest::*;

type RustFn = extern "C" fn(*mut u8, *mut RecompContext);

const WORDS: usize = 16 * 1024; // 64 KB of guest memory
/// Where the stack arguments of `osFlashReadArray` go.
const STACK: u64 = 0x100;
/// Buffers are drawn from above this offset.
const DATA: u64 = 0x1000;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn copy(ctx: &RecompContext) -> RecompContext {
    RecompContext {
        r: ctx.r,
        f: ctx.f,
        hi: ctx.hi,
        lo: ctx.lo,
        f_odd: ctx.f_odd,
        status_reg: ctx.status_reg,
        mips3_float_mode: ctx.mips3_float_mode,
    }
}

fn snapshot(words: &[AtomicU32]) -> Vec<u32> {
    words.iter().map(|w| w.load(Ordering::Relaxed)).collect()
}

struct Pair {
    reference: &'static [AtomicU32],
    rust: &'static GuestMemory<'static>,
    rust_words: &'static [AtomicU32],
}

impl Pair {
    fn new(rng: &mut Rng) -> Self {
        let fill = |rng: &mut Rng| -> &'static [AtomicU32] {
            Vec::from_iter((0..WORDS).map(|_| AtomicU32::new(rng.next() as u32))).leak()
        };
        let mut seed = Rng(rng.next() | 1);
        let reference = fill(&mut Rng(seed.next() | 1));
        let rust_words: &'static [AtomicU32] = Vec::from_iter(
            reference
                .iter()
                .map(|w| AtomicU32::new(w.load(Ordering::Relaxed))),
        )
        .leak();
        let _ = fill(&mut seed);
        assert!(install_guest_memory(GuestMemory::new(rust_words)));
        Self {
            reference,
            rust: recomp_abi::guest_memory().unwrap(),
            rust_words,
        }
    }

    /// Writes a word into both memories.
    fn write(&self, addr: u64, value: u32) {
        GuestMemory::new(self.reference).write_u32(addr, value);
        self.rust.write_u32(addr, value);
    }

    fn call(&self, name: &str, reference: RuntimeFn, rust: RustFn, ctx: &RecompContext) {
        let mut c = copy(ctx);
        let mut r = copy(ctx);
        unsafe { reference(self.reference.as_ptr().cast_mut().cast(), &mut c) };
        rust(self.rust.as_ptr(), &mut r);
        assert_eq!(r.r, c.r, "{name}: registers from {:x?}", &ctx.r[4..8]);
        assert!(
            snapshot(self.rust_words) == snapshot(self.reference),
            "{name}: guest memory from {:x?}",
            &ctx.r[4..8]
        );
        let (c_save, c_messages) = backend(false);
        let (r_save, r_messages) = backend(true);
        assert!(
            r_save == c_save,
            "{name}: save bytes from {:x?}",
            &ctx.r[4..8]
        );
        assert_eq!(r_messages, c_messages, "{name}: osSendMesg calls");
    }
}

/// A guest pointer (sign-extended KSEG0) to `offset`, sometimes with junk in the upper half
/// that the C++ drops.
fn pointer(rng: &mut Rng, offset: u64) -> u64 {
    let address = RDRAM_BASE + offset;
    if rng.below(4) == 0 {
        (address & 0xFFFF_FFFF) | (rng.next() << 32)
    } else {
        address
    }
}

fn context(rng: &mut Rng) -> RecompContext {
    let mut ctx = RecompContext::default();
    for r in &mut ctx.r[1..] {
        *r = rng.next();
    }
    ctx.r[reg::SP] = RDRAM_BASE + STACK;
    ctx
}

#[test]
fn save_commands_match_the_cpp() {
    let mut rng = Rng(0xC0FF_EE11);
    let pair = Pair::new(&mut rng);
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
