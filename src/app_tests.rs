//! End-to-end tests of `app::run` using real git repositories.

use crate::app::{run, Env};
use crate::store::Store;
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
