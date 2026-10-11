//! Round trip: Rust calls "recompiled" C code through GameFn, and that C code calls a Rust
//! runtime function defined with recomp_export!.

use std::sync::atomic::AtomicU32;

use recomp_abi::{install_guest_memory, recomp_export, reg, GameFn, GuestMemory, RecompContext, RDRAM_BASE};
use recomp_abi_ctest::ctest_game_func;

recomp_export! {
    /// Doubles the word at $a0 + 4 and returns the old word at $a0 + 4 in $v0.
    fn test_double_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        let addr = ctx.r[reg::A0] + 4;
        let old = mem.read_u32(addr);
        mem.write_u32(addr, old.wrapping_mul(2));
        ctx.set_return_u32(old);
    }
}

#[test]
fn c_and_rust_call_each_other() {
    let words: &'static [AtomicU32] = Vec::from_iter((0..64).map(|_| AtomicU32::new(0))).leak();
    assert!(install_guest_memory(GuestMemory::new(words)));
    let mem = recomp_abi::guest_memory().unwrap();
    mem.write_u32(RDRAM_BASE + 0x14, 21);

    let mut ctx = RecompContext::default();
    ctx.r[reg::A0] = RDRAM_BASE + 0x10;
    ctx.r[reg::A1] = 0xCAFE;
    let game = unsafe { GameFn::new(ctest_game_func) };
    game.call(mem, &mut ctx);

    assert_eq!(mem.read_u32(RDRAM_BASE + 0x10), 0xCAFE, "C store");
    assert_eq!(mem.read_u32(RDRAM_BASE + 0x14), 42, "Rust export doubled the word");
    assert_eq!(ctx.r[reg::V0], 21, "Rust export's return value");
    assert_eq!(ctx.r[reg::V1], 22, "C used the return value after the call");
}
