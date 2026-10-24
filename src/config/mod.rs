//! Configuration: types, defaults, and file location.

use crate::discovery;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Voice used when none is configured. Neutral, no flavor text.
pub const DEFAULT_VOICE: &str = "plain";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub scan: ScanConfig,
    pub git: GitConfig,
    pub ui: UiConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanConfig {
    /// Directories to search for repositories. Empty by default: ARES does
    /// not look through your home directory unless you tell it where to.
    pub roots: Vec<PathBuf>,
    pub max_depth: usize,
    pub exclude_dirs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitConfig {
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiConfig {
    /// Name of the voice used for human-readable output.
    pub voice: String,
}

impl Default for Config {
    fn default() -> Self {
        let d = discovery::Options::default();
        Config {
            scan: ScanConfig {
                roots: Vec::new(),
                max_depth: d.max_depth,
                exclude_dirs: d.exclude_dirs,
            },
            git: GitConfig { timeout_secs: 10 },
            ui: UiConfig {
                voice: DEFAULT_VOICE.to_string(),
            },
        }
    }
}

impl Config {
    pub fn scan_options(&self) -> discovery::Options {
        discovery::Options {
            max_depth: self.scan.max_depth,
            exclude_dirs: self.scan.exclude_dirs.clone(),
        }
    }

    pub fn git_timeout(&self) -> Duration {
        Duration::from_secs(self.git.timeout_secs)
    }
}

/// Resolve the config file location from environment values.
///
/// Follows the XDG convention: `$XDG_CONFIG_HOME/ares/config.toml` when that
/// variable is set to an absolute path, else `~/.config/ares/config.toml`.
pub fn config_path_from(xdg_config_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    match xdg_config_home {
        Some(x) if !x.is_empty() && Path::new(x).is_absolute() => {
            Some(PathBuf::from(x).join("ares").join("config.toml"))
        }
        _ => home
            .filter(|h| !h.is_empty())
            .map(|h| PathBuf::from(h).join(".config").join("ares").join("config.toml")),
    }
}

/// Config file location for the current process environment.
pub fn default_config_path() -> Option<PathBuf> {
    config_path_from(
        env::var("XDG_CONFIG_HOME").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
    )
}

pub mod toml_lite;

#[cfg(test)]
mod tests;
