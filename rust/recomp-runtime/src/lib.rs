//! Rust replacements for N64ModernRuntime (librecomp/ultramodern) functions.
//!
//! Built as a static library and linked into a game's C++ build with `RECOMP_RUST_RUNTIME`
//! defined, which removes the C++ definitions this crate replaces. Modules move here one at
//! a time (docs/rust-port-plan.md, phase 5); everything else stays in C++ until then.
//!
//! Besides these exports, the library carries `recomp_abi_install_guest_memory`, which the
//! C++ runtime calls once RDRAM is mapped.

#![deny(unsafe_code)]

pub mod cxx;
pub mod eep;
pub mod flash;
pub mod math;
pub mod pak;
pub mod pi;
pub mod print;
pub mod saving;
pub mod stubs;
pub mod ultra;

/// The fast3d renderer's C entry points (`recomp_render_fast3d_*`), linked into the same
/// static library so the game links one copy of Rust's std.
#[cfg(feature = "fast3d")]
pub use recomp_render;
