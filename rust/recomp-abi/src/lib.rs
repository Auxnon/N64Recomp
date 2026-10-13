//! The Rust side of the N64Recomp ABI (`include/recomp.h`).
//!
//! - [`RecompContext`]: `recomp_context` with the same layout.
//! - [`GuestMemory`]: RDRAM with the `MEM_*` / `LWL` / `SD` semantics of `recomp.h`.
//! - [`recomp_export!`]: define functions recompiled C code can call, with safe bodies.
//! - [`GameFn`]: call recompiled code from Rust.
//!
//! This crate holds the unsafe code for the whole Rust port, and every block is marked
//! `#[allow(unsafe_code)]` so it can be found by searching. The list is in
//! `docs/rust-port-plan.md`, under "The unsafe budget".

#![deny(unsafe_code)]

mod context;
pub mod export;
mod memory;

pub use context::{Fpr, Gpr, RecompContext, reg};
pub use export::{GameFn, RecompFunc, guest_memory, install_guest_memory};
pub use memory::{GuestMemory, RDRAM_BASE, sign_extend};
