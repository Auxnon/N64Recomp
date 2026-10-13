//! Each Rust runtime function against its C++ original (`c/runtime_reference.cpp`): the same
//! starting registers through both, and identical registers afterwards.

use std::sync::OnceLock;
use std::sync::atomic::AtomicU32;

use recomp_abi::{GuestMemory, RecompContext, install_guest_memory, reg};
use recomp_runtime::{math, print, stubs};
use recomp_runtime_ctest::*;

type RustFn = extern "C" fn(*mut u8, *mut RecompContext);

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn rdram() -> *mut u8 {
    static MEMORY: OnceLock<usize> = OnceLock::new();
    *MEMORY.get_or_init(|| {
        let words: &'static [AtomicU32] = Vec::from_iter((0..64).map(|_| AtomicU32::new(0))).leak();
        let mem = GuestMemory::new(words);
        let ptr = mem.as_ptr() as usize;
        assert!(install_guest_memory(mem));
        ptr
    }) as *mut u8
}

/// Values that exercise sign extension, word boundaries and shift counts past 63.
const EDGES: [u64; 12] = [
    0,
    1,
    2,
    31,
    32,
    63,
    64,
    127,
    0x7FFF_FFFF,
    0x8000_0000,
    0xFFFF_FFFF,
    0xFFFF_FFFF_FFFF_FFFF,
];

/// A copy of `ctx` (the context holds a raw pointer, so it isn't `Clone`).
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

fn compare(name: &str, reference: RuntimeFn, rust: RustFn, ctx: &RecompContext) {
    let mut c = copy(ctx);
    let mut r = copy(ctx);
    unsafe { reference(rdram(), &mut c) };
    rust(rdram(), &mut r);
    assert_eq!(
        r.r,
        c.r,
        "{name}: registers, starting from {:x?}",
        &ctx.r[..8]
    );
    assert_eq!(
        r.f, c.f,
        "{name}: float registers, starting from {:x?}",
        ctx.f[12]
    );
    assert_eq!((r.hi, r.lo), (c.hi, c.lo), "{name}: hi/lo");
}

/// `$a0`-`$a3` set to `args`, other registers random.
fn context(rng: &mut Rng, args: [u64; 4]) -> RecompContext {
    let mut ctx = RecompContext::default();
    for r in &mut ctx.r {
        *r = rng.next();
    }
    ctx.r[0] = 0;
    ctx.r[reg::A0..=reg::A3].copy_from_slice(&args);
    for f in &mut ctx.f {
        f.0 = rng.next();
    }
    ctx
}

#[test]
fn math_routines_match_the_cpp() {
    let integer: [(&str, RuntimeFn, RustFn); 9] = [
        ("__udivdi3", ref___udivdi3, math::__udivdi3_recomp),
        ("__divdi3", ref___divdi3, math::__divdi3_recomp),
        ("__umoddi3", ref___umoddi3, math::__umoddi3_recomp),
        ("__ull_div", ref___ull_div, math::__ull_div_recomp),
        ("__ll_div", ref___ll_div, math::__ll_div_recomp),
        ("__ll_mul", ref___ll_mul, math::__ll_mul_recomp),
        ("__ull_rem", ref___ull_rem, math::__ull_rem_recomp),
        ("__ull_rshift", ref___ull_rshift, math::__ull_rshift_recomp),
        ("__ll_lshift", ref___ll_lshift, math::__ll_lshift_recomp),
    ];
    let unary: [(&str, RuntimeFn, RustFn); 3] = [
        ("__ull_to_d", ref___ull_to_d, math::__ull_to_d_recomp),
        ("__ull_to_f", ref___ull_to_f, math::__ull_to_f_recomp),
        ("__ll_to_f", ref___ll_to_f, math::__ll_to_f_recomp),
    ];
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for step in 0..200_000 {
        // Mostly random registers, sometimes edge values in each argument.
        let pick = |rng: &mut Rng| {
            if rng.next() % 4 == 0 {
                EDGES[(rng.next() % EDGES.len() as u64) as usize]
            } else {
                rng.next()
            }
        };
        let args = [
            pick(&mut rng),
            pick(&mut rng),
            pick(&mut rng),
            pick(&mut rng),
        ];
        let a = (args[0] << 32) | (args[1] & 0xFFFF_FFFF);
        let b = (args[2] << 32) | (args[3] & 0xFFFF_FFFF);
        let ctx = context(&mut rng, args);
        let (name, reference, rust) = integer[step % integer.len()];
        let divides = name.contains("div") || name.contains("mod") || name.contains("rem");
        // Division by zero and INT64_MIN / -1 trap in both; skip them.
        let traps = b == 0
            || (name.contains("ll_div") || name == "__divdi3") && a == 1 << 63 && b == u64::MAX;
        if !(divides && traps) {
            compare(name, reference, rust, &ctx);
        }
        let (name, reference, rust) = unary[step % unary.len()];
        compare(name, reference, rust, &ctx);
    }
}

#[test]
fn float_to_long_matches_x86() {
    let mut rng = Rng(7);
    let specials = [
        0.0f32,
        -0.0,
        1.5,
        -1.5,
        9.223_372e18,
        -9.223_372e18,
        1e19,
        -1e19,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        f32::MAX,
        f32::MIN,
    ];
    for step in 0..100_000 {
        let mut ctx = context(&mut rng, [0; 4]);
        if step < specials.len() {
            ctx.f[12].set_fl(specials[step]);
        }
        compare("__f_to_ll", ref___f_to_ll, math::__f_to_ll_recomp, &ctx);
    }
}

#[test]
fn print_and_stub_returns_match_the_cpp() {
    let functions: [(&str, RuntimeFn, RustFn); 5] = [
        (
            "__checkHardware_msp",
            ref___checkHardware_msp,
            print::__checkHardware_msp_recomp,
        ),
        (
            "__checkHardware_kmc",
            ref___checkHardware_kmc,
            print::__checkHardware_kmc_recomp,
        ),
        (
            "__checkHardware_isv",
            ref___checkHardware_isv,
            print::__checkHardware_isv_recomp,
        ),
        (
            "is_proutSyncPrintf",
            ref_is_proutSyncPrintf,
            print::is_proutSyncPrintf_recomp,
        ),
        ("osVoiceInit", ref_osVoiceInit, stubs::osVoiceInit_recomp),
    ];
    let mut rng = Rng(3);
    for (name, reference, rust) in functions {
        for _ in 0..100 {
            let args = [rng.next(), rng.next(), rng.next(), rng.next()];
            compare(name, reference, rust, &context(&mut rng, args));
        }
    }
}
