//! Guest RDRAM, with the same layout and semantics as the `MEM_*` macros in `recomp.h`.
//!
//! Recompiled code sees RDRAM as `uint8_t* rdram`, where `rdram[0]` is KSEG0 address
//! `0x80000000`. Memory is stored as native-endian (little-endian) 32-bit words, so a word
//! access is a plain load, while byte and halfword accesses flip the low address bits
//! (`^ 3` and `^ 2`) to land on the right part of the word.
//!
//! The same memory is used at the same time by C game threads, the graphics thread and
//! audio. Plain `&[u8]` or `&mut [u8]` views of it would be undefined behavior in Rust,
//! so it is viewed as `[AtomicU32]`. Relaxed atomic loads and stores compile to ordinary
//! loads and stores on x86-64 and ARM64. Byte and halfword stores update their word with
//! a compare-and-swap loop so a concurrent store to a neighboring byte isn't lost.

use std::sync::atomic::{AtomicU32, Ordering};

use crate::Gpr;

#[cfg(target_endian = "big")]
compile_error!("recompiled code and its RDRAM layout require a little-endian host");

/// The sign-extended KSEG0 address that maps to `rdram[0]`.
pub const RDRAM_BASE: Gpr = 0xFFFF_FFFF_8000_0000;

/// Sign-extends a 32-bit guest address to the 64-bit form held in registers.
pub fn sign_extend(addr: u32) -> Gpr {
    addr as i32 as i64 as u64
}

/// A view of guest RDRAM, borrowed for `'m`.
#[derive(Clone, Copy, Debug)]
pub struct GuestMemory<'m> {
    words: &'m [AtomicU32],
}

impl<'m> GuestMemory<'m> {
    /// Wraps memory the caller already owns, e.g. a `Vec<AtomicU32>` in tests.
    pub fn new(words: &'m [AtomicU32]) -> Self {
        Self { words }
    }

    /// Wraps the runtime's RDRAM mapping.
    ///
    /// # Safety
    ///
    /// `ptr` must be 4-byte aligned, and `len_bytes` bytes from it must be readable and
    /// writable for all of `'m`. Other code may write that memory concurrently (that is the
    /// point of the atomic view), but nothing may unmap or reprotect it during `'m`.
    #[allow(unsafe_code)]
    pub unsafe fn from_raw_parts(ptr: *mut u8, len_bytes: usize) -> Self {
        assert!(ptr.cast::<u32>().is_aligned(), "RDRAM pointer {ptr:p} is not 4-byte aligned");
        assert!(len_bytes % 4 == 0, "RDRAM length {len_bytes:#x} is not a multiple of 4");
        // SAFETY: the caller guarantees the range is valid for 'm. AtomicU32 has the same
        // size and alignment as u32 and permits shared mutation.
        let words = unsafe { std::slice::from_raw_parts(ptr.cast::<AtomicU32>(), len_bytes / 4) };
        Self { words }
    }

    /// The `uint8_t* rdram` pointer recompiled functions expect.
    pub fn as_ptr(&self) -> *mut u8 {
        self.words.as_ptr().cast_mut().cast()
    }

    pub fn len_bytes(&self) -> usize {
        self.words.len() * 4
    }

    /// Whether `len` bytes from `addr` are inside this view.
    pub fn contains(&self, addr: Gpr, len: u64) -> bool {
        let offset = addr.wrapping_sub(RDRAM_BASE);
        offset
            .checked_add(len)
            .is_some_and(|end| end <= self.len_bytes() as u64)
    }

    #[track_caller]
    fn word(&self, offset: u64) -> &AtomicU32 {
        let index = usize::try_from(offset / 4).ok();
        match index.and_then(|i| self.words.get(i)) {
            Some(word) => word,
            None => panic!(
                "guest address {:#x} is outside RDRAM ({:#x} bytes)",
                offset.wrapping_add(RDRAM_BASE),
                self.len_bytes()
            ),
        }
    }

    #[track_caller]
    fn aligned_offset(addr: Gpr, align: u64) -> u64 {
        assert!(addr % align == 0, "unaligned {align}-byte access at guest address {addr:#x}");
        addr.wrapping_sub(RDRAM_BASE)
    }

    /// Replaces `width` bits at `shift` in the word at `offset`, leaving the rest intact.
    #[track_caller]
    fn store_bits(&self, offset: u64, shift: u32, width: u32, value: u32) {
        let mask = ((1u64 << width) - 1) as u32;
        let word = self.word(offset);
        let mut old = word.load(Ordering::Relaxed);
        loop {
            let new = (old & !(mask << shift)) | ((value & mask) << shift);
            match word.compare_exchange_weak(old, new, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(current) => old = current,
            }
        }
    }

    // MEM_W

    #[track_caller]
    pub fn read_u32(&self, addr: Gpr) -> u32 {
        self.word(Self::aligned_offset(addr, 4)).load(Ordering::Relaxed)
    }

    #[track_caller]
    pub fn write_u32(&self, addr: Gpr, value: u32) {
        self.word(Self::aligned_offset(addr, 4)).store(value, Ordering::Relaxed);
    }

    #[track_caller]
    pub fn read_i32(&self, addr: Gpr) -> i32 {
        self.read_u32(addr) as i32
    }

    // MEM_HU / MEM_H

    #[track_caller]
    pub fn read_u16(&self, addr: Gpr) -> u16 {
        let offset = Self::aligned_offset(addr, 2) ^ 2;
        (self.word(offset).load(Ordering::Relaxed) >> ((offset & 2) * 8)) as u16
    }

    #[track_caller]
    pub fn write_u16(&self, addr: Gpr, value: u16) {
        let offset = Self::aligned_offset(addr, 2) ^ 2;
        self.store_bits(offset, ((offset & 2) * 8) as u32, 16, value.into());
    }

    #[track_caller]
    pub fn read_i16(&self, addr: Gpr) -> i16 {
        self.read_u16(addr) as i16
    }

    // MEM_BU / MEM_B

    #[track_caller]
    pub fn read_u8(&self, addr: Gpr) -> u8 {
        let offset = addr.wrapping_sub(RDRAM_BASE) ^ 3;
        (self.word(offset).load(Ordering::Relaxed) >> ((offset & 3) * 8)) as u8
    }

    #[track_caller]
    pub fn write_u8(&self, addr: Gpr, value: u8) {
        let offset = addr.wrapping_sub(RDRAM_BASE) ^ 3;
        self.store_bits(offset, ((offset & 3) * 8) as u32, 8, value.into());
    }

    #[track_caller]
    pub fn read_i8(&self, addr: Gpr) -> i8 {
        self.read_u8(addr) as i8
    }

    // LD / SD: the high word is at the lower address.

    #[track_caller]
    pub fn read_u64(&self, addr: Gpr) -> u64 {
        let hi = self.read_u32(addr);
        let lo = self.read_u32(addr.wrapping_add(4));
        (u64::from(hi) << 32) | u64::from(lo)
    }

    #[track_caller]
    pub fn write_u64(&self, addr: Gpr, value: u64) {
        self.write_u32(addr.wrapping_add(4), value as u32);
        self.write_u32(addr, (value >> 32) as u32);
    }

    // Unaligned loads and stores (do_lwl ... do_sdr). `addr` is `offset + reg`.

    #[track_caller]
    pub fn lwl(&self, initial: Gpr, addr: Gpr) -> Gpr {
        let loaded = self.read_u32(addr & !3);
        let shift = (addr & 3) * 8;
        let masked = initial & Gpr::from(!(0xFFFF_FFFFu32 << shift));
        (masked | Gpr::from(loaded << shift)) as i32 as i64 as u64
    }

    #[track_caller]
    pub fn lwr(&self, initial: Gpr, addr: Gpr) -> Gpr {
        let loaded = self.read_u32(addr & !3);
        let shift = 24 - (addr & 3) * 8;
        let masked = initial & Gpr::from(!(0xFFFF_FFFFu32 >> shift));
        (masked | Gpr::from(loaded >> shift)) as i32 as i64 as u64
    }

    #[track_caller]
    pub fn swl(&self, addr: Gpr, value: Gpr) {
        let word_addr = addr & !3;
        let shift = (addr & 3) * 8;
        let initial = self.read_u32(word_addr);
        let merged = (initial & !(0xFFFF_FFFFu32 >> shift)) | ((value as u32) >> shift);
        self.write_u32(word_addr, merged);
    }

    #[track_caller]
    pub fn swr(&self, addr: Gpr, value: Gpr) {
        let word_addr = addr & !3;
        let shift = 24 - (addr & 3) * 8;
        let initial = self.read_u32(word_addr);
        let merged = (initial & !(0xFFFF_FFFFu32 << shift)) | ((value as u32) << shift);
        self.write_u32(word_addr, merged);
    }

    #[track_caller]
    pub fn ldl(&self, initial: Gpr, addr: Gpr) -> Gpr {
        let loaded = self.read_u64(addr & !7);
        let shift = (addr & 7) * 8;
        (initial & !(u64::MAX << shift)) | (loaded << shift)
    }

    #[track_caller]
    pub fn ldr(&self, initial: Gpr, addr: Gpr) -> Gpr {
        let loaded = self.read_u64(addr & !7);
        let shift = 56 - (addr & 7) * 8;
        (initial & !(u64::MAX >> shift)) | (loaded >> shift)
    }

    #[track_caller]
    pub fn sdl(&self, addr: Gpr, value: Gpr) {
        let dword_addr = addr & !7;
        let shift = (addr & 7) * 8;
        let initial = self.read_u64(dword_addr);
        self.write_u64(dword_addr, (initial & !(u64::MAX >> shift)) | (value >> shift));
    }

    #[track_caller]
    pub fn sdr(&self, addr: Gpr, value: Gpr) {
        let dword_addr = addr & !7;
        let shift = 56 - (addr & 7) * 8;
        let initial = self.read_u64(dword_addr);
        self.write_u64(dword_addr, (initial & !(u64::MAX << shift)) | (value << shift));
    }

    // Bulk copies in guest byte order (what the N64 sees at increasing addresses).

    #[track_caller]
    pub fn read_bytes(&self, addr: Gpr, out: &mut [u8]) {
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = self.read_u8(addr.wrapping_add(i as u64));
        }
    }

    #[track_caller]
    pub fn write_bytes(&self, addr: Gpr, bytes: &[u8]) {
        for (i, &byte) in bytes.iter().enumerate() {
            self.write_u8(addr.wrapping_add(i as u64), byte);
        }
    }
}
