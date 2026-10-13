//! Debug output and development-hardware probes (librecomp's `print.cpp`).
//!
//! libultra checks for development boards (the MSP, KMC and IS-Viewer) at boot; none are
//! present. `__osRdbSend` is the remote-debugger channel that `osSyncPrintf` writes to, and
//! goes to stdout. The IS-Viewer printer reports success and drops the text, as in C++.
//!
//! librecomp's `euc-jp.cpp` decoder was only used by commented-out code here, so it isn't
//! ported.

use std::io::Write;

use recomp_abi::{recomp_export, reg};

recomp_export! {
    fn __checkHardware_msp_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(0);
    }

    fn __checkHardware_kmc_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(0);
    }

    fn __checkHardware_isv_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(0);
    }

    fn __osInitialize_msp_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    fn __osInitialize_kmc_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    fn __osInitialize_isv_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    fn isPrintfInit_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    /// `u32 __osRdbSend(u8* buf, u32 size, u32 type)`: writes the bytes to stdout.
    fn __osRdbSend_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        let buf = ctx.r[reg::A0];
        let size = ctx.r[reg::A1];
        let text: Vec<u8> = (0..size).map(|i| mem.read_u8(buf.wrapping_add(i))).collect();
        let _ = std::io::stdout().write_all(&text);
        // The C++ stores the 64-bit size_t whole.
        ctx.r[reg::V0] = size;
    }

    /// The IS-Viewer printer callback: reports success without printing.
    fn is_proutSyncPrintf_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(1);
    }
}
