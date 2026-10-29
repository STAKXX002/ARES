//! Command-line argument parsing. Pure and dependency-free.
//!
//! Global options (`--config`, `--voice`, `-h`, `-V`) are accepted anywhere
//! on the line. The first remaining word is the command.

use crate::ai::ollama::valid_model_name;
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
    Changes,
    Github(ReposArgs),
    Stale(ReposArgs),
    Ask(AskArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReposArgs {
    /// Only list repositories with uncommitted changes.
    pub dirty_only: bool,
    /// Roots given on the command line. They replace the configured roots.
    pub roots: Vec<PathBuf>,
    /// Ask GitHub for pull requests, issues, and CI (`github` only).
    pub online: bool,
    /// Quiet threshold in days (`stale` only). Overrides the configured value.
    pub days: Option<u64>,
}

/// Arguments of `ares ask`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AskArgs {
    /// The question. `None` asks the default question.
    pub question: Option<String>,
    /// Show exactly what a model would receive, and send nothing.
    pub dry_run: bool,
    /// Include GitHub facts (this queries GitHub through `gh`).
    pub online: bool,
    /// Replace repository names with `R1`, `R2`, ... in the payload.
    pub alias: bool,
    /// Ollama model to use instead of `[ai] model` from the config file.
    pub model: Option<String>,
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

pub const COMMANDS: &[&str] = &[
    "repos", "status", "scan", "changes", "github", "stale", "ask", "config", "help", "version",
];

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
            "changes" => {
                if rest.len() > 1 {
                    return Err(cli_err("'changes' takes no arguments"));
                }
                Command::Changes
            }
            "repos" => Command::Repos(parse_scan_args(&rest[1..], "repos", Allowed { dirty: true, ..Allowed::NONE })?),
            "status" => Command::Status(parse_scan_args(&rest[1..], "status", Allowed::NONE)?),
            "scan" => Command::Scan(parse_scan_args(&rest[1..], "scan", Allowed::NONE)?),
            "stale" => Command::Stale(parse_scan_args(&rest[1..], "stale", Allowed { days: true, ..Allowed::NONE })?),
            "ask" => Command::Ask(parse_ask(&rest[1..])?),
            "github" => Command::Github(parse_scan_args(&rest[1..], "github", Allowed { online: true, ..Allowed::NONE })?),
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

fn parse_ask(args: &[String]) -> Result<AskArgs, CliError> {
    let mut out = AskArgs::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--dry-run" {
            out.dry_run = true;
        } else if a == "--online" {
            out.online = true;
        } else if a == "--alias" {
            out.alias = true;
        } else if let Some(v) = take_value(a, "--model", args, &mut i)? {
            if !valid_model_name(&v) {
                return Err(cli_err(format!("--model {v:?} is not a valid model name")));
            }
            out.model = Some(v);
        } else if let Some(v) = take_value(a, "--root", args, &mut i)? {
            if v.is_empty() {
                return Err(cli_err("--root needs a non-empty path"));
            }
            out.roots.push(PathBuf::from(v));
        } else if a.starts_with('-') && a.len() > 1 {
            return Err(cli_err(format!("unknown option '{a}' for 'ask'")));
        } else if out.question.is_some() {
            return Err(cli_err("ask takes a single question; put it in quotes"));
        } else {
            out.question = Some(a.to_string());
        }
        i += 1;
    }
    Ok(out)
}

/// Which optional flags a scan-style command accepts.
#[derive(Clone, Copy)]
struct Allowed {
    dirty: bool,
    online: bool,
    days: bool,
}

impl Allowed {
    const NONE: Allowed = Allowed {
        dirty: false,
        online: false,
        days: false,
    };
}

/// Shared by `repos`, `status`, `scan`, and `github`.
fn parse_scan_args(
    args: &[String],
    command: &str,
    allowed: Allowed,
) -> Result<ReposArgs, CliError> {
    let mut out = ReposArgs::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--dirty" && allowed.dirty {
            out.dirty_only = true;
        } else if a == "--online" && allowed.online {
            out.online = true;
        } else if let Some(v) = allowed.days.then(|| take_value(a, "--days", args, &mut i)).transpose()?.flatten() {
            out.days = Some(
                v.parse::<u64>()
                    .ok()
                    .filter(|d| (1..=3650).contains(d))
                    .ok_or_else(|| cli_err("--days must be a whole number from 1 to 3650"))?,
            );
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
    changes         Compare the last two recorded scans
    ask [QUESTION] [--model NAME] [--dry-run] [--online] [--alias] [--root PATH]...
                    Ask a model on this machine (Ollama) about your projects.
                    Only measured facts are sent, and answers are checked
                    against them. --dry-run prints exactly what the model would
                    be sent, and sends nothing
    stale [--days N] [--root PATH]... [PATH]...
                    List repositories with no commit in N days (default 90)
    github [--online] [--root PATH]... [PATH]...
                    Show which repositories are on GitHub. Offline by default
                    (local git only); --online adds pull requests, issues,
                    and CI status through your `gh` login
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
