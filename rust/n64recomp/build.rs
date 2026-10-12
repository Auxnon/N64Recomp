//! Generates the reimplemented/ignored/renamed function lists from `src/symbol_lists.cpp`,
//! so the Rust tool always uses the same lists as the C++ one.

use std::fmt::Write as _;
use std::path::PathBuf;

const LISTS: [&str; 3] = ["reimplemented_funcs", "ignored_funcs", "renamed_funcs"];

/// The string literals between `N64Recomp::<name> {` and the closing `};`, skipping comments.
fn list_entries(source: &str, name: &str) -> Vec<String> {
    let start = source
        .find(&format!("N64Recomp::{name} {{"))
        .unwrap_or_else(|| panic!("{name} not found in symbol_lists.cpp"));
    let body = &source[start..];
    let body = &body[..body.find("};").expect("unterminated list")];
    body.lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .flat_map(|line| {
            line.split('"')
                .skip(1)
                .step_by(2)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn main() {
    let source_path = "../../src/symbol_lists.cpp";
    println!("cargo:rerun-if-changed={source_path}");
    let source = std::fs::read_to_string(source_path).expect("read symbol_lists.cpp");

    let mut out = String::new();
    for name in LISTS {
        let entries = list_entries(&source, name);
        assert!(!entries.is_empty(), "{name} is empty");
        let _ = writeln!(out, "pub const {}: &[&str] = &[", name.to_uppercase());
        for entry in entries {
            let _ = writeln!(out, "    {entry:?},");
        }
        out.push_str("];\n");
    }
    let out_path = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("symbol_lists.rs");
    std::fs::write(out_path, out).expect("write symbol_lists.rs");
}
