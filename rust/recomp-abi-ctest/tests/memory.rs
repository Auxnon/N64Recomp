//! Runs the same random operations through recomp.h (C) and GuestMemory (Rust) on two copies
//! of the same memory, and requires identical results and identical memory afterwards.

use std::sync::atomic::{AtomicU32, Ordering};

use recomp_abi::{GuestMemory, RDRAM_BASE};
use recomp_abi_ctest::*;

const WORDS: usize = 1024;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn memory(rng: &mut Rng) -> Vec<AtomicU32> {
    (0..WORDS).map(|_| AtomicU32::new(rng.next() as u32)).collect()
}

fn snapshot(words: &[AtomicU32]) -> Vec<u32> {
    words.iter().map(|w| w.load(Ordering::Relaxed)).collect()
}

#[test]
fn guest_memory_matches_recomp_h() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let c_words = memory(&mut Rng(1));
    let rust_words = memory(&mut Rng(1));
    let c_ptr = c_words.as_ptr().cast_mut().cast::<u8>();
    let mem = GuestMemory::new(&rust_words);

    for step in 0..200_000 {
        // Stay 8 bytes clear of the end so doubleword accesses fit.
        let offset = rng.next() % (WORDS as u64 * 4 - 8);
        let value = rng.next();
        let op = rng.next() % 18;
        let addr = RDRAM_BASE + offset;
        let w = addr & !3;
        let h = addr & !1;
        let d = addr & !7;

        let (c, r): (u64, u64) = unsafe {
            match op {
                0 => (ctest_read_w(c_ptr, w).into(), mem.read_u32(w).into()),
                1 => (ctest_read_hu(c_ptr, h).into(), mem.read_u16(h).into()),
                2 => (ctest_read_h(c_ptr, h) as u64, mem.read_i16(h) as u64),
                3 => (ctest_read_bu(c_ptr, addr).into(), mem.read_u8(addr).into()),
                4 => (ctest_read_b(c_ptr, addr) as u64, mem.read_i8(addr) as u64),
                5 => (ctest_read_d(c_ptr, w), mem.read_u64(w)),
                6 => { ctest_write_w(c_ptr, w, value as u32); mem.write_u32(w, value as u32); (0, 0) }
                7 => { ctest_write_h(c_ptr, h, value as u16); mem.write_u16(h, value as u16); (0, 0) }
                8 => { ctest_write_b(c_ptr, addr, value as u8); mem.write_u8(addr, value as u8); (0, 0) }
                9 => { ctest_write_d(c_ptr, w, value); mem.write_u64(w, value); (0, 0) }
                10 => (ctest_lwl(c_ptr, value, addr), mem.lwl(value, addr)),
                11 => (ctest_lwr(c_ptr, value, addr), mem.lwr(value, addr)),
                12 => (ctest_ldl(c_ptr, value, d + (addr & 7)), mem.ldl(value, d + (addr & 7))),
                13 => (ctest_ldr(c_ptr, value, d + (addr & 7)), mem.ldr(value, d + (addr & 7))),
                14 => { ctest_swl(c_ptr, addr, value); mem.swl(addr, value); (0, 0) }
                15 => { ctest_swr(c_ptr, addr, value); mem.swr(addr, value); (0, 0) }
                16 => { ctest_sdl(c_ptr, addr, value); mem.sdl(addr, value); (0, 0) }
                _ => { ctest_sdr(c_ptr, addr, value); mem.sdr(addr, value); (0, 0) }
            }
        };
        assert_eq!(c, r, "step {step}: op {op} at {addr:#x} value {value:#x}: C {c:#x} vs Rust {r:#x}");
        if op >= 6 {
            assert!(
                snapshot(&c_words) == snapshot(&rust_words),
                "step {step}: memory differs after op {op} at {addr:#x} value {value:#x}"
            );
        }
    }
}

#[test]
fn bulk_copies_use_guest_byte_order() {
    let words: Vec<AtomicU32> = (0..4).map(|_| AtomicU32::new(0)).collect();
    let mem = GuestMemory::new(&words);
    mem.write_u32(RDRAM_BASE, 0x1122_3344);
    let mut out = [0u8; 4];
    mem.read_bytes(RDRAM_BASE, &mut out);
    assert_eq!(out, [0x11, 0x22, 0x33, 0x44]);
    mem.write_bytes(RDRAM_BASE + 5, &[0xAA, 0xBB]);
    assert_eq!(mem.read_u32(RDRAM_BASE + 4), 0x00AA_BB00);
}

#[test]
#[should_panic(expected = "outside RDRAM")]
fn out_of_range_access_panics() {
    let words: Vec<AtomicU32> = (0..4).map(|_| AtomicU32::new(0)).collect();
    GuestMemory::new(&words).read_u32(RDRAM_BASE + 16);
}

#[test]
#[should_panic(expected = "unaligned")]
fn unaligned_word_access_panics() {
    let words: Vec<AtomicU32> = (0..4).map(|_| AtomicU32::new(0)).collect();
    GuestMemory::new(&words).read_u32(RDRAM_BASE + 2);
}
