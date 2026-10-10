//! Bindings to `c/reference.c`, the C reference implementation built from `include/recomp.h`.
//! Test-only: this crate's unsafe code is outside the port's unsafe budget.

use recomp_abi::{Gpr, RecompContext};

unsafe extern "C" {
    pub fn ctest_layout(out: *mut [usize; 13]);

    pub fn ctest_read_w(rdram: *mut u8, addr: Gpr) -> u32;
    pub fn ctest_read_hu(rdram: *mut u8, addr: Gpr) -> u16;
    pub fn ctest_read_h(rdram: *mut u8, addr: Gpr) -> i16;
    pub fn ctest_read_bu(rdram: *mut u8, addr: Gpr) -> u8;
    pub fn ctest_read_b(rdram: *mut u8, addr: Gpr) -> i8;
    pub fn ctest_read_d(rdram: *mut u8, addr: Gpr) -> u64;

    pub fn ctest_write_w(rdram: *mut u8, addr: Gpr, v: u32);
    pub fn ctest_write_h(rdram: *mut u8, addr: Gpr, v: u16);
    pub fn ctest_write_b(rdram: *mut u8, addr: Gpr, v: u8);
    pub fn ctest_write_d(rdram: *mut u8, addr: Gpr, v: u64);

    pub fn ctest_lwl(rdram: *mut u8, initial: Gpr, addr: Gpr) -> Gpr;
    pub fn ctest_lwr(rdram: *mut u8, initial: Gpr, addr: Gpr) -> Gpr;
    pub fn ctest_ldl(rdram: *mut u8, initial: Gpr, addr: Gpr) -> Gpr;
    pub fn ctest_ldr(rdram: *mut u8, initial: Gpr, addr: Gpr) -> Gpr;
    pub fn ctest_swl(rdram: *mut u8, addr: Gpr, v: Gpr);
    pub fn ctest_swr(rdram: *mut u8, addr: Gpr, v: Gpr);
    pub fn ctest_sdl(rdram: *mut u8, addr: Gpr, v: Gpr);
    pub fn ctest_sdr(rdram: *mut u8, addr: Gpr, v: Gpr);

    pub fn ctest_game_func(rdram: *mut u8, ctx: *mut RecompContext);
}
