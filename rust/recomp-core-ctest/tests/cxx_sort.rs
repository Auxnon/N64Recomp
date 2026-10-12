//! cxx_sort::sort_by leaves equal elements in the same order as libstdc++'s std::sort.

use recomp_core::cxx_sort::sort_by_key;
use recomp_core_ctest::{antiqsort_keys, sort_order_u32};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn check(keys: &[u32]) {
    let mut ours: Vec<(u32, usize)> = keys.iter().copied().zip(0..).collect();
    sort_by_key(&mut ours, |&(k, _)| k);
    let ours: Vec<usize> = ours.into_iter().map(|(_, i)| i).collect();
    assert_eq!(ours, sort_order_u32(keys), "keys: {keys:?}");
}

#[test]
fn random_with_many_ties() {
    let mut rng = Rng(0xDEAD_BEEF_1234_5678);
    for round in 0..3000 {
        let len = (rng.next() % 400) as usize;
        let range = [1u64, 2, 5, 17, 100, 1 << 32][round % 6];
        let keys: Vec<u32> = (0..len).map(|_| (rng.next() % range) as u32).collect();
        check(&keys);
    }
}

#[test]
fn sorted_reversed_and_sawtooth() {
    for n in [0usize, 1, 2, 15, 16, 17, 33, 100, 1000, 5000] {
        check(&(0..n as u32).collect::<Vec<_>>());
        check(&(0..n as u32).rev().collect::<Vec<_>>());
        check(&(0..n as u32).map(|i| i % 7).collect::<Vec<_>>());
        check(&(0..n as u32).map(|i| (i / 3) % 11).collect::<Vec<_>>());
    }
}

#[test]
fn heapsort_fallback() {
    // McIlroy's adversary inputs push introsort past its depth limit.
    for n in [20usize, 64, 200, 1000, 4000] {
        let keys = antiqsort_keys(n);
        check(&keys);
        // With ties folded in, so the heapsort path also has equal elements to place.
        check(&keys.iter().map(|k| k / 3).collect::<Vec<_>>());
    }
}
