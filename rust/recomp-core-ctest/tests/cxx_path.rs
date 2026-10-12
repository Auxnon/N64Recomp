//! cxx_path matches libstdc++'s std::filesystem::path.

use recomp_core::cxx_path::{filename, replace_extension, stem};
use recomp_core_ctest::path_op;

const PATHS: &[&str] = &[
    "",
    ".",
    "..",
    "/",
    "a",
    "a.b",
    "a.b.c",
    ".hidden",
    ".hidden.c",
    "..x",
    "x.",
    "x..",
    "dir/",
    "dir/a.elf",
    "dir/.hidden",
    "dir/..",
    "dir/.",
    "/abs/path.toml",
    "a/b.c/d",
    "a/b.c/d.e",
    "patches/patches.elf",
    "RecompiledFuncs/func.name.c",
    "dir//x.y",
    "a/b/",
];
const EXTS: &[&str] = &["", ".c", "c", ".tmp", ".z64", "..x", ".a.b"];

#[test]
fn filename_and_stem() {
    for path in PATHS {
        assert_eq!(filename(path), path_op(0, path, ""), "filename({path:?})");
        assert_eq!(stem(path), path_op(1, path, ""), "stem({path:?})");
    }
}

#[test]
fn replace_extension_matches() {
    for path in PATHS {
        for ext in EXTS {
            assert_eq!(
                replace_extension(path, ext),
                path_op(2, path, ext),
                "{path:?}.replace_extension({ext:?})"
            );
        }
    }
}
