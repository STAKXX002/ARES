//! Turn config file text into a validated [`Config`].
//!
//! Unknown sections and keys are errors, so a typo like `root` instead of
//! `roots` is reported instead of silently ignored.

use super::toml_lite::{self, Entry, Value};
use super::{Config, MAX_AI_TIMEOUT_SECS, MAX_PARALLEL_LIMIT, MAX_STALE_DAYS, MIN_AI_TIMEOUT_SECS};
use crate::ai::ollama::valid_model_name;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(n) => write!(f, "line {}: {}", n, self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

impl Error for ConfigError {}

fn cerr(entry: &Entry, message: impl Into<String>) -> ConfigError {
    ConfigError {
        line: Some(entry.line),
        message: message.into(),
    }
}

fn expand_tilde(s: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    if s == "~" || s.starts_with("~/") {
        let home = home.ok_or_else(|| "cannot expand '~' because HOME is not set".to_string())?;
        Ok(if s == "~" {
            home.to_path_buf()
        } else {
            home.join(&s[2..])
        })
    } else {
        Ok(PathBuf::from(s))
    }
}

fn want_int(entry: &Entry, min: i64, max: i64) -> Result<i64, ConfigError> {
    match &entry.value {
        Value::Int(n) if (min..=max).contains(n) => Ok(*n),
        Value::Int(_) => Err(cerr(entry, format!("{} must be between {} and {}", entry.key, min, max))),
        _ => Err(cerr(entry, format!("{} must be an integer", entry.key))),
    }
}

fn want_bool(entry: &Entry) -> Result<bool, ConfigError> {
    match &entry.value {
        Value::Bool(b) => Ok(*b),
        _ => Err(cerr(
            entry,
            format!("{} must be true or false (without quotes)", entry.key),
        )),
    }
}

fn want_str_array<'a>(entry: &'a Entry) -> Result<&'a Vec<String>, ConfigError> {
    match &entry.value {
        Value::StrArray(v) => Ok(v),
        _ => Err(cerr(entry, format!("{} must be an array of strings", entry.key))),
    }
}

impl Config {
    /// Parse and validate config text. `home` is used to expand a leading `~`.
    pub fn from_toml(text: &str, home: Option<&Path>) -> Result<Config, ConfigError> {
        let entries = toml_lite::parse(text).map_err(|e| ConfigError {
            line: Some(e.line),
            message: e.message,
        })?;

        let mut cfg = Config::default();
        for e in &entries {
            match (e.section.as_str(), e.key.as_str()) {
                ("scan", "roots") => {
                    let mut roots = Vec::new();
                    for raw in want_str_array(e)? {
                        if raw.is_empty() {
                            return Err(cerr(e, "roots must not contain empty paths"));
                        }
                        let p = expand_tilde(raw, home).map_err(|m| cerr(e, m))?;
                        if !p.is_absolute() {
                            return Err(cerr(
                                e,
                                format!("root {raw:?} must be an absolute path or start with '~/'"),
                            ));
                        }
                        roots.push(p);
                    }
                    cfg.scan.roots = roots;
                }
                ("scan", "max_depth") => cfg.scan.max_depth = want_int(e, 0, 32)? as usize,
                ("scan", "exclude_dirs") => {
                    let v = want_str_array(e)?;
                    if let Some(bad) = v.iter().find(|n| n.is_empty() || n.contains('/')) {
                        return Err(cerr(
                            e,
                            format!("exclude_dirs entry {bad:?} must be a plain directory name"),
                        ));
                    }
                    cfg.scan.exclude_dirs = v.clone();
                }
                ("git", "timeout_secs") => cfg.git.timeout_secs = want_int(e, 1, 300)? as u64,
                ("git", "max_parallel") => {
                    cfg.git.max_parallel = want_int(e, 1, MAX_PARALLEL_LIMIT as i64)? as usize
                }
                ("stale", "days") => cfg.stale.days = want_int(e, 1, MAX_STALE_DAYS as i64)? as u64,
                ("ai", "model") => match &e.value {
                    Value::Str(s) if s.is_empty() || valid_model_name(s) => {
                        cfg.ai.model = s.clone()
                    }
                    Value::Str(s) => {
                        return Err(cerr(e, format!("{s:?} is not a valid model name")))
                    }
                    _ => return Err(cerr(e, "model must be a string")),
                },
                ("ai", "ollama_port") => cfg.ai.ollama_port = want_int(e, 1, 65535)? as u16,
                ("ai", "timeout_secs") => {
                    cfg.ai.timeout_secs =
                        want_int(e, MIN_AI_TIMEOUT_SECS as i64, MAX_AI_TIMEOUT_SECS as i64)? as u64
                }
                ("ai", "unload_after") => cfg.ai.unload_after = want_bool(e)?,
                ("ui", "voice") => match &e.value {
                    Value::Str(s) if !s.is_empty() => cfg.ui.voice = s.clone(),
                    _ => return Err(cerr(e, "voice must be a non-empty string")),
                },
                ("", key) => {
                    return Err(cerr(e, format!("key {key:?} must be inside a [section]")))
                }
                (section, key) => {
                    return Err(cerr(e, format!("unknown setting [{section}] {key}")))
                }
            }
        }
        Ok(cfg)
    }

    /// Load config from a file. A missing file is not an error: it yields
    /// the defaults. Any other problem is reported with the file path.
    pub fn load(path: &Path, home: Option<&Path>) -> Result<Config, ConfigError> {
        match fs::read_to_string(path) {
            Ok(text) => Config::from_toml(&text, home).map_err(|e| ConfigError {
                line: e.line,
                message: format!("{}: {}", path.display(), e.message),
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(ConfigError {
                line: None,
                message: format!("cannot read {}: {}", path.display(), e),
            }),
        }
    }
}
