//! `std::sort` with libstdc++'s exact element order.
//!
//! `std::sort` isn't stable, so elements that compare equal (relocations at the same
//! address, for example) end up in whatever order libstdc++'s introsort leaves them. The
//! C++ tools' output depends on that order, so this follows the same algorithm step for
//! step: introsort with median-of-three pivots, a heapsort fallback, and a final insertion
//! sort over 16-element runs.
//!
//! The algorithm only ever looks at comparison results, so it sorts a permutation of
//! indices and then applies it. That gives the same result as sorting the elements in
//! place, and the elements don't need to be `Clone`.

const THRESHOLD: usize = 16;

/// Sorts `items` like `std::sort(begin, end, less)`.
pub fn sort_by<T>(items: &mut Vec<T>, mut less: impl FnMut(&T, &T) -> bool) {
    let mut order: Vec<usize> = (0..items.len()).collect();
    {
        let items: &[T] = items;
        let mut sorter = Sorter {
            v: &mut order,
            less: |a: &usize, b: &usize| less(&items[*a], &items[*b]),
        };
        sorter.sort();
    }
    let mut slots: Vec<Option<T>> = std::mem::take(items).into_iter().map(Some).collect();
    items.extend(
        order
            .into_iter()
            .map(|i| slots[i].take().expect("permutation repeats an index")),
    );
}

/// Sorts `items` like `std::sort` with a key comparison (`key(a) < key(b)`).
pub fn sort_by_key<T, K: PartialOrd>(items: &mut Vec<T>, mut key: impl FnMut(&T) -> K) {
    sort_by(items, |a, b| key(a) < key(b));
}

struct Sorter<'a, F> {
    v: &'a mut [usize],
    less: F,
}

impl<F: FnMut(&usize, &usize) -> bool> Sorter<'_, F> {
    fn lt(&mut self, a: usize, b: usize) -> bool {
        (self.less)(&self.v[a], &self.v[b])
    }

    fn lt_val(&mut self, val: usize, b: usize) -> bool {
        (self.less)(&val, &self.v[b])
    }

    fn val_lt(&mut self, a: usize, val: usize) -> bool {
        (self.less)(&self.v[a], &val)
    }

    fn sort(&mut self) {
        let len = self.v.len();
        if len == 0 {
            return;
        }
        // __lg(n) * 2
        let depth_limit = 2 * (usize::BITS - 1 - len.leading_zeros()) as usize;
        self.introsort_loop(0, len, depth_limit);
        self.final_insertion_sort(0, len);
    }

    fn introsort_loop(&mut self, first: usize, mut last: usize, mut depth_limit: usize) {
        while last - first > THRESHOLD {
            if depth_limit == 0 {
                self.partial_sort(first, last);
                return;
            }
            depth_limit -= 1;
            let cut = self.unguarded_partition_pivot(first, last);
            self.introsort_loop(cut, last, depth_limit);
            last = cut;
        }
    }

    fn unguarded_partition_pivot(&mut self, first: usize, last: usize) -> usize {
        let mid = first + (last - first) / 2;
        self.move_median_to_first(first, first + 1, mid, last - 1);
        self.unguarded_partition(first + 1, last, first)
    }

    fn move_median_to_first(&mut self, result: usize, a: usize, b: usize, c: usize) {
        let pick = if self.lt(a, b) {
            if self.lt(b, c) {
                b
            } else if self.lt(a, c) {
                c
            } else {
                a
            }
        } else if self.lt(a, c) {
            a
        } else if self.lt(b, c) {
            c
        } else {
            b
        };
        self.v.swap(result, pick);
    }

    fn unguarded_partition(&mut self, mut first: usize, mut last: usize, pivot: usize) -> usize {
        loop {
            while self.lt(first, pivot) {
                first += 1;
            }
            last -= 1;
            while self.lt(pivot, last) {
                last -= 1;
            }
            if first >= last {
                return first;
            }
            self.v.swap(first, last);
            first += 1;
        }
    }

    fn final_insertion_sort(&mut self, first: usize, last: usize) {
        if last - first > THRESHOLD {
            self.insertion_sort(first, first + THRESHOLD);
            for i in first + THRESHOLD..last {
                self.unguarded_linear_insert(i);
            }
        } else {
            self.insertion_sort(first, last);
        }
    }

    fn insertion_sort(&mut self, first: usize, last: usize) {
        if first == last {
            return;
        }
        for i in first + 1..last {
            if self.lt(i, first) {
                let val = self.v[i];
                self.v.copy_within(first..i, first + 1);
                self.v[first] = val;
            } else {
                self.unguarded_linear_insert(i);
            }
        }
    }

    fn unguarded_linear_insert(&mut self, mut last: usize) {
        let val = self.v[last];
        let mut next = last - 1;
        while self.lt_val(val, next) {
            self.v[last] = self.v[next];
            last = next;
            next -= 1;
        }
        self.v[last] = val;
    }

    // Heapsort fallback: std::__partial_sort(first, last, last) = make_heap + sort_heap.

    fn partial_sort(&mut self, first: usize, last: usize) {
        self.make_heap(first, last);
        let mut end = last;
        while end - first > 1 {
            end -= 1;
            // __pop_heap(first, end, end)
            let val = self.v[end];
            self.v[end] = self.v[first];
            self.adjust_heap(first, 0, end - first, val);
        }
    }

    fn make_heap(&mut self, first: usize, last: usize) {
        let len = last - first;
        if len < 2 {
            return;
        }
        let mut parent = (len - 2) / 2;
        loop {
            let val = self.v[first + parent];
            self.adjust_heap(first, parent, len, val);
            if parent == 0 {
                return;
            }
            parent -= 1;
        }
    }

    fn adjust_heap(&mut self, first: usize, mut hole: usize, len: usize, val: usize) {
        let top = hole;
        let mut child = hole;
        while child < (len - 1) / 2 {
            child = 2 * (child + 1);
            if self.lt(first + child, first + child - 1) {
                child -= 1;
            }
            self.v[first + hole] = self.v[first + child];
            hole = child;
        }
        if len & 1 == 0 && child == (len - 2) / 2 {
            child = 2 * (child + 1);
            self.v[first + hole] = self.v[first + child - 1];
            hole = child - 1;
        }
        // __push_heap
        while hole > top {
            let parent = (hole - 1) / 2;
            if !self.val_lt(first + parent, val) {
                break;
            }
            self.v[first + hole] = self.v[first + parent];
            hole = parent;
        }
        self.v[first + hole] = val;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts() {
        let mut v: Vec<u32> = (0..1000).map(|i| (i * 7919) % 1009).collect();
        sort_by_key(&mut v, |&x| x);
        assert!(v.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(v.len(), 1000);
    }
}
