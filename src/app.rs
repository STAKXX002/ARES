//! Application entry point logic, separated from `main` so it can be tested
//! end to end with in-memory output and a fake environment.
//!
//! Exit codes: 0 success, 1 runtime or configuration error, 2 usage error.

use crate::ai::ask as ai_ask;
use crate::ai::ollama::{Ollama, Settings as OllamaSettings};
use crate::ai::payload;
use crate::cli::{self, AskArgs, Command, Global, ReposArgs};
use crate::changes;
use crate::config::{self, Config};
use crate::discovery;
use crate::facts;
use crate::gh::activity::Activity;
use crate::gh;
use crate::render::{self, RenderOptions};
use crate::github::RepoRef;
use crate::repo::{self, Remote};
use crate::stale;
use crate::store::Store;
use crate::timefmt;
use crate::voice::Voice;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Everything ARES reads from the process environment.
#[derive(Debug, Clone)]
pub struct Env {
    pub home: Option<PathBuf>,
    pub xdg_config_home: Option<String>,
    pub xdg_data_home: Option<String>,
    pub cwd: PathBuf,
    pub now_secs: u64,
    /// The GitHub CLI program. Always `gh` in real use; tests substitute a fake.
    pub gh_program: PathBuf,
}

impl Env {
    pub fn from_process() -> Env {
        Env {
            home: env::var_os("HOME").map(PathBuf::from),
            xdg_config_home: env::var("XDG_CONFIG_HOME").ok(),
            xdg_data_home: env::var("XDG_DATA_HOME").ok(),
            cwd: env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            gh_program: PathBuf::from("gh"),
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
        Command::Github(r) => cmd_github(&inv.global, &r, env, out, err),
        Command::Stale(r) => cmd_stale(&inv.global, &r, env, out, err),
        Command::Ask(a) => cmd_ask(&inv.global, &a, env, out, err),
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
    let _ = writeln!(out, "stale.days:         {}", cfg.stale.days);
    let model = if cfg.ai.model.is_empty() {
        "(none chosen)"
    } else {
        cfg.ai.model.as_str()
    };
    let _ = writeln!(out, "ai.model:           {model}");
    let _ = writeln!(out, "ai.ollama_port:     {}", cfg.ai.ollama_port);
    let _ = writeln!(out, "ai.timeout_secs:    {}", cfg.ai.timeout_secs);
    let _ = writeln!(out, "ai.unload_after:    {}", cfg.ai.unload_after);
    0
}

/// Everything gathered by one scan, ready to render.
struct Scan {
    voice: Voice,
    reports: Vec<repo::RepoReport>,
    errors: Vec<(PathBuf, String)>,
}

/// The result of loading config and discovering repositories.
struct Prepared {
    cfg: Config,
    voice: Voice,
    found: discovery::Discovery,
}

/// Load config, resolve the voice and roots, and discover repositories.
/// On failure, writes the message to `err` and returns the exit code.
fn prepare(
    global: &Global,
    args: &ReposArgs,
    env: &Env,
    err: &mut dyn Write,
) -> Result<Prepared, i32> {
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
    Ok(Prepared { cfg, voice, found })
}

/// Discover and inspect the working-tree state of every repository.
fn scan(global: &Global, args: &ReposArgs, env: &Env, err: &mut dyn Write) -> Result<Scan, i32> {
    let p = prepare(global, args, env, err)?;
    let reports = repo::inspect_all(&p.found.repos, p.cfg.git_timeout(), p.cfg.git.max_parallel);
    Ok(Scan {
        voice: p.voice,
        reports,
        errors: p.found.errors,
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

/// Per-request time allowed for one `gh` call.
const GH_TIMEOUT: Duration = Duration::from_secs(20);

/// Ask GitHub about every repository whose `origin` is on github.com.
///
/// The result lines up with `reports`: `None` for repositories that are not
/// on GitHub.
fn online_activity(
    p: &Prepared,
    reports: &[repo::RemoteReport],
    env: &Env,
) -> Vec<Option<Result<Activity, String>>> {
    let on_github: Vec<(PathBuf, RepoRef)> = reports
        .iter()
        .filter_map(|r| match &r.remote {
            Ok(Remote::GitHub(g)) => Some((r.path.clone(), g.clone())),
            _ => None,
        })
        .collect();
    // What the local clones last saw of origin, so GitHub's tip can be
    // compared with it. Local only: nothing here touches the network.
    let timeout = p.cfg.git_timeout();
    let local = repo::par_map(&on_github, p.cfg.git.max_parallel, |(path, _)| {
        repo::origin_refs(path, timeout).ok()
    });
    let targets: Vec<gh::fetch::Target> = on_github
        .into_iter()
        .zip(local)
        .map(|((_, repo), origin_refs)| gh::fetch::Target { repo, origin_refs })
        .collect();
    let mut fetched = gh::fetch::fetch_all(&env.gh_program, &targets, GH_TIMEOUT, p.cfg.git.max_parallel)
        .into_iter();
    let online: Vec<_> = reports
        .iter()
        .map(|r| match &r.remote {
            Ok(Remote::GitHub(_)) => fetched.next(),
            _ => None,
        })
        .collect();
    online
}

fn cmd_github(
    global: &Global,
    args: &ReposArgs,
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let p = match prepare(global, args, env, err) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let reports = repo::remote_reports(&p.found.repos, p.cfg.git_timeout(), p.cfg.git.max_parallel);

    if !args.online {
        let _ = write!(out, "{}", render::github(&reports, &p.found.errors));
        return 0;
    }

    let online = online_activity(&p, &reports, env);
    let _ = write!(
        out,
        "{}",
        render::github_online(&reports, &online, &p.found.errors, env.now_secs)
    );
    0
}

fn cmd_stale(
    global: &Global,
    args: &ReposArgs,
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let p = match prepare(global, args, env, err) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let days = args.days.unwrap_or(p.cfg.stale.days);
    let timeout = p.cfg.git_timeout();
    let reports = repo::inspect_all(&p.found.repos, timeout, p.cfg.git.max_parallel);
    let last = repo::par_map(&p.found.repos, p.cfg.git.max_parallel, |path| {
        repo::last_commit_time(path, timeout)
    });
    let paired: Vec<_> = reports.into_iter().zip(last).collect();
    let report = stale::classify(&paired, env.now_secs, days);
    let opts = RenderOptions {
        voice: p.voice,
        day: env.now_secs / 86_400,
        dirty_only: false,
    };
    let _ = write!(out, "{}", render::stale(&report, &p.found.errors, &opts));
    0
}

fn cmd_ask(global: &Global, args: &AskArgs, env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let scan_args = ReposArgs {
        roots: args.roots.clone(),
        ..ReposArgs::default()
    };
    let p = match prepare(global, &scan_args, env, err) {
        Ok(p) => p,
        Err(code) => return code,
    };

    // Settle the model before scanning or touching the network, so a missing
    // model fails fast.
    let model = args.model.clone().unwrap_or_else(|| p.cfg.ai.model.clone());
    if !args.dry_run && model.is_empty() {
        let _ = writeln!(err, "ares: no model is chosen.");
        let _ = writeln!(err, "Set `model` under [ai] in the config file, or pass --model NAME.");
        let _ = writeln!(err, "`ollama list` shows the models installed on this machine.");
        return 1;
    }

    let timeout = p.cfg.git_timeout();
    let workers = p.cfg.git.max_parallel;

    let reports = repo::inspect_all(&p.found.repos, timeout, workers);
    let last = repo::par_map(&p.found.repos, workers, |path| repo::last_commit_time(path, timeout));
    let paired: Vec<_> = reports.iter().cloned().zip(last).collect();
    let stale_report = stale::classify(&paired, env.now_secs, p.cfg.stale.days);

    let online = if args.online {
        let remotes = repo::remote_reports(&p.found.repos, timeout, workers);
        Some(online_activity(&p, &remotes, env))
    } else {
        None
    };

    let facts = facts::build(&reports, &stale_report, online.as_deref());
    let opts = payload::Options { alias: args.alias };
    let payload = payload::build(args.question.as_deref().unwrap_or(""), &facts, &opts);

    if args.dry_run {
        print_dry_run(args, &facts, &payload, out);
        return 0;
    }

    let ollama = match Ollama::new(OllamaSettings {
        model,
        port: p.cfg.ai.ollama_port,
        timeout: Duration::from_secs(p.cfg.ai.timeout_secs),
        unload_after: p.cfg.ai.unload_after,
    }) {
        Ok(o) => o,
        Err(e) => {
            let _ = writeln!(err, "ares: {e}");
            return 1;
        }
    };
    let _ = writeln!(
        err,
        "ares: asking {} on this machine (waits up to {} seconds)...",
        ollama.model(),
        p.cfg.ai.timeout_secs
    );
    match ai_ask::ask(&payload.text, &facts, args.alias, &ollama) {
        Ok(ai_ask::Outcome::Answered { shown, rejected }) => {
            let _ = write!(
                out,
                "{}",
                ai_ask::render_answer(&shown, &rejected, &facts, args.alias, ollama.model())
            );
            0
        }
        Ok(ai_ask::Outcome::Unusable { reason, rejected }) => {
            let _ = writeln!(err, "ares: the model's answer could not be used: {reason}.");
            if !rejected.is_empty() {
                let _ = writeln!(err, "ares: {} statement(s) failed the checks.", rejected.len());
            }
            let _ = writeln!(err, "ares: showing the measured facts instead.");
            let _ = write!(out, "{}", ai_ask::render_facts(&facts));
            0
        }
        Err(e) => {
            let _ = writeln!(err, "ares: {e}");
            let _ = writeln!(err, "Without a model, `ares status` and `ares stale` show the same measured facts.");
            1
        }
    }
}

fn print_dry_run(args: &AskArgs, facts: &facts::Facts, payload: &payload::Payload, out: &mut dyn Write) {
    let _ = writeln!(out, "Dry run: nothing was sent anywhere. Below is exactly what a model would receive.");
    let _ = writeln!(out);
    let _ = writeln!(out, "Facts:             {} included, {} left out", payload.included, payload.omitted);
    let _ = writeln!(
        out,
        "Size:              {} bytes, about {} tokens (an estimate)",
        payload.bytes(),
        payload.estimated_tokens()
    );
    let _ = writeln!(
        out,
        "Repository names:  {}",
        if args.alias { "replaced with R1, R2, ..." } else { "real names" }
    );
    let _ = writeln!(
        out,
        "GitHub data:       {}",
        if args.online {
            "included (fetched through gh)"
        } else {
            "not included (add --online to include it)"
        }
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "----- payload begins -----");
    let _ = write!(out, "{}", payload.text);
    let _ = writeln!(out, "----- payload ends -----");

    if args.alias && !facts.names.is_empty() {
        const SHOWN: usize = 50;
        let _ = writeln!(out);
        let _ = writeln!(out, "Alias key (this stays on your machine and is never part of the payload):");
        for (i, name) in facts.names.iter().take(SHOWN).enumerate() {
            let _ = writeln!(out, "  R{} = {}", i + 1, name);
        }
        if facts.names.len() > SHOWN {
            let _ = writeln!(out, "  ... and {} more", facts.names.len() - SHOWN);
        }
    }
}
