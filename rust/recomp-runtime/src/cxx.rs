//! Calls into the parts of N64ModernRuntime still in C++.
//!
//! Each C function gets one safe wrapper here, and nothing else in this crate calls C; the
//! C++ calls into Rust (`recomp_rs_*`) are here too. `runtime.patch` adds the C entry points
//! on the C++ side. This module shrinks as those parts are ported (docs/rust-port-plan.md,
//! unsafe budget item 5).

#![allow(unsafe_code)]

use std::ffi::{c_char, CStr, CString};
use std::path::PathBuf;

use recomp_abi::GuestMemory;

use crate::saving;

unsafe extern "C" {
    fn recomp_save_type() -> u32;
    fn recomp_eeprom_allowed() -> bool;
    fn recomp_flashram_allowed() -> bool;
    fn recomp_flash_handle() -> i32;
    fn recomp_cart_handle() -> i32;
    fn recomp_drive_handle() -> i32;
    fn recomp_sram_allowed() -> bool;
    fn recomp_rom(len: *mut usize) -> *const u8;
    fn recomp_fatal_error(message: *const c_char) -> !;
    fn recomp_message_box(message: *const c_char);
    fn recomp_exited() -> bool;
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

/// Shows `message` and carries on.
pub fn message_box(message: &str) {
    let message = CString::new(message.replace('\0', " ")).expect("NULs replaced");
    // SAFETY: a valid C string.
    unsafe { recomp_message_box(message.as_ptr()) }
}

/// Whether the game is exiting (librecomp's `exited`).
pub fn exited() -> bool {
    // SAFETY: reads an atomic flag.
    unsafe { recomp_exited() }
}

/// A C string from the C++ as UTF-8 (lossily, though librecomp's are UTF-8).
///
/// # Safety
///
/// `text` is a valid, NUL-terminated string for the call.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
unsafe fn text(text: *const c_char) -> String {
    // SAFETY: from the caller.
    unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
}

/// `ultramodern::init_saving`: the save file `<folder>/<game_id>.bin`, `size` bytes.
///
/// # Safety
///
/// Both strings are valid and NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn recomp_rs_init_saving(folder: *const c_char, game_id: *const c_char, size: usize) {
    // SAFETY: from the caller.
    let (folder, game_id) = unsafe { (text(folder), text(game_id)) };
    saving::init(
        PathBuf::from(folder),
        &game_id,
        size,
        saving::Host {
            exited,
            report: message_box,
        },
    );
}

/// `ultramodern::change_save_file`.
///
/// # Safety
///
/// Both strings are valid and NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn recomp_rs_change_save_file(subfolder: *const c_char, name: *const c_char) {
    // SAFETY: from the caller.
    let (subfolder, name) = unsafe { (text(subfolder), text(name)) };
    saving::saves().change_file(&subfolder, &name);
}

/// `ultramodern::get_save_file_path`: copies up to `capacity` bytes of the path to `out` and
/// returns its full length, so the C++ can size its buffer with a first call.
///
/// # Safety
///
/// `out` is valid for `capacity` bytes (it may be null when `capacity` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn recomp_rs_save_file_path(out: *mut u8, capacity: usize) -> usize {
    let path = saving::saves().path();
    let bytes = path.as_os_str().as_encoded_bytes();
    let n = bytes.len().min(capacity);
    if n > 0 {
        // SAFETY: `out` holds `capacity` >= `n` bytes.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, n) };
    }
    bytes.len()
}

/// `ultramodern::join_saving_thread`.
#[unsafe(no_mangle)]
pub extern "C" fn recomp_rs_join_saving_thread() {
    if let Some(saves) = saving::try_saves() {
        saves.join();
    }
}
