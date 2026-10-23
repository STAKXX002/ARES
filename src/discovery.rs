//! Git repository discovery.
//!
//! Walks user-configured roots looking for directories that contain a
//! `.git` entry. Read-only: it only lists directories and inspects metadata.
//!
//! Behavior:
//! - Symlinks are never followed, which avoids loops and escaping the roots.
//! - Once a repository is found, discovery does not descend into it, so
//!   submodules and vendored repos are not reported separately.
//! - Heavy directories such as `node_modules` and `target` are skipped.
//! - Unreadable directories are recorded, not fatal.
//! - A `.git` file counts as well as a `.git` directory, covering worktrees
//!   and submodules. Bare repositories are not detected.

use std::fs;
use std::path::{Path, PathBuf};

/// Directory names skipped by default. They are large, generated, or
/// not places where a person keeps source repositories.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    "node_modules",
    "target",
    "build",
    "dist",
    "venv",
    ".venv",
    "__pycache__",
    ".cache",
    ".cargo",
    ".rustup",
    ".npm",
    ".local",
    "snap",
];

#[derive(Debug, Clone)]
pub struct Options {
    /// Maximum directory depth below each root. Depth 0 is the root itself.
    pub max_depth: usize,
    /// Directory names to skip, matched exactly against each entry name.
    pub exclude_dirs: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            max_depth: 6,
            exclude_dirs: DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Discovery {
    /// Repository working directories, sorted and de-duplicated.
    pub repos: Vec<PathBuf>,
    /// Directories that could not be read, with the reason.
    pub errors: Vec<(PathBuf, String)>,
}

fn is_repo_root(dir: &Path) -> bool {
    match fs::symlink_metadata(dir.join(".git")) {
        Ok(m) => m.is_dir() || m.is_file(),
        Err(_) => false,
    }
}

/// Discover repositories under each of `roots`.
pub fn discover(roots: &[PathBuf], opts: &Options) -> Discovery {
    let mut out = Discovery::default();
    let mut stack: Vec<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();

    while let Some((dir, depth)) = stack.pop() {
        if is_repo_root(&dir) {
            out.repos.push(dir);
            continue;
        }
        if depth >= opts.max_depth {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                out.errors.push((dir, e.to_string()));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    out.errors.push((dir.clone(), e.to_string()));
                    continue;
                }
            };
            // DirEntry::file_type does not follow symlinks.
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if !is_dir {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if opts.exclude_dirs.iter().any(|x| x == name.as_ref()) {
                continue;
            }
            stack.push((entry.path(), depth + 1));
        }
    }

    out.repos.sort();
    out.repos.dedup();
    out
}
