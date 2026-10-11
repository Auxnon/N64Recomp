//! libultra functions from librecomp's `recomp.cpp`.

use recomp_abi::recomp_export;

/// The RDRAM size reported to the game: 8 MB, i.e. an Expansion Pak is installed.
pub const REPORTED_MEM_SIZE: u32 = 8 * 1024 * 1024;

recomp_export! {
    /// `u32 osGetMemSize(void)`.
    fn osGetMemSize_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(REPORTED_MEM_SIZE);
    }
}
