//! FlashRAM saves (librecomp's `flash.cpp`), through librecomp's save buffer.
//!
//! A 1 Mbit chip written a 128-byte page at a time: `osFlashWriteBuffer` fills the chip's
//! page buffer, then `osFlashWriteArray` commits it. Erases and writes finish at once, so the
//! status calls always report success.

use std::sync::Mutex;

use recomp_abi::{recomp_export, reg, RecompContext};

use crate::cxx;
use crate::saving::saves;

const FLASH_SIZE: u32 = 1024 * 1024 / 8;
const PAGE_SIZE: u32 = 128;
const PAGE_COUNT: u32 = FLASH_SIZE / PAGE_SIZE;
/// N64ModernRuntime's `ultramodern::save_size`, the largest save (flash).
const SAVE_SIZE: u32 = FLASH_SIZE;
/// The type and maker IDs of a real chip; some games check for them.
const FLASH_TYPE: u32 = 0x1111_8001;
const FLASH_MAKER: u32 = 0x00C2_001E;

/// The chip's page buffer, between `osFlashWriteBuffer` and `osFlashWriteArray`.
static WRITE_BUFFER: Mutex<[u8; PAGE_SIZE as usize]> = Mutex::new([0; PAGE_SIZE as usize]);

/// A register holding a guest pointer, as the C++ `PTR()` types read it: the low 32 bits,
/// sign-extended.
fn guest_pointer(value: u64) -> u64 {
    value as i32 as i64 as u64
}

fn require_flash() {
    if !cxx::flashram_allowed() {
        cxx::fatal_error(c"Attempted to use FlashRAM saving with other save type");
    }
}

fn erase_all(ctx: &mut RecompContext) {
    require_flash();
    saves().clear(0, SAVE_SIZE, 0xFF);
    ctx.set_return_u32(0);
}

/// Erases the page in `$a0` (libultra calls it a sector).
fn erase_page(ctx: &mut RecompContext) {
    require_flash();
    let page = ctx.r[reg::A0] as u32;
    if page >= PAGE_COUNT {
        ctx.set_return_i32(-1);
        return;
    }
    saves().clear(page * PAGE_SIZE, PAGE_SIZE, 0xFF);
    ctx.set_return_u32(0);
}

recomp_export! {
    /// `OSPiHandle* osFlashInit(void)`
    fn osFlashInit_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        require_flash();
        ctx.set_return_i32(cxx::flash_handle());
    }

    /// `void osFlashReadStatus(u8* status)`
    fn osFlashReadStatus_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        require_flash();
        mem.write_u8(guest_pointer(ctx.r[reg::A0]), 0);
    }

    /// `void osFlashReadId(u32* type, u32* maker)`
    fn osFlashReadId_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        require_flash();
        mem.write_u32(guest_pointer(ctx.r[reg::A0]), FLASH_TYPE);
        mem.write_u32(guest_pointer(ctx.r[reg::A1]), FLASH_MAKER);
    }

    /// `void osFlashClearStatus(void)`
    fn osFlashClearStatus_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {
        require_flash();
    }

    /// `s32 osFlashAllErase(void)`
    fn osFlashAllErase_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        erase_all(ctx);
    }

    /// `void osFlashAllEraseThrough(void)`
    fn osFlashAllEraseThrough_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        erase_all(ctx);
    }

    /// `s32 osFlashSectorErase(u32 page)`
    fn osFlashSectorErase_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        erase_page(ctx);
    }

    /// `void osFlashSectorEraseThrough(u32 page)`
    fn osFlashSectorEraseThrough_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        erase_page(ctx);
    }

    /// `s32 osFlashCheckEraseEnd(void)`: erases finish at once.
    fn osFlashCheckEraseEnd_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        require_flash();
        ctx.set_return_u32(0); // FLASH_STATUS_ERASE_OK
    }

    /// `s32 osFlashWriteBuffer(OSIoMesg*, s32 priority, void* dram, OSMesgQueue*)`
    fn osFlashWriteBuffer_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        require_flash();
        let dram = guest_pointer(ctx.r[reg::A2]);
        let mq = ctx.r[reg::A3] as i32;
        {
            let mut buffer = WRITE_BUFFER.lock().unwrap_or_else(|e| e.into_inner());
            for (i, byte) in buffer.iter_mut().enumerate() {
                *byte = mem.read_u8(dram.wrapping_add(i as u64));
            }
        }
        cxx::send_mesg(mem, mq, 0, cxx::OS_MESG_NOBLOCK);
        ctx.set_return_u32(0);
    }

    /// `s32 osFlashWriteArray(u32 page)`: commits the page buffer.
    fn osFlashWriteArray_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        require_flash();
        let page = ctx.r[reg::A0] as u32;
        let buffer = *WRITE_BUFFER.lock().unwrap_or_else(|e| e.into_inner());
        saves().write_bytes(&buffer, page.wrapping_mul(PAGE_SIZE));
        ctx.set_return_u32(0);
    }

    /// `s32 osFlashReadArray(OSIoMesg*, s32 priority, u32 page, void* dram, u32 pages,
    /// OSMesgQueue*)`: the last two arguments are on the stack.
    fn osFlashReadArray_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        require_flash();
        let page = ctx.r[reg::A2] as u32;
        let dram = ctx.r[reg::A3] as i32;
        let sp = ctx.r[reg::SP];
        let pages = mem.read_u32(sp.wrapping_add(0x10));
        let mq = mem.read_u32(sp.wrapping_add(0x14)) as i32;
        saves().read(mem, dram, page.wrapping_mul(PAGE_SIZE), pages.wrapping_mul(PAGE_SIZE));
        cxx::send_mesg(mem, mq, 0, cxx::OS_MESG_NOBLOCK);
        ctx.set_return_u32(0);
    }

    /// `s32 osFlashChange(u32 flash_num)`: only one chip. The C++ `assert(false)`s, which
    /// release builds compile out.
    fn osFlashChange_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {
        require_flash();
    }
}
