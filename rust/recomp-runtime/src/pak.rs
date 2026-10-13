//! Controller Pak file system (librecomp's `pak.cpp`): no pak is inserted.

use recomp_abi::{recomp_export, reg};

/// libultra's `PFS_ERR_NOPACK`.
const PFS_ERR_NOPACK: u32 = 1;

recomp_export! {
    fn osPfsInitPak_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(PFS_ERR_NOPACK);
    }

    fn osPfsFreeBlocks_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(PFS_ERR_NOPACK);
    }

    fn osPfsAllocateFile_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(PFS_ERR_NOPACK);
    }

    fn osPfsDeleteFile_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(PFS_ERR_NOPACK);
    }

    fn osPfsFileState_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(PFS_ERR_NOPACK);
    }

    fn osPfsFindFile_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(PFS_ERR_NOPACK);
    }

    fn osPfsReadWriteFile_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(PFS_ERR_NOPACK);
    }

    fn osPfsChecker_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(PFS_ERR_NOPACK);
    }

    /// `s32 osPfsNumFiles(OSPfs*, s32* max_files, s32* files_used)`: zero of zero.
    fn osPfsNumFiles_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        // The C++ `_arg` uses the whole register as the pointer, unlike the `PTR()` types.
        mem.write_u32(ctx.r[reg::A1], 0);
        mem.write_u32(ctx.r[reg::A2], 0);
        ctx.set_return_u32(PFS_ERR_NOPACK);
    }

    fn osPfsRepairId_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(PFS_ERR_NOPACK);
    }
}
