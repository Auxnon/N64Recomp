//! `recomp_context` from `include/recomp.h`, with the same `#[repr(C)]` layout.

/// A 64-bit general purpose register (`gpr` in `recomp.h`).
pub type Gpr = u64;

/// A floating point register (`fpr` in `recomp.h`).
///
/// The C type is a union of `double`, two `float`s, two `uint32_t`s and a `uint64_t`. All of
/// those are views of the same 8 bytes, so this stores the raw bits and converts on access,
/// which keeps the layout identical without needing `unsafe` to read a union field.
/// The low half (`fl`, `u32l`) is the first 4 bytes, as on the little-endian hosts that
/// recompiled code requires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct Fpr(pub u64);

impl Fpr {
    pub fn d(self) -> f64 {
        f64::from_bits(self.0)
    }
    pub fn set_d(&mut self, value: f64) {
        self.0 = value.to_bits();
    }
    pub fn fl(self) -> f32 {
        f32::from_bits(self.u32l())
    }
    pub fn set_fl(&mut self, value: f32) {
        self.set_u32l(value.to_bits());
    }
    pub fn fh(self) -> f32 {
        f32::from_bits(self.u32h())
    }
    pub fn set_fh(&mut self, value: f32) {
        self.set_u32h(value.to_bits());
    }
    pub fn u32l(self) -> u32 {
        self.0 as u32
    }
    pub fn set_u32l(&mut self, value: u32) {
        self.0 = (self.0 & 0xFFFF_FFFF_0000_0000) | u64::from(value);
    }
    pub fn u32h(self) -> u32 {
        (self.0 >> 32) as u32
    }
    pub fn set_u32h(&mut self, value: u32) {
        self.0 = (self.0 & 0x0000_0000_FFFF_FFFF) | (u64::from(value) << 32);
    }
}

/// The CPU state passed to every recompiled function (`recomp_context` in `recomp.h`).
///
/// `r` and `f` are arrays here; in C they are 32 consecutive named fields (`r0`..`r31`,
/// `f0`..`f31`), which has the same layout.
#[derive(Debug)]
#[repr(C)]
pub struct RecompContext {
    pub r: [Gpr; 32],
    pub f: [Fpr; 32],
    pub hi: u64,
    pub lo: u64,
    /// Points at the high halves of the even float registers when MIPS3 float mode is off.
    /// Owned by the runtime and never dereferenced from Rust; public only so its layout
    /// can be checked against `recomp.h`.
    pub f_odd: *mut u32,
    pub status_reg: u32,
    pub mips3_float_mode: u8,
}

impl Default for RecompContext {
    fn default() -> Self {
        Self {
            r: [0; 32],
            f: [Fpr(0); 32],
            hi: 0,
            lo: 0,
            f_odd: std::ptr::null_mut(),
            status_reg: 0,
            mips3_float_mode: 0,
        }
    }
}

/// MIPS register numbers for the o32 calling convention used by N64 code.
pub mod reg {
    pub const ZERO: usize = 0;
    pub const V0: usize = 2;
    pub const V1: usize = 3;
    pub const A0: usize = 4;
    pub const A1: usize = 5;
    pub const A2: usize = 6;
    pub const A3: usize = 7;
    pub const SP: usize = 29;
    pub const RA: usize = 31;
}

impl RecompContext {
    /// Argument `index` (0-3) as passed in `$a0`-`$a3`.
    ///
    /// Arguments past the fourth live on the guest stack; read those through
    /// [`crate::GuestMemory`] at `$sp + 0x10 + 4 * index`.
    pub fn arg_u32(&self, index: usize) -> u32 {
        assert!(
            index < 4,
            "register argument index {index} out of range (0-3)"
        );
        self.r[reg::A0 + index] as u32
    }

    pub fn arg_i32(&self, index: usize) -> i32 {
        self.arg_u32(index) as i32
    }

    /// A 32-bit return value in `$v0`, sign-extended like every 32-bit MIPS result.
    pub fn set_return_u32(&mut self, value: u32) {
        self.r[reg::V0] = value as i32 as i64 as u64;
    }

    pub fn set_return_i32(&mut self, value: i32) {
        self.set_return_u32(value as u32);
    }

    /// A 64-bit return value: high word in `$v0`, low word in `$v1` (o32 convention).
    pub fn set_return_u64(&mut self, value: u64) {
        self.set_return_u32((value >> 32) as u32);
        self.r[reg::V1] = (value as u32) as i32 as i64 as u64;
    }

    /// A `float` return value in `$f0`.
    pub fn set_return_f32(&mut self, value: f32) {
        self.f[0].set_fl(value);
    }
}
