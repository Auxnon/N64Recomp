//! Parallel Interface: cartridge ROM and SRAM transfers (librecomp's `pi.cpp` exports).
//!
//! Transfers complete at once: the data is copied, then the completion message is sent. ROM
//! reads come from the ROM librecomp loaded; SRAM goes through librecomp's save buffer.

use recomp_abi::{recomp_export, reg, Gpr, GuestMemory, RecompContext};

use crate::cxx;
use crate::saving::saves;

const ROM_BASE: u32 = 0x1000_0000;
const SRAM_BASE: u32 = 0x0800_0000;
const DRIVE_BASE: u32 = 0x0600_0000;

/// `OSPiHandle` field offsets in guest memory.
const HANDLE_TYPE: u64 = 4;
const HANDLE_DOMAIN: u64 = 9;
const HANDLE_BASE_ADDRESS: u64 = 12;
/// `OSIoMesg` field offsets in guest memory.
const MESG_RET_QUEUE: u64 = 4;
const MESG_DRAM_ADDR: u64 = 8;
const MESG_DEV_ADDR: u64 = 12;
const MESG_SIZE: u64 = 16;

fn k1_to_phys(address: u32) -> u32 {
    address & 0x1FFF_FFFF
}

fn phys_to_k1(address: u32) -> u32 {
    address | 0xA000_0000
}

/// A guest pointer from a 32-bit value, sign-extended as the C++ stores it in a `gpr`.
fn widen(value: i32) -> Gpr {
    i64::from(value) as Gpr
}

/// librecomp's `do_rom_read`: `count` ROM bytes from `physical` to guest memory.
fn rom_read(mem: &GuestMemory, ram_address: Gpr, physical: u32, count: u32) {
    cxx::with_rom(|rom| {
        let start = (physical - ROM_BASE) as usize;
        for (i, &byte) in rom[start..start + count as usize].iter().enumerate() {
            mem.write_u8(ram_address.wrapping_add(i as u64), byte);
        }
    });
}

/// librecomp's `do_rom_pio`: one 4-byte word.
fn rom_pio(mem: &GuestMemory, ram_address: Gpr, physical: u32) {
    rom_read(mem, ram_address, physical, 4);
}

fn require_sram() {
    if !cxx::sram_allowed() {
        cxx::fatal_error(c"Attempted to use SRAM saving with other save type");
    }
}

/// librecomp's `do_dma`. `write` is the direction: RDRAM to the device.
fn dma(mem: &GuestMemory, mq: i32, ram_address: Gpr, physical: u32, size: u32, write: bool) {
    if physical >= ROM_BASE {
        if write {
            cxx::fatal_error(c"ROM DMA write unimplemented");
        }
        rom_read(mem, ram_address, physical, size);
    } else if physical >= SRAM_BASE {
        require_sram();
        // The save buffer takes the address as a 32-bit guest pointer.
        let address = ram_address as i32;
        if write {
            saves().write(mem, address, physical - SRAM_BASE, size);
        } else {
            saves().read(mem, address, physical - SRAM_BASE, size);
        }
    } else {
        let what = if write { "write to" } else { "read from" };
        eprintln!("[WARN] PI DMA {what} unknown region, phys address 0x{physical:08X}");
        return;
    }
    cxx::send_mesg(mem, mq, 0, cxx::OS_MESG_NOBLOCK);
}

fn init_handle(mem: &GuestMemory, handle: i32, kind: u8, base: u32, ctx: &mut RecompContext) {
    let address = widen(handle);
    mem.write_u8(address + HANDLE_TYPE, kind);
    mem.write_u32(address + HANDLE_BASE_ADDRESS, phys_to_k1(base));
    mem.write_u8(address + HANDLE_DOMAIN, 0);
    ctx.set_return_i32(handle);
}

recomp_export! {
    fn __osPiGetAccess_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    fn __osPiRelAccess_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    /// `OSPiHandle* osCartRomInit(void)`
    fn osCartRomInit_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        init_handle(mem, cxx::cart_handle(), 0, ROM_BASE, ctx);
    }

    /// `OSPiHandle* osDriveRomInit(void)`: a bulk device.
    fn osDriveRomInit_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        init_handle(mem, cxx::drive_handle(), 1, DRIVE_BASE, ctx);
    }

    fn osCreatePiManager_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {}

    /// `s32 osPiStartDma(OSIoMesg*, s32 priority, s32 direction, u32 devAddr, void* dram,
    /// u32 size, OSMesgQueue*)`: the last three on the stack.
    fn osPiStartDma_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        let write = ctx.r[reg::A2] as u32 != 0;
        let dev_addr = ctx.r[reg::A3] as u32 | ROM_BASE;
        let sp = ctx.r[reg::SP];
        let dram = widen(mem.read_i32(sp.wrapping_add(0x10)));
        let size = mem.read_u32(sp.wrapping_add(0x14));
        let mq = mem.read_i32(sp.wrapping_add(0x18));
        dma(mem, mq, dram, k1_to_phys(dev_addr), size, write);
        ctx.set_return_u32(0);
    }

    /// `s32 osEPiStartDma(OSPiHandle*, OSIoMesg*, s32 direction)`
    fn osEPiStartDma_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        let handle = ctx.r[reg::A0];
        let mb = ctx.r[reg::A1];
        let write = ctx.r[reg::A2] as u32 != 0;
        let dev_addr = mem.read_u32(handle.wrapping_add(HANDLE_BASE_ADDRESS))
            | mem.read_u32(mb.wrapping_add(MESG_DEV_ADDR));
        let dram = widen(mem.read_i32(mb.wrapping_add(MESG_DRAM_ADDR)));
        let size = mem.read_u32(mb.wrapping_add(MESG_SIZE));
        let mq = mem.read_i32(mb.wrapping_add(MESG_RET_QUEUE));
        dma(mem, mq, dram, k1_to_phys(dev_addr), size, write);
        ctx.set_return_u32(0);
    }

    /// `s32 osEPiReadIo(OSPiHandle*, u32 devAddr, u32* data)`: one word. Reading SRAM this
    /// way is an `assert` in C++, compiled out in release builds, so it does nothing.
    fn osEPiReadIo_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
        let handle = ctx.r[reg::A0];
        let dev_addr = mem.read_u32(handle.wrapping_add(HANDLE_BASE_ADDRESS)) | ctx.r[reg::A1] as u32;
        let dram = ctx.r[reg::A2];
        let physical = k1_to_phys(dev_addr);
        if physical > ROM_BASE {
            rom_pio(mem, dram, physical);
        }
        ctx.set_return_u32(0);
    }

    fn osPiGetStatus_recomp(_mem: &GuestMemory, ctx: &mut RecompContext) {
        ctx.set_return_u32(0);
    }

    fn osPiRawStartDma_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {
        cxx::fatal_error(&raw_dma_message("osPiRawStartDma_recomp"));
    }

    fn osEPiRawStartDma_recomp(_mem: &GuestMemory, _ctx: &mut RecompContext) {
        cxx::fatal_error(&raw_dma_message("osEPiRawStartDma_recomp"));
    }
}

/// The C++ message for a raw PI DMA call, which means a libultra function wasn't identified.
fn raw_dma_message(function: &str) -> std::ffi::CString {
    std::ffi::CString::new(format!(
        "Stub `{function}` function called!\n\
         Most games do not call this function directly, which means the libultra function\n\
         that uses this function was not properly named.\n\
         \n\
         If you triggered this message, please make sure you have properly identified\n\
         every libultra function on your recompiled game. If you are sure every libultra\n\
         function has been identified and you still get this problem then open an issue on\n\
         the N64ModernRuntime Github repository mentioning the game you are trying to\n\
         recompile and steps to reproduce the issue.\n\
         \n\
         The application will close now, bye and good luck!"
    ))
    .expect("no NUL in the message")
}
