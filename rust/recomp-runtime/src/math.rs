//! 64-bit integer and conversion helpers the N64 compiler calls (librecomp's
//! `math_routines.cpp`).
//!
//! A 64-bit argument arrives split across two registers, high word first (`$a0:$a1`, then
//! `$a2:$a3`); a 64-bit result goes back in `$v0:$v1`, and a float result in `$f0`.
//!
//! The C++ versions leave some inputs to x86's behavior: a shift count of 64 or more uses
//! its low 6 bits, and a float too large for `int64_t` (or NaN) converts to `INT64_MIN`.
//! These match that. Division by zero, and `INT64_MIN / -1`, trap in C++; here they panic,
//! which also ends the process.

use recomp_abi::{Gpr, RecompContext, recomp_export, reg};

/// The 64-bit argument in `hi:lo`, keeping the low word of each register.
fn pair(hi: Gpr, lo: Gpr) -> u64 {
    (hi << 32) | (lo & 0xFFFF_FFFF)
}

fn first(ctx: &RecompContext) -> u64 {
    pair(ctx.r[reg::A0], ctx.r[reg::A1])
}

fn second(ctx: &RecompContext) -> u64 {
    pair(ctx.r[reg::A2], ctx.r[reg::A3])
}

/// x86's `cvttss2si`: truncation, with `INT64_MIN` for NaN and out-of-range values.
fn f32_to_i64_x86(value: f32) -> i64 {
    // 2^63 is exactly representable; anything at or above it, or below -2^63, is out of range.
    const LIMIT: f32 = 9_223_372_036_854_775_808.0;
    if value.is_nan() || value >= LIMIT || value < -LIMIT {
        i64::MIN
    } else {
        value as i64
    }
}

recomp_export! {
    /// `u64 __udivdi3(u64, u64)`
    fn __udivdi3_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u64(first(ctx) / second(ctx));
    }

    /// `s64 __divdi3(s64, s64)`
    fn __divdi3_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u64((first(ctx) as i64 / second(ctx) as i64) as u64);
    }

    /// `u64 __umoddi3(u64, u64)`
    fn __umoddi3_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u64(first(ctx) % second(ctx));
    }

    /// `u64 __ull_div(u64, u64)`
    fn __ull_div_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u64(first(ctx) / second(ctx));
    }

    /// `s64 __ll_div(s64, s64)`
    fn __ll_div_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u64((first(ctx) as i64 / second(ctx) as i64) as u64);
    }

    /// `u64 __ll_mul(u64, u64)`
    fn __ll_mul_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u64(first(ctx).wrapping_mul(second(ctx)));
    }

    /// `u64 __ull_rem(u64, u64)`
    fn __ull_rem_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u64(first(ctx) % second(ctx));
    }

    /// `double __ull_to_d(u64)`
    fn __ull_to_d_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.f[0].set_d(first(ctx) as f64);
    }

    /// `float __ull_to_f(u64)`
    fn __ull_to_f_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_f32(first(ctx) as f32);
    }

    /// `u64 __ull_rshift(u64, u64)`
    fn __ull_rshift_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u64(first(ctx).wrapping_shr(second(ctx) as u32));
    }

    /// `float __ll_to_f(s64)`
    fn __ll_to_f_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_f32(first(ctx) as i64 as f32);
    }

    /// `s64 __f_to_ll(float)`, the argument in `$f12`.
    fn __f_to_ll_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u64(f32_to_i64_x86(ctx.f[12].fl()) as u64);
    }

    /// `u64 __ll_lshift(u64, u64)`
    fn __ll_lshift_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u64(first(ctx).wrapping_shl(second(ctx) as u32));
    }
}
