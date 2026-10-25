//! Shared helpers for tests.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A uniquely named temporary directory, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "ares-test-{}-{}-{}",
            std::process::id(),
            n,
            nanos
        ));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Run git for test setup only. Isolated from the user's global config.
pub fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git must be installed to run these tests");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

pub fn init_repo() -> TempDir {
    let t = TempDir::new();
    git(t.path(), &["init", "-q", "-b", "main"]);
    t
}

pub fn commit_file(t: &TempDir, name: &str, content: &str) {
    fs::write(t.path().join(name), content).unwrap();
    git(t.path(), &["add", name]);
    git(t.path(), &["commit", "-q", "-m", "add file"]);
}

/// One canned response of the fake `gh`: when the request path (the last
/// argument) matches `pattern`, print `stdout` and `stderr` and exit `code`.
/// `pattern` is a shell `case` glob such as `repos/o/n/pulls*`.
pub struct Route {
    pub pattern: String,
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Route {
    pub fn ok(pattern: &str, stdout: &str) -> Route {
        Route {
            pattern: pattern.into(),
            code: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    pub fn fail(pattern: &str, code: i32, stderr: &str) -> Route {
        Route {
            pattern: pattern.into(),
            code,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }
}

/// A stand-in for the `gh` program: a shell script that logs every call and
/// answers from a route table. Anything unmatched gets a 404, like GitHub.
pub struct FakeGh {
    dir: TempDir,
    pub program: PathBuf,
}

impl FakeGh {
    pub fn new(routes: &[Route]) -> FakeGh {
        let dir = TempDir::new();
        let log = dir.path().join("calls.log");
        let mut script = String::from("#!/bin/sh\n");
        script.push_str("[ \"$1\" = \"--ares-probe\" ] && exit 0\n");
        script.push_str("for a; do last=\"$a\"; done\n");
        script.push_str(&format!("printf '%s\\n' \"$*\" >> '{}'\n", log.display()));
        script.push_str("case \"$last\" in\n");
        for r in routes {
            script.push_str(&format!("  {})\n", r.pattern));
            if !r.stdout.is_empty() {
                script.push_str(&format!("    cat <<'__ARES_OUT__'\n{}\n__ARES_OUT__\n", r.stdout));
            }
            if !r.stderr.is_empty() {
                script.push_str(&format!("    cat >&2 <<'__ARES_ERR__'\n{}\n__ARES_ERR__\n", r.stderr));
            }
            script.push_str(&format!("    exit {} ;;\n", r.code));
        }
        script.push_str("  *) echo 'gh: Not Found (HTTP 404)' >&2; exit 1 ;;\nesac\n");
        FakeGh::install(dir, &script)
    }

    /// A `gh` that never answers, for timeout tests.
    pub fn hanging() -> FakeGh {
        let dir = TempDir::new();
        FakeGh::install(
            dir,
            "#!/bin/sh\n[ \"$1\" = \"--ares-probe\" ] && exit 0\nsleep 30\n",
        )
    }

    fn install(dir: TempDir, script: &str) -> FakeGh {
        let program = dir.path().join("gh");
        fs::write(&program, script).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();

        // A freshly written script can briefly fail to execute ("text file
        // busy") if another test forks while the file is still open. Probe
        // until it runs.
        for _ in 0..200 {
            if Command::new(&program).arg("--ares-probe").output().is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        FakeGh { dir, program }
    }

    /// The argument lines of every call made so far, oldest first.
    pub fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.dir.path().join("calls.log"))
            .map(|s| s.lines().map(|l| l.to_string()).collect())
            .unwrap_or_default()
    }
}
