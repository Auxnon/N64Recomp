//! The RSPRecomp config file (`read_config` in `RSPRecomp/src/rsp_recomp.cpp`).

use std::fmt;
use std::path::{Path, PathBuf};

use recomp_core::cxx_hash::CxxHashSet;
use toml::{Table, Value};

pub struct OverlayConfig {
    pub offset: u32,
    pub size: u32,
}

pub struct OverlaySlotConfig {
    pub text_address: u32,
    pub overlays: Vec<OverlayConfig>,
}

pub struct Config {
    pub text_offset: u32,
    pub text_size: u32,
    pub text_address: u32,
    pub rom_file_path: PathBuf,
    pub output_file_path: PathBuf,
    pub output_function_name: String,
    pub extra_indirect_branch_targets: Vec<u32>,
    pub unsupported_instructions: CxxHashSet<u32>,
    pub overlay_slots: Vec<OverlaySlotConfig>,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Toml(toml::de::Error),
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "{e}"),
            ConfigError::Toml(e) => write!(f, "{e}"),
            ConfigError::Invalid(msg) => f.write_str(msg),
        }
    }
}

/// toml++'s `node.value<uint32_t>()`: an integer that fits, otherwise nothing.
fn get_u32(table: &Table, key: &str) -> Option<u32> {
    table
        .get(key)?
        .as_integer()
        .and_then(|i| u32::try_from(i).ok())
}

fn require_u32(table: &Table, key: &str, context: &str) -> Result<u32, ConfigError> {
    get_u32(table, key)
        .ok_or_else(|| ConfigError::Invalid(format!("Missing {key} in config file{context}")))
}

fn require_str<'a>(table: &'a Table, key: &str) -> Result<&'a str, ConfigError> {
    table
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ConfigError::Invalid(format!("Missing {key} in config file")))
}

/// `concat_if_not_empty`: paths in the config are relative to the config's directory.
fn relative_to(basedir: &Path, child: &str) -> PathBuf {
    if child.is_empty() {
        PathBuf::new()
    } else {
        basedir.join(child)
    }
}

/// `toml_to_vec<uint32_t>`: the integer elements, truncated to 32 bits like the C++
/// conversion from `int64_t`; other elements are skipped.
fn integers(array: &[Value]) -> impl Iterator<Item = u32> + '_ {
    array.iter().filter_map(Value::as_integer).map(|i| i as u32)
}

pub fn read_config(config_path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(config_path).map_err(ConfigError::Io)?;
    let data: Table = text.parse().map_err(ConfigError::Toml)?;
    let basedir = config_path.parent().unwrap_or(Path::new(""));

    let text_offset = require_u32(&data, "text_offset", "")?;
    let text_size = require_u32(&data, "text_size", "")?;
    let text_address = require_u32(&data, "text_address", "")?;
    let rom_file_path = relative_to(basedir, require_str(&data, "rom_file_path")?);
    let output_file_path = relative_to(basedir, require_str(&data, "output_file_path")?);
    let output_function_name = require_str(&data, "output_function_name")?.to_owned();

    let extra_indirect_branch_targets = match data.get("extra_indirect_branch_targets") {
        Some(Value::Array(array)) => integers(array).collect(),
        _ => Vec::new(),
    };
    let unsupported_instructions = match data.get("unsupported_instructions") {
        Some(Value::Array(array)) => integers(array).collect(),
        _ => CxxHashSet::new(),
    };

    let mut overlay_slots = Vec::new();
    if let Some(Value::Array(slots)) = data.get("overlay_slots") {
        // toml++'s for_each with a `toml::table` callback only visits (and counts) tables.
        for (slot_idx, slot) in slots.iter().filter_map(Value::as_table).enumerate() {
            let at_slot = format!(" at overlay slot {slot_idx}");
            let text_address = require_u32(slot, "text_address", &at_slot)?;
            let Some(Value::Array(overlays)) = slot.get("overlays") else {
                return Err(ConfigError::Invalid(format!(
                    "Missing overlays in config file{at_slot}"
                )));
            };
            let mut slot_config = OverlaySlotConfig {
                text_address,
                overlays: Vec::new(),
            };
            for (overlay_idx, overlay) in overlays.iter().filter_map(Value::as_table).enumerate() {
                let at_overlay = format!("{at_slot} overlay {overlay_idx}");
                let offset = require_u32(overlay, "offset", &at_overlay)?;
                let size = require_u32(overlay, "size", &at_overlay)?;
                if size % 4 != 0 {
                    return Err(ConfigError::Invalid(format!(
                        "Overlay size must be a multiple of 4 in config file{at_overlay}"
                    )));
                }
                slot_config.overlays.push(OverlayConfig { offset, size });
            }
            overlay_slots.push(slot_config);
        }
    }

    Ok(Config {
        text_offset,
        text_size,
        text_address,
        rom_file_path,
        output_file_path,
        output_function_name,
        extra_indirect_branch_targets,
        unsupported_instructions,
        overlay_slots,
    })
}
