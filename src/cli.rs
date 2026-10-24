//! Command-line argument parsing. Pure and dependency-free.
//!
//! Global options (`--config`, `--voice`, `-h`, `-V`) are accepted anywhere
//! on the line. The first remaining word is the command.

use crate::voice::Voice;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Config,
    Repos(ReposArgs),
    Status(ReposArgs),
    Scan(ReposArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReposArgs {
    /// Only list repositories with uncommitted changes.
    pub dirty_only: bool,
    /// Roots given on the command line. They replace the configured roots.
    pub roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Global {
    pub config_path: Option<PathBuf>,
    pub voice: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub global: Global,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError(pub String);

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for CliError {}

fn cli_err(msg: impl Into<String>) -> CliError {
    CliError(msg.into())
}

pub const COMMANDS: &[&str] = &["repos", "status", "scan", "config", "help", "version"];

/// Take the value of `--name VALUE` or `--name=VALUE`.
fn take_value(
    arg: &str,
    name: &str,
    args: &[String],
    i: &mut usize,
) -> Result<Option<String>, CliError> {
    if arg == name {
        *i += 1;
        let v = args
            .get(*i)
            .ok_or_else(|| cli_err(format!("{name} needs a value")))?;
        Ok(Some(v.clone()))
    } else if let Some(v) = arg.strip_prefix(&format!("{name}=")) {
        Ok(Some(v.to_string()))
    } else {
        Ok(None)
    }
}

pub fn parse(args: &[String]) -> Result<Invocation, CliError> {
    let mut global = Global::default();
    let mut help = false;
    let mut version = false;
    let mut rest: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "-h" || a == "--help" {
            help = true;
        } else if a == "-V" || a == "--version" {
            version = true;
        } else if let Some(v) = take_value(a, "--config", args, &mut i)? {
            if v.is_empty() {
                return Err(cli_err("--config needs a non-empty path"));
            }
            global.config_path = Some(PathBuf::from(v));
        } else if let Some(v) = take_value(a, "--voice", args, &mut i)? {
            global.voice = Some(v);
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }

    let command = if help {
        Command::Help
    } else if version {
        Command::Version
    } else if rest.is_empty() {
        Command::Help
    } else {
        match rest[0].as_str() {
            "help" => Command::Help,
            "version" => Command::Version,
            "config" => {
                if rest.len() > 1 {
                    return Err(cli_err("'config' takes no arguments"));
                }
                Command::Config
            }
            "repos" => Command::Repos(parse_scan_args(&rest[1..], "repos", true)?),
            "status" => Command::Status(parse_scan_args(&rest[1..], "status", false)?),
            "scan" => Command::Scan(parse_scan_args(&rest[1..], "scan", false)?),
            other => {
                return Err(cli_err(format!(
                    "unknown command '{}'. Available commands: {}",
                    other,
                    COMMANDS.join(", ")
                )))
            }
        }
    };

    Ok(Invocation { global, command })
}

/// Shared by `repos` and `status`. `--dirty` is only accepted when allowed.
fn parse_scan_args(
    args: &[String],
    command: &str,
    allow_dirty: bool,
) -> Result<ReposArgs, CliError> {
    let mut out = ReposArgs::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--dirty" && allow_dirty {
            out.dirty_only = true;
        } else if let Some(v) = take_value(a, "--root", args, &mut i)? {
            if v.is_empty() {
                return Err(cli_err("--root needs a non-empty path"));
            }
            out.roots.push(PathBuf::from(v));
        } else if a.starts_with('-') && a.len() > 1 {
            return Err(cli_err(format!("unknown option '{a}' for '{command}'")));
        } else {
            // A bare path is treated as a root, so `ares repos ~/code` works.
            out.roots.push(PathBuf::from(a));
        }
        i += 1;
    }
    Ok(out)
}

pub fn help_text() -> String {
    format!(
        "ares {version}
Autonomous Runtime and Environment Supervisor

USAGE:
    ares [OPTIONS] <COMMAND>

COMMANDS:
    repos [--dirty] [--root PATH]... [PATH]...
                    List git repositories and their state
    status [--root PATH]... [PATH]...
                    Overview of which repositories need attention
    scan [--root PATH]... [PATH]...
                    Like status, and also record the scan in ares's own database
    config          Show the config file location and effective settings
    help            Show this help
    version         Show the version

OPTIONS:
    --config PATH   Use this config file instead of the default
    --voice NAME    Wording style for reports: {voices} (default: plain)
    -h, --help      Show this help
    -V, --version   Show the version

ares never modifies your repositories or files. Only `scan` writes, and only
to its own database.
",
        version = env!("CARGO_PKG_VERSION"),
        voices = Voice::names()
    )
}
