//! toml++'s lookups and conversions, so configs are read exactly as the C++ reads them.

use std::path::{Path, PathBuf};

use toml::{Table, Value};

/// A config error, reported like toml++'s `parse_error`.
#[derive(Debug)]
pub struct TomlError(pub String);

impl std::fmt::Display for TomlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn err<T>(msg: impl Into<String>) -> Result<T, TomlError> {
    Err(TomlError(msg.into()))
}

/// Parses a file the way `toml::parse_file` does, failing on I/O or syntax errors.
pub fn parse_file(path: &Path) -> Result<Table, TomlError> {
    let text =
        std::fs::read_to_string(path).map_err(|e| TomlError(format!("{}: {e}", path.display())))?;
    text.parse::<Table>()
        .map_err(|e| TomlError(format!("{}: {e}", path.display())))
}

/// `node["key"]` on a value that may not be a table.
pub fn get<'a>(value: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    value?.as_table()?.get(key)
}

/// `value<uint32_t>()`: an integer that fits.
pub fn u32_of(value: Option<&Value>) -> Option<u32> {
    value?.as_integer().and_then(|i| u32::try_from(i).ok())
}

pub fn i32_of(value: Option<&Value>) -> Option<i32> {
    value?.as_integer().and_then(|i| i32::try_from(i).ok())
}

pub fn u64_of(value: Option<&Value>) -> Option<u64> {
    value?.as_integer().and_then(|i| u64::try_from(i).ok())
}

/// `value<bool>()`: toml++ also converts integers (nonzero is true).
pub fn bool_of(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Boolean(b) => Some(*b),
        Value::Integer(i) => Some(*i != 0),
        _ => None,
    }
}

pub fn string_of(value: Option<&Value>) -> Option<String> {
    value?.as_str().map(str::to_owned)
}

pub fn array_of(value: Option<&Value>) -> Option<&Vec<Value>> {
    value?.as_array()
}

/// `concat_if_not_empty`: paths in a config are relative to the config's directory.
pub fn concat_if_not_empty(parent: &Path, child: &str) -> PathBuf {
    if child.is_empty() {
        PathBuf::new()
    } else {
        parent.join(child)
    }
}
