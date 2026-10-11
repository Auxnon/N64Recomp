//! Bindings to `cxx/reference.cpp`, the C++ behavior recomp-core has to match.

unsafe extern "C" {
    pub fn ref_set_order_u32(keys: *const u32, n: usize, out: *mut u32) -> usize;
    pub fn ref_set_order_i32(keys: *const i32, n: usize, out: *mut i32) -> usize;
    pub fn ref_fmt_hex_alt_i64(value: i64, out: *mut u8, cap: usize) -> usize;
    pub fn ref_fmt_hex_alt_u16(value: u16, out: *mut u8, cap: usize) -> usize;
    pub fn ref_fmt_hex_alt_i16(value: i16, out: *mut u8, cap: usize) -> usize;
}

macro_rules! safe_order {
    ($name:ident, $ffi:ident, $t:ty) => {
        /// The iteration order of a `std::unordered_set` after inserting `keys` in order.
        pub fn $name(keys: &[$t]) -> Vec<$t> {
            let mut out = vec![0; keys.len()];
            // SAFETY: `keys` and `out` are valid for `keys.len()` elements, and the set can
            // hold at most that many.
            let len = unsafe { $ffi(keys.as_ptr(), keys.len(), out.as_mut_ptr()) };
            out.truncate(len);
            out
        }
    };
}
safe_order!(set_order_u32, ref_set_order_u32, u32);
safe_order!(set_order_i32, ref_set_order_i32, i32);

macro_rules! safe_fmt {
    ($name:ident, $ffi:ident, $t:ty) => {
        /// `fmt::format("{:#X}", value)`.
        pub fn $name(value: $t) -> String {
            let mut out = [0u8; 64];
            // SAFETY: `out` is valid for its length.
            let len = unsafe { $ffi(value, out.as_mut_ptr(), out.len()) };
            String::from_utf8(out[..len].to_vec()).expect("fmt output is ASCII")
        }
    };
}
safe_fmt!(fmt_hex_alt_i64, ref_fmt_hex_alt_i64, i64);
safe_fmt!(fmt_hex_alt_u16, ref_fmt_hex_alt_u16, u16);
safe_fmt!(fmt_hex_alt_i16, ref_fmt_hex_alt_i16, i16);
