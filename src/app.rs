//! Application entry point logic, separated from `main` so it can be tested
//! end to end with in-memory output and a fake environment.
//!
//! Exit codes: 0 success, 1 runtime or configuration error, 2 usage error.

use crate::cli::{self, Command, Global, ReposArgs};
use crate::changes;
use crate::config::{self, Config};
use crate::discovery;
use crate::render::{self, RenderOptions};
use crate::repo;
use crate::store::Store;
use crate::timefmt;
use crate::voice::Voice;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Everything ARES reads from the process environment.
#[derive(Debug, Clone)]
pub struct Env {
    pub home: Option<PathBuf>,
    pub xdg_config_home: Option<String>,
    pub xdg_data_home: Option<String>,
    pub cwd: PathBuf,
    pub now_secs: u64,
}

impl Env {
    pub fn from_process() -> Env {
        Env {
            home: env::var_os("HOME").map(PathBuf::from),
            xdg_config_home: env::var("XDG_CONFIG_HOME").ok(),
            xdg_data_home: env::var("XDG_DATA_HOME").ok(),
            cwd: env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            now_secs: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }
}

pub fn run(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let inv = match cli::parse(args) {
        Ok(i) => i,
        Err(e) => {
            let _ = writeln!(err, "ares: {e}");
            let _ = writeln!(err, "Try 'ares --help'.");
            return 2;
        }
    };

    match inv.command {
        Command::Help => {
            let _ = write!(out, "{}", cli::help_text());
            0
        }
        Command::Version => {
            let _ = writeln!(out, "ares {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Command::Config => cmd_config(&inv.global, env, out, err),
        Command::Repos(r) => cmd_repos(&inv.global, &r, env, out, err),
        Command::Status(r) => cmd_status(&inv.global, &r, env, out, err),
        Command::Scan(r) => cmd_scan(&inv.global, &r, env, out, err),
        Command::Changes => cmd_changes(&inv.global, env, out, err),
    }
}

/// Load config, honoring an explicit `--config` (which must exist) or the
/// default location (which may be absent, giving defaults).
fn load_config(global: &Global, env: &Env) -> Result<(Config, Option<PathBuf>, bool), String> {
    let (path, explicit) = match &global.config_path {
        Some(p) => (Some(abs(p, &env.cwd)), true),
        None => (
            config::config_path_from(env.xdg_config_home.as_deref(), env.home.as_deref().and_then(Path::to_str)),
            false,
        ),
    };
    match path {
        None => Ok((Config::default(), None, false)),
        Some(p) => {
            let exists = p.exists();
            if explicit && !exists {
                return Err(format!("config file not found: {}", p.display()));
            }
            let cfg = Config::load(&p, env.home.as_deref()).map_err(|e| format!("config error: {e}"))?;
            Ok((cfg, Some(p), exists))
        }
    }
}

fn abs(p: &Path, cwd: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

fn resolve_voice(global: &Global, cfg: &Config) -> Result<Voice, String> {
    let name = global.voice.as_deref().unwrap_or(&cfg.ui.voice);
    Voice::from_name(name).ok_or_else(|| {
        format!("unknown voice '{}'. Available voices: {}", name, Voice::names())
    })
}

fn cmd_config(global: &Global, env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let (cfg, path, exists) = match load_config(global, env) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(err, "ares: {e}");
            return 1;
        }
    };
    let file = match (&path, exists) {
        (Some(p), true) => p.display().to_string(),
        (Some(p), false) => format!("{} (not found, using defaults)", p.display()),
        (None, _) => "(none, HOME is not set; using defaults)".to_string(),
    };
    let roots = if cfg.scan.roots.is_empty() {
        "(none)".to_string()
    } else {
        cfg.scan
            .roots
            .iter()
            .map(|r| r.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let _ = writeln!(out, "config file:        {file}");
    let _ = writeln!(out, "ui.voice:           {}", cfg.ui.voice);
    let _ = writeln!(out, "scan.roots:         {roots}");
    let _ = writeln!(out, "scan.max_depth:     {}", cfg.scan.max_depth);
    let _ = writeln!(out, "scan.exclude_dirs:  {}", cfg.scan.exclude_dirs.join(", "));
    let _ = writeln!(out, "git.timeout_secs:   {}", cfg.git.timeout_secs);
    let _ = writeln!(out, "git.max_parallel:   {}", cfg.git.max_parallel);
    0
}

/// Everything gathered by one scan, ready to render.
struct Scan {
    voice: Voice,
    reports: Vec<repo::RepoReport>,
    errors: Vec<(PathBuf, String)>,
}

/// Load config, resolve the voice and roots, discover, and inspect.
/// On failure, writes the message to `err` and returns the exit code.
fn scan(global: &Global, args: &ReposArgs, env: &Env, err: &mut dyn Write) -> Result<Scan, i32> {
    let (cfg, path, _) = match load_config(global, env) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(err, "ares: {e}");
            return Err(1);
        }
    };
    let voice = match resolve_voice(global, &cfg) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(err, "ares: {e}");
            return Err(2);
        }
    };

    let roots: Vec<PathBuf> = if args.roots.is_empty() {
        cfg.scan.roots.clone()
    } else {
        args.roots.iter().map(|r| abs(r, &env.cwd)).collect()
    };
    if roots.is_empty() {
        let _ = writeln!(err, "ares: no scan roots configured.");
        let _ = writeln!(err, "Pass a directory, for example: ares repos ~/code");
        let where_ = path
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "your config file".to_string());
        let _ = writeln!(err, "Or add roots to {where_}:");
        let _ = writeln!(err);
        let _ = writeln!(err, "[scan]");
        let _ = writeln!(err, "roots = [\"~/code\"]");
        return Err(2);
    }

    let found = discovery::discover(&roots, &cfg.scan_options());
    let reports = repo::inspect_all(&found.repos, cfg.git_timeout(), cfg.git.max_parallel);
    Ok(Scan {
        voice,
        reports,
        errors: found.errors,
    })
}

fn cmd_repos(
    global: &Global,
    args: &ReposArgs,
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let scan = match scan(global, args, env, err) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let opts = RenderOptions {
        voice: scan.voice,
        day: env.now_secs / 86_400,
        dirty_only: args.dirty_only,
    };
    let _ = write!(out, "{}", render::repos(&scan.reports, &scan.errors, &opts));
    0
}

fn cmd_status(
    global: &Global,
    args: &ReposArgs,
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let scan = match scan(global, args, env, err) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let opts = RenderOptions {
        voice: scan.voice,
        day: env.now_secs / 86_400,
        dirty_only: false,
    };
    let _ = write!(out, "{}", render::status(&scan.reports, &scan.errors, &opts));
    0
}

fn cmd_scan(
    global: &Global,
    args: &ReposArgs,
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let scan = match scan(global, args, env, err) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let opts = RenderOptions {
        voice: scan.voice,
        day: env.now_secs / 86_400,
        dirty_only: false,
    };
    let _ = write!(out, "{}", render::status(&scan.reports, &scan.errors, &opts));

    let db = match config::data_path_from(
        env.xdg_data_home.as_deref(),
        env.home.as_deref().and_then(Path::to_str),
    ) {
        Some(p) => p,
        None => {
            let _ = writeln!(err, "ares: cannot record the scan: HOME is not set.");
            return 1;
        }
    };

    let recorded = Store::open(&db).and_then(|mut store| {
        let previous = store.latest_scan()?;
        let record = store.record_scan(env.now_secs, &scan.reports)?;
        Ok((previous, record))
    });
    match recorded {
        Ok((previous, record)) => {
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "Recorded scan #{} ({}) in {}",
                record.id,
                plural_repos(record.repo_count),
                db.display()
            );
            match previous {
                Some(p) => {
                    let _ = writeln!(
                        out,
                        "Previous scan: {}, {} ({})",
                        plural_repos(p.repo_count),
                        timefmt::ago(env.now_secs, p.started_at),
                        timefmt::format_utc(p.started_at)
                    );
                }
                None => {
                    let _ = writeln!(out, "This is the first recorded scan.");
                }
            }
            0
        }
        Err(e) => {
            let _ = writeln!(err, "ares: could not record the scan: {e}");
            1
        }
    }
}

fn plural_repos(n: usize) -> String {
    format!("{} {}", n, if n == 1 { "repository" } else { "repositories" })
}

fn cmd_changes(global: &Global, env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let (cfg, _, _) = match load_config(global, env) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(err, "ares: {e}");
            return 1;
        }
    };
    let voice = match resolve_voice(global, &cfg) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(err, "ares: {e}");
            return 2;
        }
    };
    let db = match config::data_path_from(
        env.xdg_data_home.as_deref(),
        env.home.as_deref().and_then(Path::to_str),
    ) {
        Some(p) => p,
        None => {
            let _ = writeln!(err, "ares: cannot find the scan history: HOME is not set.");
            return 1;
        }
    };

    // Read-only: looking at history must never create a database.
    let store = match Store::open_existing(&db) {
        Ok(Some(s)) => s,
        Ok(None) => {
            let _ = writeln!(out, "No scan history yet. Run `ares scan` first.");
            return 0;
        }
        Err(e) => {
            let _ = writeln!(err, "ares: could not read the scan history: {e}");
            return 1;
        }
    };

    let loaded = store.recent_scans(2).and_then(|scans| {
        if scans.len() < 2 {
            return Ok(None);
        }
        let current = store.scan_reports(scans[0].id)?;
        let previous = store.scan_reports(scans[1].id)?;
        Ok(Some((scans, previous, current)))
    });
    match loaded {
        Err(e) => {
            let _ = writeln!(err, "ares: could not read the scan history: {e}");
            1
        }
        Ok(None) => {
            let _ = writeln!(
                out,
                "Only one scan is recorded so far. Run `ares scan` again later to compare."
            );
            0
        }
        Ok(Some((scans, previous, current))) => {
            let found = changes::diff(&previous, &current);
            let opts = RenderOptions {
                voice,
                day: env.now_secs / 86_400,
                dirty_only: false,
            };
            let _ = write!(
                out,
                "{}",
                render::changes(&scans[1], &scans[0], &found, env.now_secs, &opts)
            );
            0
        }
    }
}
