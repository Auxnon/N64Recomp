//! libultra functions with nothing to emulate (librecomp's `ultra_stubs.cpp`).
//!
//! There's no Voice Recognition Unit: `osVoiceInit` reports no device, so a game never gets
//! as far as the other `osVoice*` calls. Those `assert(false)` in C++, which compiles to
//! nothing in the release builds games ship, so here they do nothing too.

use recomp_abi::recomp_export;

/// libultra's `CONT_ERR_DEVICE`: no device of the requested kind.
const CONT_ERR_DEVICE: u32 = 11;

recomp_export! {
    fn osVoiceInit_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(CONT_ERR_DEVICE);
    }

    fn osVoiceSetWord_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    fn osVoiceCheckWord_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    fn osVoiceStopReadData_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    fn osVoiceMaskDictionary_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    fn osVoiceStartReadData_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    fn osVoiceControlGain_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    fn osVoiceGetReadData_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    fn osVoiceClearDictionary_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}
}

#[cfg(feature = "tlb-stub")]
recomp_export! {
    /// Games that don't use the TLB never map anything, so there's nothing to unmap. A game
    /// that emulates the TLB (Gauntlet Legends) defines its own and builds this crate without
    /// the `tlb-stub` feature.
    fn osUnmapTLBAll_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}
}
