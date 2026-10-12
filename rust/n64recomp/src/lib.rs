//! N64Recomp ported to Rust: statically recompiles N64 MIPS code into C.
//!
//! This follows the C++ tool closely and writes byte-identical output, which
//! `rust/golden/golden.sh` checks. Where the C++ output depends on libstdc++ (unordered
//! container order, `std::sort` order, `std::filesystem` path rules, `{fmt}` hex), the
//! port uses `recomp-core`'s equivalents.

#![deny(unsafe_code)]

pub mod analysis;
pub mod config;
pub mod context;
pub mod elf;
pub mod generator;
pub mod instr;
pub mod mdebug;
pub mod operations;
pub mod recompilation;
pub mod symbol_file;
pub mod tomlx;

/// The reimplemented, ignored and renamed function lists, generated from
/// `src/symbol_lists.cpp` by `build.rs`.
pub mod symbol_lists {
    include!(concat!(env!("OUT_DIR"), "/symbol_lists.rs"));
}
