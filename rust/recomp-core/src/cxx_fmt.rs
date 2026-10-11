//! {fmt} formatting that Rust's `format!` spells differently.

use std::fmt::{self, Display};

/// `fmt::format("{:#X}", value)`: an uppercase `0X` prefix, and a sign in front of it for
/// negative values (`-0X10`). Rust's `{:#X}` writes `0x` and formats negatives as two's
/// complement.
#[derive(Clone, Copy, Debug)]
pub struct HexAlt(pub i64);

impl Display for HexAlt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        write!(f, "{sign}0X{:X}", self.0.unsigned_abs())
    }
}

/// [`HexAlt`] for any integer that fits in an `i64`.
pub fn hex_alt(value: impl Into<i64>) -> HexAlt {
    HexAlt(value.into())
}
