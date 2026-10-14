//! Running a Rust runtime function and its C++ reference side by side: each on its own copy
//! of guest memory and its own stand-in save buffer, compared after every call.

use std::sync::atomic::{AtomicU32, Ordering};

use recomp_abi::{GuestMemory, RDRAM_BASE, RecompContext, install_guest_memory, reg};

use crate::{RuntimeFn, backend};

/// A Rust runtime export.
pub type RustFn = extern "C" fn(*mut u8, *mut RecompContext);

/// xorshift64*
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

/// A copy of `ctx` (the context holds a raw pointer, so it isn't `Clone`).
pub fn copy(ctx: &RecompContext) -> RecompContext {
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

/// Random registers, with `$sp` at `stack` bytes into guest memory.
pub fn context(rng: &mut Rng, stack: u64) -> RecompContext {
    let mut ctx = RecompContext::default();
    for r in &mut ctx.r[1..] {
        *r = rng.next_u64();
    }
    ctx.r[reg::SP] = RDRAM_BASE + stack;
    ctx
}

/// A guest pointer (sign-extended KSEG0) to `offset`, sometimes with junk in the upper half,
/// which C++ `PTR()` arguments drop.
pub fn pointer(rng: &mut Rng, offset: u64) -> u64 {
    let address = RDRAM_BASE + offset;
    if rng.below(4) == 0 {
        (address & 0xFFFF_FFFF) | (rng.next_u64() << 32)
    } else {
        address
    }
}

fn snapshot(words: &[AtomicU32]) -> Vec<u32> {
    words.iter().map(|w| w.load(Ordering::Relaxed)).collect()
}

/// Starts the Rust save backend (with its saving thread) in a scratch directory, sized like
/// the C++ stand-in's buffer.
pub fn start_rust_saves() {
    let folder = std::env::temp_dir().join(format!("recomp-runtime-ctest-{}", std::process::id()));
    recomp_runtime::saving::init(
        folder,
        "ctest",
        crate::SAVE_BUFFER_SIZE,
        recomp_runtime::saving::Host {
            exited: || false,
            report: |message| panic!("save failed: {message}"),
        },
    );
}

/// Two copies of the same random guest memory: the reference's, and the Rust side's, which is
/// installed as the process's guest memory (so one `Pair` per test binary).
pub struct Pair {
    reference: &'static [AtomicU32],
    rust: &'static GuestMemory<'static>,
    rust_words: &'static [AtomicU32],
}

impl Pair {
    pub fn new(rng: &mut Rng, words: usize) -> Self {
        Self::with_reserve(rng, words, words)
    }

    /// Like [`Self::new`], with only the first `rdram_words` as RDRAM proper on the Rust side
    /// and the rest standing in for the reserved range around it (TLB-mapped pages). The C++
    /// reference indexes it all as plain memory, as it does in the runtime.
    pub fn with_reserve(rng: &mut Rng, words: usize, rdram_words: usize) -> Self {
        let reference: &'static [AtomicU32] =
            Vec::from_iter((0..words).map(|_| AtomicU32::new(rng.next_u64() as u32))).leak();
        let rust_words: &'static [AtomicU32] = Vec::from_iter(
            reference
                .iter()
                .map(|w| AtomicU32::new(w.load(Ordering::Relaxed))),
        )
        .leak();
        assert!(install_guest_memory(GuestMemory::with_reserve(
            rust_words,
            rdram_words
        )));
        start_rust_saves();
        Self {
            reference,
            rust: recomp_abi::guest_memory().unwrap(),
            rust_words,
        }
    }

    /// Writes a word into both memories.
    pub fn write(&self, addr: u64, value: u32) {
        GuestMemory::new(self.reference).write_u32(addr, value);
        self.rust.write_u32(addr, value);
    }

    /// Runs `reference` and `rust` from the same registers, then requires the same registers,
    /// guest memory, save bytes and `osSendMesg` calls.
    pub fn call(&self, name: &str, reference: RuntimeFn, rust: RustFn, ctx: &RecompContext) {
        let mut c = copy(ctx);
        let mut r = copy(ctx);
        // SAFETY: the reference takes its own guest memory and a valid context.
        unsafe { reference(self.reference.as_ptr().cast_mut().cast(), &mut c) };
        rust(self.rust.as_ptr(), &mut r);
        assert_eq!(r.r, c.r, "{name}: registers from {:x?}", &ctx.r[4..8]);
        assert!(
            snapshot(self.rust_words) == snapshot(self.reference),
            "{name}: guest memory from {:x?}",
            &ctx.r[4..8]
        );
        let (c_save, c_messages) = backend(false);
        let (_, r_messages) = backend(true);
        let r_save = recomp_runtime::saving::saves().snapshot();
        assert!(
            r_save == c_save,
            "{name}: save bytes from {:x?}",
            &ctx.r[4..8]
        );
        assert_eq!(r_messages, c_messages, "{name}: osSendMesg calls");
    }
}
