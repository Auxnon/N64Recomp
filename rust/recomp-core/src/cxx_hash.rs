//! `std::unordered_map` / `std::unordered_set` with libstdc++'s iteration order.
//!
//! The C++ tools write some tables (indirect jump targets, resume targets, ...) by iterating
//! unordered containers, so their output depends on libstdc++'s hash table layout. To produce
//! byte-identical output, the port iterates in the same order. This reimplements the parts of
//! `_Hashtable` that decide that order:
//!
//! - All nodes form one singly linked list. Each bucket points at the node *before* its
//!   first node, so a bucket's nodes are contiguous.
//! - A new node goes at the front of its bucket, or at the front of the whole list if its
//!   bucket is empty.
//! - Rehashing walks the old list front to back and reinserts each node the same way.
//! - Bucket counts follow `_Prime_rehash_policy` (max load factor 1.0, growth factor 2,
//!   11 buckets minimum on first insert) with libstdc++'s prime table.
//!
//! `rust/recomp-core-ctest` checks this against the real containers.

use std::hash::Hash;

/// `std::hash<T>` for the integer keys the tools use: the value itself, as `size_t`.
pub trait CxxHash: Copy + Eq + Hash {
    fn cxx_hash(self) -> usize;
}

macro_rules! identity_hash {
    ($($t:ty),*) => {$(
        impl CxxHash for $t {
            fn cxx_hash(self) -> usize {
                // static_cast<size_t>(value): signed types sign-extend.
                self as usize
            }
        }
    )*};
}
identity_hash!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

/// The start of libstdc++'s `__prime_list` (LP64). Bucket counts never get near the end.
#[rustfmt::skip]
const PRIME_LIST: [usize; 256] = [
    2, 3, 5, 7, 11, 13, 17, 19,
    23, 29, 31, 37, 41, 43, 47, 53,
    59, 61, 67, 71, 73, 79, 83, 89,
    97, 103, 109, 113, 127, 137, 139, 149,
    157, 167, 179, 193, 199, 211, 227, 241,
    257, 277, 293, 313, 337, 359, 383, 409,
    439, 467, 503, 541, 577, 619, 661, 709,
    761, 823, 887, 953, 1031, 1109, 1193, 1289,
    1381, 1493, 1613, 1741, 1879, 2029, 2179, 2357,
    2549, 2753, 2971, 3209, 3469, 3739, 4027, 4349,
    4703, 5087, 5503, 5953, 6427, 6949, 7517, 8123,
    8783, 9497, 10273, 11113, 12011, 12983, 14033, 15173,
    16411, 17749, 19183, 20753, 22447, 24281, 26267, 28411,
    30727, 33223, 35933, 38873, 42043, 45481, 49201, 53201,
    57557, 62233, 67307, 72817, 78779, 85229, 92203, 99733,
    107897, 116731, 126271, 136607, 147793, 159871, 172933, 187091,
    202409, 218971, 236897, 256279, 277261, 299951, 324503, 351061,
    379787, 410857, 444487, 480881, 520241, 562841, 608903, 658753,
    712697, 771049, 834181, 902483, 976369, 1056323, 1142821, 1236397,
    1337629, 1447153, 1565659, 1693859, 1832561, 1982627, 2144977, 2320627,
    2510653, 2716249, 2938679, 3179303, 3439651, 3721303, 4026031, 4355707,
    4712381, 5098259, 5515729, 5967347, 6456007, 6984629, 7556579, 8175383,
    8844859, 9569143, 10352717, 11200489, 12117689, 13109983, 14183539, 15345007,
    16601593, 17961079, 19431899, 21023161, 22744717, 24607243, 26622317, 28802401,
    31160981, 33712729, 36473443, 39460231, 42691603, 46187573, 49969847, 54061849,
    58488943, 63278561, 68460391, 74066549, 80131819, 86693767, 93793069, 101473717,
    109783337, 118773397, 128499677, 139022417, 150406843, 162723577, 176048909, 190465427,
    206062531, 222936881, 241193053, 260944219, 282312799, 305431229, 330442829, 357502601,
    386778277, 418451333, 452718089, 489790921, 529899637, 573292817, 620239453, 671030513,
    725980837, 785430967, 849749479, 919334987, 994618837, 1076067617, 1164186217, 1259520799,
    1362662261, 1474249943, 1594975441, 1725587117, 1866894511, 2019773507, 2185171673, 2364114217,
    2557710269, 2767159799, 2993761039, 3238918481, 3504151727, 3791104843, 4101556399, 4294967291,
];

/// `_Prime_rehash_policy` with the default max load factor of 1.0.
#[derive(Clone, Debug, Default)]
struct RehashPolicy {
    next_resize: usize,
}

impl RehashPolicy {
    const GROWTH_FACTOR: usize = 2;

    fn next_bucket_count(&mut self, n: usize) -> usize {
        const FAST_BKT: [usize; 14] = [2, 2, 2, 3, 5, 5, 7, 7, 11, 11, 11, 11, 13, 13];
        if n < FAST_BKT.len() {
            if n == 0 {
                return 1;
            }
            self.next_resize = FAST_BKT[n];
            return FAST_BKT[n];
        }
        assert!(
            n <= PRIME_LIST[PRIME_LIST.len() - 2],
            "hash table too large to emulate"
        );
        // std::lower_bound(__prime_list + 6, __last_prime, n)
        let count = PRIME_LIST[6 + PRIME_LIST[6..].partition_point(|&p| p < n)];
        self.next_resize = count;
        count
    }

    /// The new bucket count, if inserting `n_ins` more elements needs a rehash.
    fn need_rehash(&mut self, n_bkt: usize, n_elt: usize, n_ins: usize) -> Option<usize> {
        if n_elt + n_ins <= self.next_resize {
            return None;
        }
        let min_bkts = (n_elt + n_ins).max(if self.next_resize == 0 { 11 } else { 0 });
        if min_bkts >= n_bkt {
            Some(self.next_bucket_count((min_bkts + 1).max(n_bkt * Self::GROWTH_FACTOR)))
        } else {
            self.next_resize = n_bkt;
            None
        }
    }
}

/// Where a bucket's run of nodes starts: after this node, or at the list head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Before {
    Head,
    Node(usize),
}

#[derive(Clone, Debug)]
struct Node<K, V> {
    key: K,
    value: V,
    next: Option<usize>,
}

/// `std::unordered_map<K, V>` (libstdc++, unique keys). Iterates in libstdc++'s order.
#[derive(Clone, Debug)]
pub struct CxxHashMap<K, V> {
    nodes: Vec<Node<K, V>>,
    head: Option<usize>,
    buckets: Vec<Option<Before>>,
    policy: RehashPolicy,
}

impl<K: CxxHash, V> Default for CxxHashMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: CxxHash, V> CxxHashMap<K, V> {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            head: None,
            buckets: vec![None],
            policy: RehashPolicy::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    fn bucket_of(&self, key: K) -> usize {
        key.cxx_hash() % self.buckets.len()
    }

    fn next_after(&self, before: Before) -> Option<usize> {
        match before {
            Before::Head => self.head,
            Before::Node(i) => self.nodes[i].next,
        }
    }

    fn set_next(&mut self, before: Before, next: Option<usize>) {
        match before {
            Before::Head => self.head = next,
            Before::Node(i) => self.nodes[i].next = next,
        }
    }

    fn find_index(&self, key: K) -> Option<usize> {
        let bucket = self.bucket_of(key);
        let mut node = self.next_after(self.buckets[bucket]?);
        while let Some(i) = node {
            if self.nodes[i].key == key {
                return Some(i);
            }
            node = self.nodes[i]
                .next
                .filter(|&n| self.bucket_of(self.nodes[n].key) == bucket);
        }
        None
    }

    pub fn contains_key(&self, key: K) -> bool {
        self.find_index(key).is_some()
    }

    pub fn get(&self, key: K) -> Option<&V> {
        self.find_index(key).map(|i| &self.nodes[i].value)
    }

    pub fn get_mut(&mut self, key: K) -> Option<&mut V> {
        self.find_index(key).map(|i| &mut self.nodes[i].value)
    }

    /// `_M_insert_bucket_begin`.
    fn link_at_bucket_begin(&mut self, bucket: usize, node: usize) {
        match self.buckets[bucket] {
            Some(before) => {
                self.nodes[node].next = self.next_after(before);
                self.set_next(before, Some(node));
            }
            None => {
                self.nodes[node].next = self.head;
                self.head = Some(node);
                if let Some(next) = self.nodes[node].next {
                    let next_bucket = self.bucket_of(self.nodes[next].key);
                    self.buckets[next_bucket] = Some(Before::Node(node));
                }
                self.buckets[bucket] = Some(Before::Head);
            }
        }
    }

    /// `_M_rehash_aux` for unique keys.
    fn rehash(&mut self, bucket_count: usize) {
        self.buckets = vec![None; bucket_count];
        let mut node = self.head.take();
        let mut head_bucket = 0;
        while let Some(i) = node {
            node = self.nodes[i].next;
            let bucket = self.bucket_of(self.nodes[i].key);
            match self.buckets[bucket] {
                None => {
                    self.nodes[i].next = self.head;
                    self.head = Some(i);
                    self.buckets[bucket] = Some(Before::Head);
                    if self.nodes[i].next.is_some() {
                        self.buckets[head_bucket] = Some(Before::Node(i));
                    }
                    head_bucket = bucket;
                }
                Some(before) => {
                    self.nodes[i].next = self.next_after(before);
                    self.set_next(before, Some(i));
                }
            }
        }
    }

    /// Like `emplace`/`insert`: leaves an existing entry alone. Returns whether it inserted.
    pub fn insert(&mut self, key: K, value: V) -> bool {
        if self.contains_key(key) {
            return false;
        }
        if let Some(count) = self
            .policy
            .need_rehash(self.buckets.len(), self.nodes.len(), 1)
        {
            self.rehash(count);
        }
        let node = self.nodes.len();
        self.nodes.push(Node {
            key,
            value,
            next: None,
        });
        self.link_at_bucket_begin(self.bucket_of(key), node);
        true
    }

    /// `operator[]`: the existing value, or a default-constructed one inserted now.
    pub fn entry_or_default(&mut self, key: K) -> &mut V
    where
        V: Default,
    {
        self.insert(key, V::default());
        self.get_mut(key).expect("just inserted")
    }

    /// Iterates in the order a C++ range-for over the container would.
    pub fn iter(&self) -> impl Iterator<Item = (K, &V)> + '_ {
        std::iter::successors(self.head, |&i| self.nodes[i].next).map(|i| {
            let node = &self.nodes[i];
            (node.key, &node.value)
        })
    }

    pub fn keys(&self) -> impl Iterator<Item = K> + '_ {
        self.iter().map(|(k, _)| k)
    }
}

/// `std::unordered_set<K>` (libstdc++). Iterates in libstdc++'s order.
#[derive(Clone, Debug)]
pub struct CxxHashSet<K>(CxxHashMap<K, ()>);

impl<K: CxxHash> Default for CxxHashSet<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: CxxHash> CxxHashSet<K> {
    pub fn new() -> Self {
        Self(CxxHashMap::new())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn insert(&mut self, key: K) -> bool {
        self.0.insert(key, ())
    }

    pub fn contains(&self, key: K) -> bool {
        self.0.contains_key(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = K> + '_ {
        self.0.keys()
    }
}

impl<K: CxxHash> FromIterator<K> for CxxHashSet<K> {
    fn from_iter<I: IntoIterator<Item = K>>(iter: I) -> Self {
        let mut set = Self::new();
        iter.into_iter().for_each(|k| {
            set.insert(k);
        });
        set
    }
}

impl<K: CxxHash> Extend<K> for CxxHashSet<K> {
    fn extend<I: IntoIterator<Item = K>>(&mut self, iter: I) {
        iter.into_iter().for_each(|k| {
            self.insert(k);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_insert_allocates_13_buckets() {
        let mut set = CxxHashSet::new();
        set.insert(5u32);
        assert_eq!(set.0.buckets.len(), 13);
    }

    #[test]
    fn same_bucket_inserts_go_to_bucket_front() {
        // 1 and 14 share bucket 1 of 13; the later one comes first.
        let set: CxxHashSet<u32> = [1, 14].into_iter().collect();
        assert_eq!(set.iter().collect::<Vec<_>>(), [14, 1]);
    }

    #[test]
    fn lookups_survive_rehash() {
        let set: CxxHashSet<u32> = (0..1000).map(|i| i * 7919).collect();
        assert_eq!(set.len(), 1000);
        assert!((0..1000).all(|i| set.contains(i * 7919)));
        assert!(!set.contains(1));
        assert_eq!(set.iter().count(), 1000);
    }
}
