//! End-to-end tests of `app::run` using real git repositories.

use crate::app::{run, Env};
use crate::testutil::{git, TempDir};
use std::fs;
use std::path::{Path, PathBuf};

const DAY: u64 = 20_000;

struct Sandbox {
    root: TempDir,
}

impl Sandbox {
    fn new() -> Sandbox {
        let s = Sandbox { root: TempDir::new() };
        fs::create_dir_all(s.home()).unwrap();
        fs::create_dir_all(s.code()).unwrap();
        s
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn code(&self) -> PathBuf {
        self.root.path().join("code")
    }

    fn xdg(&self) -> PathBuf {
        self.root.path().join("xdg")
    }

    fn env(&self) -> Env {
        Env {
            home: Some(self.home()),
            xdg_config_home: Some(self.xdg().to_string_lossy().into_owned()),
            cwd: self.root.path().to_path_buf(),
            now_secs: DAY * 86_400,
        }
    }

    fn write_config(&self, text: &str) {
        let dir = self.xdg().join("ares");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), text).unwrap();
    }

    /// Create a committed repository under `code/`.
    fn repo(&self, name: &str) -> PathBuf {
        let dir = self.code().join(name);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        fs::write(dir.join("f.txt"), "one\n").unwrap();
        git(&dir, &["add", "f.txt"]);
        git(&dir, &["commit", "-q", "-m", "init"]);
        dir
    }
}

fn go(args: &[&str], env: &Env) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(&args, env, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn code_arg(s: &Sandbox) -> String {
    s.code().to_string_lossy().into_owned()
}

#[test]
fn help_and_version_succeed_with_clean_stderr() {
    let s = Sandbox::new();
    let (c, out, err) = go(&[], &s.env());
    assert_eq!(c, 0);
    assert!(out.contains("USAGE:"));
    assert!(err.is_empty());

    let (c, out, _) = go(&["--version"], &s.env());
    assert_eq!(c, 0);
    assert!(out.starts_with("ares "));
}

#[test]
fn usage_errors_exit_2_and_write_only_to_stderr() {
    let s = Sandbox::new();
    let (c, out, err) = go(&["frobnicate"], &s.env());
    assert_eq!(c, 2);
    assert!(out.is_empty());
    assert!(err.contains("unknown command 'frobnicate'"));
    assert!(err.contains("ares --help"));
}

#[test]
fn repos_lists_repositories_from_a_cli_root() {
    let s = Sandbox::new();
    s.repo("alpha");
    s.repo("beta");
    let (c, out, err) = go(&["repos", &code_arg(&s)], &s.env());
    assert_eq!(c, 0, "{err}");
    assert!(err.is_empty());
    assert!(out.starts_with("2 repositories: 0 with uncommitted changes, 0 with unpushed commits."));
    assert!(out.contains("alpha"));
    assert!(out.contains("beta"));
}

#[test]
fn dirty_flag_lists_only_repos_with_uncommitted_changes() {
    let s = Sandbox::new();
    s.repo("tidy");
    let messy = s.repo("messy");
    fs::write(messy.join("scratch.txt"), "x\n").unwrap();
    let (c, out, _) = go(&["repos", "--dirty", &code_arg(&s)], &s.env());
    assert_eq!(c, 0);
    assert!(out.contains("messy"));
    assert!(out.contains("1 changed (0 staged, 0 unstaged, 1 untracked)"));
    assert!(!out.contains("tidy "));
}

#[test]
fn configured_roots_are_used_when_none_are_given() {
    let s = Sandbox::new();
    s.repo("fromconfig");
    s.write_config(&format!("[scan]\nroots = [\"{}\"]\n", s.code().display()));
    let (c, out, err) = go(&["repos"], &s.env());
    assert_eq!(c, 0, "{err}");
    assert!(out.contains("fromconfig"));
}

#[test]
fn tilde_in_config_expands_to_home() {
    let s = Sandbox::new();
    fs::create_dir_all(s.home().join("proj")).unwrap();
    let dir = s.home().join("proj/inhome");
    fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    s.write_config("[scan]\nroots = [\"~/proj\"]\n");
    let (c, out, err) = go(&["repos"], &s.env());
    assert_eq!(c, 0, "{err}");
    assert!(out.contains("inhome"));
}

#[test]
fn relative_cli_root_resolves_against_cwd() {
    let s = Sandbox::new();
    s.repo("rel");
    let (c, out, _) = go(&["repos", "code"], &s.env());
    assert_eq!(c, 0);
    assert!(out.contains("rel"));
}

#[test]
fn no_roots_is_a_usage_error_with_guidance() {
    let s = Sandbox::new();
    let (c, out, err) = go(&["repos"], &s.env());
    assert_eq!(c, 2);
    assert!(out.is_empty());
    assert!(err.contains("no scan roots configured"));
    assert!(err.contains("[scan]"));
    assert!(err.contains(&s.xdg().join("ares/config.toml").display().to_string()));
}

#[test]
fn unknown_voice_is_rejected_and_lists_choices() {
    let s = Sandbox::new();
    let (c, _, err) = go(&["--voice", "sassy", "repos", &code_arg(&s)], &s.env());
    assert_eq!(c, 2);
    assert!(err.contains("unknown voice 'sassy'"));
    assert!(err.contains("plain, cheerful, deadpan"));
}

#[test]
fn voice_comes_from_config_and_flag_overrides_it() {
    let s = Sandbox::new();
    s.repo("alpha");
    s.write_config("[ui]\nvoice = \"deadpan\"\n");

    let (_, from_config, _) = go(&["repos", &code_arg(&s)], &s.env());
    assert!(from_config.contains("1 repos"), "deadpan wording expected: {from_config}");
    assert!(!from_config.starts_with("1 repository:"));

    let (_, overridden, _) = go(&["--voice", "plain", "repos", &code_arg(&s)], &s.env());
    assert!(overridden.starts_with("1 repository:"));
}

#[test]
fn default_voice_is_plain_without_any_config() {
    let s = Sandbox::new();
    s.repo("alpha");
    let (_, out, _) = go(&["repos", &code_arg(&s)], &s.env());
    assert!(out.starts_with("1 repository:"));
}

#[test]
fn explicit_missing_config_file_is_an_error() {
    let s = Sandbox::new();
    let (c, out, err) = go(&["--config", "nope.toml", "repos", &code_arg(&s)], &s.env());
    assert_eq!(c, 1);
    assert!(out.is_empty());
    assert!(err.contains("config file not found"));
}

#[test]
fn invalid_config_reports_line_and_exits_1() {
    let s = Sandbox::new();
    s.write_config("[scan]\nrootz = [\"/x\"]\n");
    let (c, _, err) = go(&["repos", &code_arg(&s)], &s.env());
    assert_eq!(c, 1);
    assert!(err.contains("config error"));
    assert!(err.contains("line 2"));
    assert!(err.contains("unknown setting"));
}

#[test]
fn config_command_shows_effective_settings() {
    let s = Sandbox::new();
    let (c, out, _) = go(&["config"], &s.env());
    assert_eq!(c, 0);
    assert!(out.contains("not found, using defaults"));
    assert!(out.contains("ui.voice:           plain"));
    assert!(out.contains("scan.roots:         (none)"));

    s.write_config("[ui]\nvoice = \"cheerful\"\n[git]\ntimeout_secs = 7\n");
    let (_, out, _) = go(&["config"], &s.env());
    assert!(!out.contains("not found"));
    assert!(out.contains("ui.voice:           cheerful"));
    assert!(out.contains("git.timeout_secs:   7"));
}

#[test]
fn unreadable_repo_is_reported_without_aborting_the_scan() {
    let s = Sandbox::new();
    s.repo("good");
    fs::create_dir_all(s.code().join("fake/.git")).unwrap();
    let (c, out, _) = go(&["repos", &code_arg(&s)], &s.env());
    assert_eq!(c, 0);
    assert!(out.contains("good"));
    assert!(out.contains("fake"));
    assert!(out.contains("ERROR"));
    assert!(out.contains("1 could not be read."));
}

#[test]
fn running_repos_never_modifies_a_repository() {
    let s = Sandbox::new();
    let repo = s.repo("guarded");
    // Make the index's stat data stale, so a careless `git status` would rewrite it.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(repo.join("f.txt"), "one\n").unwrap();
    fs::write(repo.join("untracked.txt"), "x\n").unwrap();

    let snapshot = |p: &Path| -> Vec<(String, Vec<u8>)> {
        let mut files = Vec::new();
        let mut stack = vec![p.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(&d).unwrap() {
                let e = e.unwrap();
                let path = e.path();
                if e.file_type().unwrap().is_dir() {
                    stack.push(path);
                } else {
                    files.push((
                        path.strip_prefix(p).unwrap().to_string_lossy().into_owned(),
                        fs::read(&path).unwrap(),
                    ));
                }
            }
        }
        files.sort();
        files
    };

    let before = snapshot(&repo);
    for voice in ["plain", "cheerful", "deadpan"] {
        let (c, _, _) = go(&["--voice", voice, "repos", &code_arg(&s)], &s.env());
        assert_eq!(c, 0);
    }
    let after = snapshot(&repo);
    assert_eq!(before, after, "ares must leave repositories byte-for-byte unchanged");
}
