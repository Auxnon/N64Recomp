//! CxxHashSet iterates in the same order as libstdc++'s std::unordered_set.

use recomp_core::cxx_hash::CxxHashSet;
use recomp_core_ctest::{set_order_i32, set_order_u32};

/// xorshift64*, so failures reproduce without a rand dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn check_u32(keys: &[u32]) {
    let ours: Vec<u32> = keys
        .iter()
        .copied()
        .collect::<CxxHashSet<u32>>()
        .iter()
        .collect();
    assert_eq!(ours, set_order_u32(keys), "keys: {keys:x?}");
}

#[test]
fn every_size_up_to_rehash_boundaries() {
    // Covers the 13 -> 29 -> 59 -> 127 -> ... bucket growth steps.
    for n in 0..600u32 {
        let keys: Vec<u32> = (0..n).map(|i| i.wrapping_mul(0x9E37_79B9) >> 7).collect();
        check_u32(&keys);
    }
}

#[test]
fn random_keys_with_duplicates() {
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    for round in 0..2000 {
        let len = (rng.next() % 300) as usize;
        // Small ranges force duplicates and bucket collisions; large ones spread out.
        let range = [16u64, 64, 0x2000, 1 << 32][round % 4];
        let keys: Vec<u32> = (0..len).map(|_| (rng.next() % range) as u32).collect();
        check_u32(&keys);
    }
}

#[test]
fn rsp_like_addresses() {
    // Word-aligned IMEM addresses, the shape of RSPRecomp's jump and resume targets.
    let mut rng = Rng(42);
    for _ in 0..500 {
        let len = (rng.next() % 64) as usize;
        let keys: Vec<u32> = (0..len)
            .map(|_| 0x1000 + ((rng.next() % 0x400) as u32) * 4)
            .collect();
        check_u32(&keys);
    }
}

#[test]
fn negative_signed_keys() {
    let mut rng = Rng(7);
    for _ in 0..500 {
        let len = (rng.next() % 100) as usize;
        let keys: Vec<i32> = (0..len)
            .map(|_| rng.next() as i32 >> (rng.next() % 31))
            .collect();
        let ours: Vec<i32> = keys
            .iter()
            .copied()
            .collect::<CxxHashSet<i32>>()
            .iter()
            .collect();
        assert_eq!(ours, set_order_i32(&keys), "keys: {keys:?}");
    }
}
