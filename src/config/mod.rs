//! Configuration: types, defaults, and file location.

use crate::discovery;
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
    /// How many repositories are inspected at the same time.
    pub max_parallel: usize,
}

/// Largest allowed value for `git.max_parallel`.
pub const MAX_PARALLEL_LIMIT: usize = 16;

/// Default worker count for a machine with `cores` hardware threads: one
/// worker per thread, but never fewer than 2 or more than 8. Inspection is
/// dominated by starting `git` processes, so more workers help until the
/// machine runs out of threads, and a modest cap keeps load polite.
pub fn default_parallelism(cores: usize) -> usize {
    cores.clamp(2, 8)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiConfig {
    /// Name of the voice used for human-readable output.
    pub voice: String,
}

impl Default for Config {
    fn default() -> Self {
        let d = discovery::Options::default();
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        Config {
            scan: ScanConfig {
                roots: Vec::new(),
                max_depth: d.max_depth,
                exclude_dirs: d.exclude_dirs,
            },
            git: GitConfig {
                timeout_secs: 10,
                max_parallel: default_parallelism(cores),
            },
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

/// Database location: `$XDG_DATA_HOME/ares/ares.db` when that variable is an
/// absolute path, else `~/.local/share/ares/ares.db`.
pub fn data_path_from(xdg_data_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    match xdg_data_home {
        Some(x) if !x.is_empty() && Path::new(x).is_absolute() => {
            Some(PathBuf::from(x).join("ares").join("ares.db"))
        }
        _ => home.filter(|h| !h.is_empty()).map(|h| {
            PathBuf::from(h)
                .join(".local")
                .join("share")
                .join("ares")
                .join("ares.db")
        }),
    }
}

pub mod load;
pub mod toml_lite;

#[cfg(test)]
mod load_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod toml_lite_tests;
