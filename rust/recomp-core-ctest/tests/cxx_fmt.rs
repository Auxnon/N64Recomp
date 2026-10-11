//! hex_alt matches {fmt}'s "{:#X}".

use recomp_core::cxx_fmt::hex_alt;
use recomp_core_ctest::{fmt_hex_alt_i16, fmt_hex_alt_i64, fmt_hex_alt_u16};

#[test]
fn every_16_bit_value() {
    for v in 0..=u16::MAX {
        assert_eq!(hex_alt(v).to_string(), fmt_hex_alt_u16(v));
        assert_eq!(hex_alt(v as i16).to_string(), fmt_hex_alt_i16(v as i16));
    }
}

#[test]
fn i64_extremes() {
    for v in [
        0,
        1,
        -1,
        0x7F,
        -0x40,
        i64::MAX,
        i64::MIN,
        i64::MIN + 1,
        i32::MIN as i64,
    ] {
        assert_eq!(hex_alt(v).to_string(), fmt_hex_alt_i64(v), "{v}");
    }
}
