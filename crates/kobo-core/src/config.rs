//! Per-user configuration.
//!
//! The vanilla ROM never lives inside a project, so the path to it comes
//! from the user's environment: the `KOBO_SMW_ROM` variable first, then the
//! `roms.smw` key in the user config file. Tools are found the same way:
//! Asar's library from `KOBO_ASAR_LIB`, then `tools.asar`.

use std::env;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

pub const ROM_ENV_VAR: &str = "KOBO_SMW_ROM";
pub const ASAR_ENV_VAR: &str = "KOBO_ASAR_LIB";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error(
        "no vanilla SMW ROM configured; set {ROM_ENV_VAR} or add `roms.smw` to {}",
        config_path().map(|p| p.display().to_string()).unwrap_or_default()
    )]
    NoVanillaRom,
    #[error(
        "no Asar library configured; set {ASAR_ENV_VAR} or add `tools.asar` to {}",
        config_path().map(|p| p.display().to_string()).unwrap_or_default()
    )]
    NoAsar,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub roms: Roms,
    #[serde(default)]
    pub tools: Tools,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roms {
    /// Path to the vanilla SMW ROM.
    pub smw: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tools {
    /// Path to Asar's shared library: `libasar.so`, `libasar.dylib`, or
    /// `asar.dll`.
    pub asar: Option<PathBuf>,
}

/// Location of the user config file, if a config directory exists on this
/// platform.
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("kobo").join("config.toml"))
}

/// Loads the user config. A missing file yields the default config.
pub fn load() -> Result<Config, ConfigError> {
    let Some(path) = config_path() else {
        return Ok(Config::default());
    };
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(source) => return Err(ConfigError::Io { path, source }),
    };
    toml::from_str(&text).map_err(|source| ConfigError::Parse { path, source })
}

/// Resolves the path to the vanilla SMW ROM.
pub fn vanilla_rom_path() -> Result<PathBuf, ConfigError> {
    if let Some(p) = env::var_os(ROM_ENV_VAR).filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    load()?.roms.smw.ok_or(ConfigError::NoVanillaRom)
}

/// Resolves the path to Asar's shared library.
pub fn asar_library_path() -> Result<PathBuf, ConfigError> {
    if let Some(p) = env::var_os(ASAR_ENV_VAR).filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    load()?.tools.asar.ok_or(ConfigError::NoAsar)
}
