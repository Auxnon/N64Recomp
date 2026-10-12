//! `std::filesystem::path` operations whose results differ from `std::path`.
//!
//! For example, `path("").replace_extension(".z64")` is `".z64"` in C++, while Rust's
//! `set_extension` refuses to add an extension to a path without a file name. These follow
//! libstdc++ on POSIX, where `/` is the only separator.

/// `path.filename()`: everything after the last `/` (empty for a trailing `/`).
pub fn filename(path: &str) -> &str {
    path.rfind('/').map_or(path, |i| &path[i + 1..])
}

/// Where `filename`'s extension starts, if it has one. `.` and `..` have none, and a
/// leading dot (`.bashrc`) doesn't start an extension.
fn extension_start(filename: &str) -> Option<usize> {
    if filename == "." || filename == ".." {
        return None;
    }
    filename.rfind('.').filter(|&i| i > 0)
}

/// `path.stem()`: the file name without its extension.
pub fn stem(path: &str) -> &str {
    let name = filename(path);
    extension_start(name).map_or(name, |i| &name[..i])
}

/// `path.replace_extension(replacement)`: removes the extension (if any), then appends
/// `replacement`, adding a dot if it doesn't start with one.
pub fn replace_extension(path: &str, replacement: &str) -> String {
    let name_start = path.len() - filename(path).len();
    let mut out = match extension_start(filename(path)) {
        Some(i) => path[..name_start + i].to_owned(),
        None => path.to_owned(),
    };
    if !replacement.is_empty() && !replacement.starts_with('.') {
        out.push('.');
    }
    out.push_str(replacement);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_std_filesystem() {
        assert_eq!(replace_extension("", ".z64"), ".z64");
        assert_eq!(
            replace_extension(filename("dir/patches.elf"), ".z64"),
            "patches.z64"
        );
        assert_eq!(
            replace_extension(stem("patches/patches.elf"), ".c"),
            "patches.c"
        );
        assert_eq!(replace_extension(stem("a/b.c.elf"), ".c"), "b.c");
        assert_eq!(
            replace_extension("RecompiledFuncs/func.name.c", ".tmp"),
            "RecompiledFuncs/func.name.tmp"
        );
        assert_eq!(replace_extension("x/.hidden", "tmp"), "x/.hidden.tmp");
        assert_eq!(replace_extension("x/", ".tmp"), "x/.tmp");
        assert_eq!(stem("x/.."), "..");
        assert_eq!(stem("x/noext"), "noext");
    }
}
