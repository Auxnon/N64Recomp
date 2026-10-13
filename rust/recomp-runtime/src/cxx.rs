//! Calls into the parts of N64ModernRuntime still in C++.
//!
//! Each C function gets one safe wrapper here, and nothing else in this crate calls C. The
//! save functions are librecomp's save buffer (`pi.cpp`), which writes the save file from its
//! own thread; `runtime.patch` adds their C entry points. This module goes away piece by piece
//! as those parts are ported (docs/rust-port-plan.md, unsafe budget item 5).

#![allow(unsafe_code)]

use std::ffi::{c_char, CStr};

use recomp_abi::GuestMemory;

unsafe extern "C" {
    fn recomp_save_read(rdram: *mut u8, rdram_address: i32, offset: u32, count: u32);
    fn recomp_save_write(rdram: *mut u8, rdram_address: i32, offset: u32, count: u32);
    fn recomp_save_write_ptr(input: *const u8, offset: u32, count: u32);
    fn recomp_save_clear(start: u32, size: u32, value: u8);
    fn recomp_save_type() -> u32;
    fn recomp_eeprom_allowed() -> bool;
    fn recomp_flashram_allowed() -> bool;
    fn recomp_flash_handle() -> i32;
    fn recomp_cart_handle() -> i32;
    fn recomp_drive_handle() -> i32;
    fn recomp_sram_allowed() -> bool;
    fn recomp_rom(len: *mut usize) -> *const u8;
    fn recomp_fatal_error(message: *const c_char) -> !;
    fn osSendMesg(rdram: *mut u8, mq: i32, msg: i32, flag: i32) -> i32;
}

/// librecomp's `recomp::SaveType`, in declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveType {
    None,
    Eep4k,
    Eep16k,
    Sram,
    Flashram,
    /// Every save type works, and EEPROM reports 16 kbit.
    AllowAll,
}

pub fn save_type() -> SaveType {
    // SAFETY: reads the registered game's save type.
    match unsafe { recomp_save_type() } {
        1 => SaveType::Eep4k,
        2 => SaveType::Eep16k,
        3 => SaveType::Sram,
        4 => SaveType::Flashram,
        5 => SaveType::AllowAll,
        _ => SaveType::None,
    }
}

pub fn eeprom_allowed() -> bool {
    // SAFETY: reads the registered game's save type.
    unsafe { recomp_eeprom_allowed() }
}

pub fn flashram_allowed() -> bool {
    // SAFETY: reads the registered game's save type.
    unsafe { recomp_flashram_allowed() }
}

pub fn sram_allowed() -> bool {
    // SAFETY: reads the registered game's save type.
    unsafe { recomp_sram_allowed() }
}

/// The guest address of the cartridge `OSPiHandle` librecomp reserves in extended RDRAM.
pub fn cart_handle() -> i32 {
    // SAFETY: returns a constant.
    unsafe { recomp_cart_handle() }
}

/// The guest address of the 64DD drive `OSPiHandle`.
pub fn drive_handle() -> i32 {
    // SAFETY: returns a constant.
    unsafe { recomp_drive_handle() }
}

/// Runs `f` on the loaded ROM (empty if none). The C++ owns the bytes and only replaces them
/// when a ROM is selected, before the game starts, so they're stable for the call.
pub fn with_rom<R>(f: impl FnOnce(&[u8]) -> R) -> R {
    let mut len = 0;
    // SAFETY: returns the ROM vector's data pointer and length.
    let data = unsafe { recomp_rom(&mut len) };
    let rom = if data.is_null() {
        &[][..]
    } else {
        // SAFETY: `len` bytes from `data` are the ROM, alive and unchanged during the call.
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    f(rom)
}

/// The guest address of the flash `OSPiHandle` librecomp reserves in extended RDRAM.
pub fn flash_handle() -> i32 {
    // SAFETY: returns a constant.
    unsafe { recomp_flash_handle() }
}

/// Copies `count` save bytes from `offset` to guest memory at `address`.
pub fn save_read(mem: &GuestMemory, address: i32, offset: u32, count: u32) {
    // SAFETY: `mem` is the guest RDRAM the C++ runtime registered; it writes through the same
    // byte-swizzled addressing as the guest.
    unsafe { recomp_save_read(mem.as_ptr(), address, offset, count) }
}

/// Copies `count` bytes from guest memory at `address` into the save at `offset`.
pub fn save_write(mem: &GuestMemory, address: i32, offset: u32, count: u32) {
    // SAFETY: as in `save_read`; this only reads guest memory.
    unsafe { recomp_save_write(mem.as_ptr(), address, offset, count) }
}

/// Copies `bytes` into the save at `offset`.
pub fn save_write_bytes(bytes: &[u8], offset: u32) {
    // SAFETY: the C++ copies `bytes.len()` bytes from a valid slice.
    unsafe { recomp_save_write_ptr(bytes.as_ptr(), offset, bytes.len() as u32) }
}

/// Fills `size` save bytes from `start` with `value`.
pub fn save_clear(start: u32, size: u32, value: u8) {
    // SAFETY: touches only the C++ save buffer.
    unsafe { recomp_save_clear(start, size, value) }
}

/// libultra's `OS_MESG_NOBLOCK`.
pub const OS_MESG_NOBLOCK: i32 = 0;

/// `osSendMesg(mq, msg, flag)`.
pub fn send_mesg(mem: &GuestMemory, mq: i32, msg: i32, flag: i32) -> i32 {
    // SAFETY: the C++ message queue code takes the guest RDRAM and a guest pointer.
    unsafe { osSendMesg(mem.as_ptr(), mq, msg, flag) }
}

/// Shows `message` and exits, as `ULTRAMODERN_QUICK_EXIT` does.
pub fn fatal_error(message: &CStr) -> ! {
    // SAFETY: `message` is a valid C string; the C++ doesn't return.
    unsafe { recomp_fatal_error(message.as_ptr()) }
}
