//! End-to-end tests of `app::run` using real git repositories.

use crate::app::{run, Env};
use crate::store::Store;
use crate::testutil::{git, FakeGh, Route, TempDir};
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

    fn db(&self) -> PathBuf {
        self.root.path().join("data/ares/ares.db")
    }

    fn xdg(&self) -> PathBuf {
        self.root.path().join("xdg")
    }

    fn env(&self) -> Env {
        Env {
            home: Some(self.home()),
            xdg_config_home: Some(self.xdg().to_string_lossy().into_owned()),
            xdg_data_home: Some(self.root.path().join("data").to_string_lossy().into_owned()),
            cwd: self.root.path().to_path_buf(),
            now_secs: DAY * 86_400,
            gh_program: PathBuf::from("/nonexistent/gh"),
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
    assert!(out.contains("git.max_parallel:   "));
    assert!(out.contains("stale.days:         90"));
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
    let (c, _, err) = go(&["scan", &code_arg(&s)], &s.env());
    assert_eq!(c, 0, "{err}");
    let after = snapshot(&repo);
    assert_eq!(before, after, "ares must leave repositories byte-for-byte unchanged");
}

#[test]
fn output_is_identical_for_any_parallelism() {
    let s = Sandbox::new();
    for i in 0..9 {
        let r = s.repo(&format!("p{i}"));
        if i % 2 == 0 {
            fs::write(r.join("loose.txt"), "x\n").unwrap();
        }
    }
    let mut outputs = Vec::new();
    for n in [1, 3, 16] {
        s.write_config(&format!("[git]\nmax_parallel = {n}\n"));
        let (c, out, err) = go(&["repos", &code_arg(&s)], &s.env());
        assert_eq!(c, 0, "{err}");
        outputs.push(out);
    }
    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(outputs[1], outputs[2]);
}

#[test]
fn status_summarizes_what_needs_attention() {
    let s = Sandbox::new();
    s.repo("tidy");
    let messy = s.repo("messy");
    fs::write(messy.join("a.txt"), "x\n").unwrap();
    fs::write(messy.join("b.txt"), "x\n").unwrap();
    let (c, out, err) = go(&["status", &code_arg(&s)], &s.env());
    assert_eq!(c, 0, "{err}");
    assert!(out.starts_with("2 repositories: 1 with uncommitted changes, 0 with unpushed commits."));
    assert!(out.contains("uncommitted: messy (2)"));
    assert!(!out.contains("tidy"));
    assert!(err.is_empty());
}

#[test]
fn status_without_roots_is_a_usage_error() {
    let s = Sandbox::new();
    let (c, out, err) = go(&["status"], &s.env());
    assert_eq!(c, 2);
    assert!(out.is_empty());
    assert!(err.contains("no scan roots configured"));
}

#[test]
fn status_uses_configured_roots_and_voice() {
    let s = Sandbox::new();
    s.repo("only");
    s.write_config(&format!(
        "[scan]\nroots = [\"{}\"]\n[ui]\nvoice = \"cheerful\"\n",
        s.code().display()
    ));
    let (c, out, _) = go(&["status"], &s.env());
    assert_eq!(c, 0);
    assert!(!out.starts_with("1 repository:"), "cheerful wording expected: {out}");
}

#[test]
fn first_scan_is_recorded_in_a_private_database() {
    let s = Sandbox::new();
    s.repo("a");
    s.repo("b");
    let (c, out, err) = go(&["scan", &code_arg(&s)], &s.env());
    assert_eq!(c, 0, "{err}");
    assert!(err.is_empty());
    assert!(out.starts_with("2 repositories:"));
    assert!(out.contains(&format!("Recorded scan #1 (2 repositories) in {}", s.db().display())));
    assert!(out.contains("This is the first recorded scan."));

    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(s.db()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    let store = Store::open(&s.db()).unwrap();
    let rec = store.latest_scan().unwrap().unwrap();
    assert_eq!((rec.id, rec.repo_count, rec.started_at), (1, 2, DAY * 86_400));
}

#[test]
fn second_scan_reports_the_previous_one() {
    let s = Sandbox::new();
    s.repo("a");
    let env = s.env();
    go(&["scan", &code_arg(&s)], &env);

    let mut later = env.clone();
    later.now_secs += 2 * 3_600;
    let (c, out, _) = go(&["scan", &code_arg(&s)], &later);
    assert_eq!(c, 0);
    assert!(out.contains("Recorded scan #2 (1 repository)"));
    assert!(out.contains("Previous scan: 1 repository, 2 hours ago (2024-10-04 00:00 UTC)"), "{out}");
}

#[test]
fn repos_and_status_never_create_a_database() {
    let s = Sandbox::new();
    s.repo("a");
    go(&["repos", &code_arg(&s)], &s.env());
    go(&["status", &code_arg(&s)], &s.env());
    assert!(!s.db().exists());
    assert!(!s.db().parent().unwrap().exists());
}

#[test]
fn scan_without_a_data_location_still_reports_but_fails_to_record() {
    let s = Sandbox::new();
    s.repo("a");
    let mut env = s.env();
    env.home = None;
    env.xdg_data_home = None;
    env.xdg_config_home = None;
    let (c, out, err) = go(&["scan", &code_arg(&s)], &env);
    assert_eq!(c, 1);
    assert!(out.starts_with("1 repository:"));
    assert!(err.contains("HOME is not set"));
}

#[test]
fn unusable_database_location_is_reported_after_the_status() {
    let s = Sandbox::new();
    s.repo("a");
    // Make `data` a file so the database directory cannot be created.
    fs::write(s.root.path().join("data"), "in the way").unwrap();
    let (c, out, err) = go(&["scan", &code_arg(&s)], &s.env());
    assert_eq!(c, 1);
    assert!(out.starts_with("1 repository:"));
    assert!(err.contains("could not record the scan"));
}

#[test]
fn a_database_from_a_newer_ares_is_refused_and_untouched() {
    let s = Sandbox::new();
    s.repo("a");
    fs::create_dir_all(s.db().parent().unwrap()).unwrap();
    {
        let c = rusqlite::Connection::open(s.db()).unwrap();
        c.execute_batch("PRAGMA user_version = 99;").unwrap();
    }
    let (c, _, err) = go(&["scan", &code_arg(&s)], &s.env());
    assert_eq!(c, 1);
    assert!(err.contains("newer"), "{err}");
    let c = rusqlite::Connection::open(s.db()).unwrap();
    let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v, 99);
}

#[test]
fn scan_with_no_roots_is_a_usage_error_and_creates_nothing() {
    let s = Sandbox::new();
    let (c, _, err) = go(&["scan"], &s.env());
    assert_eq!(c, 2);
    assert!(err.contains("no scan roots configured"));
    assert!(!s.db().exists());
}

fn later(env: &Env, secs: u64) -> Env {
    let mut e = env.clone();
    e.now_secs += secs;
    e
}

#[test]
fn changes_without_history_explains_and_creates_nothing() {
    let s = Sandbox::new();
    let (c, out, err) = go(&["changes"], &s.env());
    assert_eq!(c, 0, "{err}");
    assert!(out.contains("No scan history yet. Run `ares scan` first."));
    assert!(!s.db().exists());
    assert!(!s.db().parent().unwrap().exists());
}

#[test]
fn changes_with_a_single_scan_asks_for_another() {
    let s = Sandbox::new();
    s.repo("a");
    go(&["scan", &code_arg(&s)], &s.env());
    let (c, out, _) = go(&["changes"], &s.env());
    assert_eq!(c, 0);
    assert!(out.contains("Only one scan is recorded so far."));
}

#[test]
fn changes_reports_what_moved_between_the_last_two_scans() {
    let s = Sandbox::new();
    let a = s.repo("a");
    s.repo("gone");
    let t0 = s.env();
    go(&["scan", &code_arg(&s)], &t0);

    fs::write(a.join("loose.txt"), "x\n").unwrap();
    fs::remove_dir_all(s.code().join("gone")).unwrap();
    s.repo("fresh");
    let t1 = later(&t0, 7_200);
    let (c, _, err) = go(&["scan", &code_arg(&s)], &t1);
    assert_eq!(c, 0, "{err}");

    let (c, out, err) = go(&["changes"], &t1);
    assert_eq!(c, 0, "{err}");
    assert!(err.is_empty());
    assert!(out.starts_with("Comparing scan #1 (2 hours ago, 2024-10-04 00:00 UTC) with scan #2 (just now, 2024-10-04 02:00 UTC)."), "{out}");
    assert!(out.contains("new:               fresh\n"), "{out}");
    assert!(out.contains("gone:              gone\n"), "{out}");
    assert!(out.contains("became dirty:      a (1 files)\n"), "{out}");
}

#[test]
fn changes_between_identical_scans_says_so() {
    let s = Sandbox::new();
    s.repo("a");
    let t0 = s.env();
    go(&["scan", &code_arg(&s)], &t0);
    go(&["scan", &code_arg(&s)], &later(&t0, 60));
    let (_, out, _) = go(&["changes"], &later(&t0, 60));
    assert!(out.contains("No changes between these scans."));
}

#[test]
fn changes_uses_only_the_two_most_recent_scans() {
    let s = Sandbox::new();
    let a = s.repo("a");
    let t0 = s.env();
    go(&["scan", &code_arg(&s)], &t0);
    fs::write(a.join("x.txt"), "x\n").unwrap();
    go(&["scan", &code_arg(&s)], &later(&t0, 100));
    go(&["scan", &code_arg(&s)], &later(&t0, 200));
    let (_, out, _) = go(&["changes"], &later(&t0, 200));
    assert!(out.starts_with("Comparing scan #2"), "{out}");
    assert!(out.contains("No changes between these scans."), "{out}");
}

#[test]
fn changes_voice_comes_from_the_flag_and_adds_only_a_remark() {
    let s = Sandbox::new();
    s.repo("a");
    let t0 = s.env();
    go(&["scan", &code_arg(&s)], &t0);
    go(&["scan", &code_arg(&s)], &later(&t0, 60));
    let (_, plain, _) = go(&["changes"], &later(&t0, 60));
    let (_, cheery, _) = go(&["--voice", "cheerful", "changes"], &later(&t0, 60));
    assert!(cheery.starts_with(plain.trim_end()));
    assert!(cheery.len() > plain.len());
}

#[test]
fn changes_never_modifies_the_database() {
    let s = Sandbox::new();
    s.repo("a");
    let t0 = s.env();
    go(&["scan", &code_arg(&s)], &t0);
    go(&["scan", &code_arg(&s)], &later(&t0, 60));
    let before = fs::read(s.db()).unwrap();
    go(&["changes"], &later(&t0, 60));
    go(&["--voice", "deadpan", "changes"], &later(&t0, 60));
    assert_eq!(before, fs::read(s.db()).unwrap());
}

#[test]
fn changes_refuses_a_newer_database_and_fails_without_home() {
    let s = Sandbox::new();
    fs::create_dir_all(s.db().parent().unwrap()).unwrap();
    {
        let c = rusqlite::Connection::open(s.db()).unwrap();
        c.execute_batch("PRAGMA user_version = 99;").unwrap();
    }
    let (c, _, err) = go(&["changes"], &s.env());
    assert_eq!(c, 1);
    assert!(err.contains("newer"), "{err}");

    let mut nohome = s.env();
    nohome.home = None;
    nohome.xdg_data_home = None;
    let (c, _, err) = go(&["changes"], &nohome);
    assert_eq!(c, 1);
    assert!(err.contains("HOME is not set"));
}

#[test]
fn changes_rejects_an_unknown_voice() {
    let s = Sandbox::new();
    let (c, _, err) = go(&["--voice", "sassy", "changes"], &s.env());
    assert_eq!(c, 2);
    assert!(err.contains("unknown voice"));
}

#[test]
fn github_lists_each_repository_by_where_its_origin_points() {
    let s = Sandbox::new();
    let a = s.repo("on-github");
    git(&a, &["remote", "add", "origin", "https://user:ghp_SECRETTOKEN@github.com/STAKXX002/on-github.git"]);
    let b = s.repo("elsewhere");
    git(&b, &["remote", "add", "origin", "https://user:SECRETPW@gitlab.com/o/elsewhere.git"]);
    s.repo("local-only");

    let (c, out, err) = go(&["github", &code_arg(&s)], &s.env());
    assert_eq!(c, 0, "{err}");
    assert!(err.is_empty());
    assert!(out.starts_with("3 repositories: 1 on GitHub, 1 with another remote, 1 with no remote.\n"), "{out}");
    assert!(out.contains("on-github   STAKXX002/on-github"));
    assert!(out.contains("elsewhere   (another remote)"));
    assert!(out.contains("local-only  (no remote)"));
    assert!(!out.contains("SECRET"), "credentials must never be printed: {out}");
    assert!(!out.contains("gitlab"), "another remote's URL must not be printed: {out}");
}

#[test]
fn github_never_modifies_repositories_or_creates_a_database() {
    let s = Sandbox::new();
    let r = s.repo("a");
    git(&r, &["remote", "add", "origin", "git@github.com:o/a.git"]);
    let config_before = fs::read(r.join(".git/config")).unwrap();
    let index_before = fs::read(r.join(".git/index")).unwrap();
    let (c, _, _) = go(&["github", &code_arg(&s)], &s.env());
    assert_eq!(c, 0);
    assert_eq!(config_before, fs::read(r.join(".git/config")).unwrap());
    assert_eq!(index_before, fs::read(r.join(".git/index")).unwrap());
    assert!(!s.db().exists());
}

#[test]
fn github_without_roots_is_a_usage_error() {
    let s = Sandbox::new();
    let (c, out, err) = go(&["github"], &s.env());
    assert_eq!(c, 2);
    assert!(out.is_empty());
    assert!(err.contains("no scan roots configured"));
}

#[test]
fn github_reports_an_unreadable_repository_without_aborting() {
    let s = Sandbox::new();
    let good = s.repo("good");
    git(&good, &["remote", "add", "origin", "git@github.com:o/good.git"]);
    fs::create_dir_all(s.code().join("fake/.git")).unwrap();
    let (c, out, _) = go(&["github", &code_arg(&s)], &s.env());
    assert_eq!(c, 0);
    assert!(out.contains("good  o/good"));
    assert!(out.contains("fake  ERROR"));
    assert!(out.contains("1 could not be read."));
}

const SECRET: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";

fn online_sandbox() -> (Sandbox, FakeGh) {
    let s = Sandbox::new();
    let a = s.repo("a");
    git(&a, &["remote", "add", "origin", &format!("https://user:{SECRET}@github.com/o/a.git")]);
    let b = s.repo("b");
    git(&b, &["remote", "add", "origin", "https://gitlab.com/o/b.git"]);
    s.repo("c");
    let gh = FakeGh::new(&[
        Route::ok(
            "repos/o/a",
            r#"{"private": false, "archived": false, "fork": false, "default_branch": "main",
                "pushed_at": "2023-11-14T22:13:20Z", "open_issues_count": 5}"#,
        ),
        Route::ok("repos/o/a/pulls*", r#"[{"number": 1}, {"number": 2}]"#),
        Route::ok(
            "repos/o/a/actions/runs*",
            r#"{"workflow_runs": [{"name": "CI", "status": "completed", "conclusion": "failure", "head_branch": "main"}]}"#,
        ),
    ]);
    (s, gh)
}

fn with_gh(s: &Sandbox, gh: &FakeGh) -> Env {
    let mut env = s.env();
    env.gh_program = gh.program.clone();
    env
}

#[test]
fn online_reports_activity_for_github_repositories_only() {
    let (s, gh) = online_sandbox();
    let (c, out, err) = go(&["github", "--online", &code_arg(&s)], &with_gh(&s, &gh));
    assert_eq!(c, 0, "{err}");
    assert!(err.is_empty());
    assert!(out.starts_with("3 repositories: 1 on GitHub, 1 with another remote, 1 with no remote.\n"), "{out}");
    assert!(out.contains("Checked 1 on GitHub: 1 with failing CI, 1 with open pull requests."), "{out}");
    assert!(out.contains("a  o/a  2 open PRs, 3 open issues, CI failing on main, main never fetched, pushed 324 days ago"), "{out}");
    assert!(out.contains("b  (another remote)"));
    assert!(out.contains("c  (no remote)"));

    // Only the one GitHub repository was ever asked about, with three GETs.
    let calls = gh.calls();
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert!(calls.iter().all(|c| c.contains("--method GET") && c.contains("repos/o/a")), "{calls:?}");
}

#[test]
fn the_token_in_the_remote_url_never_reaches_gh_or_the_output() {
    let (s, gh) = online_sandbox();
    let (_, out, err) = go(&["github", "--online", &code_arg(&s)], &with_gh(&s, &gh));
    assert!(!out.contains(SECRET) && !err.contains(SECRET));
    assert!(!out.contains("abcdefghijklmnop"));
    for call in gh.calls() {
        assert!(!call.contains(SECRET) && !call.contains("user:"), "{call}");
    }
}

#[test]
fn without_online_gh_is_never_started() {
    let (s, gh) = online_sandbox();
    let (c, out, _) = go(&["github", &code_arg(&s)], &with_gh(&s, &gh));
    assert_eq!(c, 0);
    assert!(out.contains("a  o/a"));
    assert!(!out.contains("open PRs"));
    assert!(gh.calls().is_empty(), "offline mode must not call gh: {:?}", gh.calls());
}

#[test]
fn online_is_rejected_by_other_commands() {
    let (s, _) = online_sandbox();
    for cmd in ["repos", "status", "scan"] {
        let (c, out, err) = go(&[cmd, "--online", &code_arg(&s)], &s.env());
        assert_eq!(c, 2, "{cmd}");
        assert!(out.is_empty());
        assert!(err.contains("unknown option '--online'"));
    }
}

#[test]
fn a_missing_gh_is_explained_once_and_the_listing_still_works() {
    let (s, _) = online_sandbox();
    let (c, out, _) = go(&["github", "--online", &code_arg(&s)], &s.env());
    assert_eq!(c, 0);
    assert_eq!(out.matches("is it installed").count(), 1, "{out}");
    assert!(out.contains("GitHub could not be queried:"));
    assert!(out.contains("a  o/a"));
}

#[test]
fn a_gh_that_is_not_logged_in_is_explained_once() {
    let (s, _) = online_sandbox();
    let gh = FakeGh::new(&[Route::fail(
        "repos/*",
        4,
        "To get started with GitHub CLI, please run:  gh auth login",
    )]);
    let (c, out, _) = go(&["github", "--online", &code_arg(&s)], &with_gh(&s, &gh));
    assert_eq!(c, 0);
    assert!(out.contains("GitHub could not be queried: To get started with GitHub CLI, please run: gh auth login"), "{out}");
    assert_eq!(gh.calls().len(), 1, "a failed lookup stops after its first request");
}

#[test]
fn credentials_in_gh_error_output_are_redacted_end_to_end() {
    let (s, _) = online_sandbox();
    let gh = FakeGh::new(&[Route::fail(
        "repos/*",
        1,
        &format!("> Authorization: token {SECRET}\nrequest to https://x:{SECRET}@api.github.com failed"),
    )]);
    let (c, out, err) = go(&["github", "--online", &code_arg(&s)], &with_gh(&s, &gh));
    assert_eq!(c, 0);
    assert!(!out.contains(SECRET) && !err.contains(SECRET), "{out}{err}");
    assert!(!out.contains("abcdefghijklmnop"), "{out}");
}

#[test]
fn one_repository_failing_does_not_hide_the_others() {
    let s = Sandbox::new();
    for (name, url) in [("good", "git@github.com:o/good.git"), ("gone", "git@github.com:o/gone.git")] {
        let r = s.repo(name);
        git(&r, &["remote", "add", "origin", url]);
    }
    let gh = FakeGh::new(&[
        Route::ok("repos/o/good", r#"{"open_issues_count": 1, "default_branch": "main"}"#),
        Route::ok("repos/o/good/pulls*", "[]"),
        Route::ok("repos/o/good/actions/runs*", r#"{"workflow_runs": []}"#),
        Route::fail("repos/o/gone", 1, "gh: Not Found (HTTP 404)"),
    ]);
    let (c, out, _) = go(&["github", "--online", &code_arg(&s)], &with_gh(&s, &gh));
    assert_eq!(c, 0);
    assert!(out.contains("Checked 2 on GitHub: 0 with failing CI, 0 with open pull requests, 1 could not be checked."), "{out}");
    assert!(out.contains("good  o/good  0 open PRs, 1 open issue, no CI runs"), "{out}");
    assert!(out.contains("gone  o/gone  ERROR: gh: Not Found (HTTP 404)"), "{out}");
}

#[test]
fn online_never_modifies_repositories_or_creates_a_database() {
    let (s, gh) = online_sandbox();
    let a = s.code().join("a");
    let before = (fs::read(a.join(".git/config")).unwrap(), fs::read(a.join(".git/index")).unwrap());
    let (c, _, _) = go(&["github", "--online", &code_arg(&s)], &with_gh(&s, &gh));
    assert_eq!(c, 0);
    assert_eq!(before.0, fs::read(a.join(".git/config")).unwrap());
    assert_eq!(before.1, fs::read(a.join(".git/index")).unwrap());
    assert!(!s.db().exists());
}

#[test]
fn online_with_no_github_repositories_never_starts_gh() {
    let s = Sandbox::new();
    s.repo("lonely");
    let gh = FakeGh::new(&[Route::ok("repos/*", "{}")]);
    let (c, out, _) = go(&["github", "--online", &code_arg(&s)], &with_gh(&s, &gh));
    assert_eq!(c, 0);
    assert!(out.contains("lonely  (no remote)"));
    assert!(!out.contains("Checked"));
    assert!(gh.calls().is_empty());
}

/// A clone whose `origin` URL now points at GitHub, while its remote-tracking
/// refs still hold the last fetch from the real local origin. Returns the
/// sandbox, the clone's directory, and the commit `origin/main` points at.
fn fetched_clone() -> (Sandbox, PathBuf, String) {
    let s = Sandbox::new();
    let upstream = TempDir::new();
    git(upstream.path(), &["init", "-q", "-b", "main"]);
    fs::write(upstream.path().join("f.txt"), "one\n").unwrap();
    git(upstream.path(), &["add", "f.txt"]);
    git(upstream.path(), &["commit", "-q", "-m", "init"]);
    git(&s.code(), &["clone", "-q", upstream.path().to_str().unwrap(), "work"]);
    let work = s.code().join("work");
    git(&work, &["remote", "set-url", "origin", "https://github.com/o/work.git"]);
    let tip = String::from_utf8(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&work)
            .args(["rev-parse", "refs/remotes/origin/main"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    (s, work, tip)
}

fn gh_for_work(compare: &str) -> FakeGh {
    FakeGh::new(&[
        Route::ok(
            "repos/o/work",
            r#"{"default_branch": "main", "pushed_at": "2023-11-14T22:13:20Z", "open_issues_count": 0}"#,
        ),
        Route::ok("repos/o/work/pulls*", "[]"),
        Route::ok("repos/o/work/actions/runs*", r#"{"workflow_runs": []}"#),
        Route::ok("repos/o/work/compare/*", compare),
    ])
}

#[test]
fn online_says_when_github_has_commits_you_have_not_fetched() {
    let (s, _work, tip) = fetched_clone();
    let gh = gh_for_work(r#"{"status": "ahead", "ahead_by": 2, "behind_by": 0}"#);
    let (c, out, err) = go(&["github", "--online", &code_arg(&s)], &with_gh(&s, &gh));
    assert_eq!(c, 0, "{err}");
    assert!(out.contains("GitHub has 2 new commits, fetch needed"), "{out}");
    assert!(out.contains("1 needing a fetch"), "{out}");
    // The comparison started from the commit the clone really has.
    let last = gh.calls().pop().unwrap();
    assert!(last.ends_with(&format!("repos/o/work/compare/{tip}...main?per_page=1")), "{last}");
}

#[test]
fn online_says_in_sync_when_nothing_is_new() {
    let (s, _work, _) = fetched_clone();
    let gh = gh_for_work(r#"{"status": "identical", "ahead_by": 0, "behind_by": 0}"#);
    let (_, out, _) = go(&["github", "--online", &code_arg(&s)], &with_gh(&s, &gh));
    assert!(out.contains("origin/main in sync"), "{out}");
    assert!(!out.contains("needing a fetch"));
}

#[test]
fn the_comparison_never_changes_the_clone() {
    let (s, work, _) = fetched_clone();
    let refs_before = fs::read_dir(work.join(".git/refs/remotes/origin"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    let config_before = fs::read(work.join(".git/config")).unwrap();
    let gh = gh_for_work(r#"{"status": "ahead", "ahead_by": 9, "behind_by": 0}"#);
    go(&["github", "--online", &code_arg(&s)], &with_gh(&s, &gh));
    let refs_after = fs::read_dir(work.join(".git/refs/remotes/origin"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(refs_before, refs_after);
    assert_eq!(config_before, fs::read(work.join(".git/config")).unwrap());
}

/// Create a repository whose only commit has the given commit time.
fn repo_at(s: &Sandbox, name: &str, iso: &str) -> PathBuf {
    let dir = s.code().join(name);
    fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    fs::write(dir.join("f.txt"), "x\n").unwrap();
    git(&dir, &["add", "f.txt"]);
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=T", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .arg("-C")
        .arg(&dir)
        .args(["commit", "-q", "-m", "x"])
        .env("GIT_AUTHOR_DATE", iso)
        .env("GIT_COMMITTER_DATE", iso)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    dir
}

/// The test clock is 2024-10-04 00:00 UTC. A commit on 2024-01-01 is 277 days
/// old, and one on 2024-10-01 is 3 days old.
fn stale_sandbox() -> Sandbox {
    let s = Sandbox::new();
    let old = repo_at(&s, "old", "2024-01-01T00:00:00Z");
    fs::write(old.join("wip.txt"), "unfinished\n").unwrap();
    repo_at(&s, "recent", "2024-10-01T00:00:00Z");
    let empty = s.code().join("empty");
    fs::create_dir_all(&empty).unwrap();
    git(&empty, &["init", "-q", "-b", "main"]);
    s
}

#[test]
fn stale_lists_quiet_repositories_with_their_unfinished_work() {
    let s = stale_sandbox();
    let (c, out, err) = go(&["stale", &code_arg(&s)], &s.env());
    assert_eq!(c, 0, "{err}");
    assert!(err.is_empty());
    assert!(
        out.starts_with("3 repositories: 1 active, 1 with no commit in 90+ days, 1 without any commits.\n"),
        "{out}"
    );
    assert!(out.contains("old  last commit 277 days ago, 1 uncommitted file\n"), "{out}");
    assert!(out.contains("no commits yet:  empty\n"), "{out}");
    assert!(!out.contains("recent  last commit"));
}

#[test]
fn the_days_flag_changes_the_threshold() {
    let s = stale_sandbox();
    let (_, out, _) = go(&["stale", "--days", "2", &code_arg(&s)], &s.env());
    assert!(out.contains("2 with no commit in 2+ days"), "{out}");
    assert!(out.contains("recent  last commit 3 days ago"), "{out}");
    let (_, out, _) = go(&["stale", "--days", "300", &code_arg(&s)], &s.env());
    assert!(out.contains("0 with no commit in 300+ days"), "{out}");
}

#[test]
fn the_config_sets_the_default_and_the_flag_overrides_it() {
    let s = stale_sandbox();
    s.write_config("[stale]\ndays = 5\n");
    let (_, out, _) = go(&["stale", &code_arg(&s)], &s.env());
    // Only the 277-day-old repository is quiet; the 3-day-old one is not.
    assert!(out.contains("1 with no commit in 5+ days"), "{out}");
    let (_, out, _) = go(&["stale", "--days", "100", &code_arg(&s)], &s.env());
    assert!(out.contains("with no commit in 100+ days"), "{out}");
}

#[test]
fn a_bad_days_value_is_a_usage_error() {
    let s = stale_sandbox();
    for bad in ["0", "abc", "3651"] {
        let (c, out, err) = go(&["stale", &format!("--days={bad}"), &code_arg(&s)], &s.env());
        assert_eq!(c, 2, "{bad}");
        assert!(out.is_empty());
        assert!(err.contains("--days must be a whole number"), "{err}");
    }
    let (c, _, err) = go(&["repos", "--days", "5", &code_arg(&s)], &s.env());
    assert_eq!(c, 2);
    assert!(err.contains("unknown option '--days'"));
}

#[test]
fn stale_needs_roots_like_the_other_scans() {
    let s = Sandbox::new();
    let (c, out, err) = go(&["stale"], &s.env());
    assert_eq!(c, 2);
    assert!(out.is_empty());
    assert!(err.contains("no scan roots configured"));
}

#[test]
fn stale_never_modifies_repositories_or_creates_a_database() {
    let s = stale_sandbox();
    let old = s.code().join("old");
    let before = (fs::read(old.join(".git/index")).unwrap(), fs::read(old.join(".git/HEAD")).unwrap());
    let (c, _, _) = go(&["stale", &code_arg(&s)], &s.env());
    assert_eq!(c, 0);
    assert_eq!(before.0, fs::read(old.join(".git/index")).unwrap());
    assert_eq!(before.1, fs::read(old.join(".git/HEAD")).unwrap());
    assert!(!s.db().exists());
}

#[test]
fn stale_voice_adds_one_remark_without_changing_the_facts() {
    let s = stale_sandbox();
    let (_, plain, _) = go(&["stale", &code_arg(&s)], &s.env());
    let (_, cheery, _) = go(&["--voice", "cheerful", "stale", &code_arg(&s)], &s.env());
    assert!(cheery.starts_with(plain.trim_end()));
    assert!(cheery.len() > plain.len());
}

#[test]
fn stale_reports_an_unreadable_repository_without_aborting() {
    let s = stale_sandbox();
    fs::create_dir_all(s.code().join("fake/.git")).unwrap();
    let (c, out, _) = go(&["stale", &code_arg(&s)], &s.env());
    assert_eq!(c, 0);
    assert!(out.contains("1 could not be read"), "{out}");
    assert!(out.contains("unreadable:      fake"), "{out}");
    assert!(out.contains("old  last commit 277 days ago"));
}
