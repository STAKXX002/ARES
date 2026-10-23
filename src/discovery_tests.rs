use crate::discovery::{discover, Options};
use crate::testutil::TempDir;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

/// Fake a repository by creating a `.git` directory. Discovery only checks
/// for the entry's existence, so no real git is needed here.
fn mk_repo(dir: &Path) {
    fs::create_dir_all(dir.join(".git")).unwrap();
}

fn rel(root: &Path, found: &[PathBuf]) -> Vec<String> {
    found
        .iter()
        .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn finds_repos_at_various_depths() {
    let t = TempDir::new();
    mk_repo(&t.path().join("a"));
    mk_repo(&t.path().join("work/b"));
    mk_repo(&t.path().join("work/deep/c"));
    let d = discover(&[t.path().to_path_buf()], &Options::default());
    assert_eq!(rel(t.path(), &d.repos), vec!["a", "work/b", "work/deep/c"]);
    assert!(d.errors.is_empty());
}

#[test]
fn root_itself_can_be_a_repo() {
    let t = TempDir::new();
    mk_repo(t.path());
    let d = discover(&[t.path().to_path_buf()], &Options::default());
    assert_eq!(d.repos, vec![t.path().to_path_buf()]);
}

#[test]
fn git_file_counts_as_repo() {
    let t = TempDir::new();
    let wt = t.path().join("worktree");
    fs::create_dir_all(&wt).unwrap();
    fs::write(wt.join(".git"), "gitdir: /somewhere/else\n").unwrap();
    let d = discover(&[t.path().to_path_buf()], &Options::default());
    assert_eq!(rel(t.path(), &d.repos), vec!["worktree"]);
}

#[test]
fn does_not_descend_into_a_repo() {
    let t = TempDir::new();
    mk_repo(&t.path().join("outer"));
    mk_repo(&t.path().join("outer/vendor/inner"));
    let d = discover(&[t.path().to_path_buf()], &Options::default());
    assert_eq!(rel(t.path(), &d.repos), vec!["outer"]);
}

#[test]
fn excluded_directories_are_skipped() {
    let t = TempDir::new();
    mk_repo(&t.path().join("keep"));
    mk_repo(&t.path().join("node_modules/pkg"));
    mk_repo(&t.path().join("proj/target/debug/dep"));
    let d = discover(&[t.path().to_path_buf()], &Options::default());
    assert_eq!(rel(t.path(), &d.repos), vec!["keep"]);
}

#[test]
fn custom_excludes_replace_defaults() {
    let t = TempDir::new();
    mk_repo(&t.path().join("node_modules/pkg"));
    mk_repo(&t.path().join("skipme/x"));
    let opts = Options {
        exclude_dirs: vec!["skipme".to_string()],
        ..Options::default()
    };
    let d = discover(&[t.path().to_path_buf()], &opts);
    assert_eq!(rel(t.path(), &d.repos), vec!["node_modules/pkg"]);
}

#[test]
fn max_depth_limits_the_walk() {
    let t = TempDir::new();
    mk_repo(&t.path().join("one"));
    mk_repo(&t.path().join("x/two"));
    mk_repo(&t.path().join("x/y/three"));
    let opts = Options {
        max_depth: 2,
        ..Options::default()
    };
    let d = discover(&[t.path().to_path_buf()], &opts);
    assert_eq!(rel(t.path(), &d.repos), vec!["one", "x/two"]);
}

#[test]
fn symlinks_are_not_followed() {
    let outside = TempDir::new();
    mk_repo(&outside.path().join("secret"));
    let t = TempDir::new();
    symlink(outside.path(), t.path().join("link")).unwrap();
    let d = discover(&[t.path().to_path_buf()], &Options::default());
    assert!(d.repos.is_empty());
}

#[test]
fn symlink_loop_terminates() {
    let t = TempDir::new();
    fs::create_dir_all(t.path().join("a")).unwrap();
    symlink(t.path(), t.path().join("a/loop")).unwrap();
    let d = discover(&[t.path().to_path_buf()], &Options::default());
    assert!(d.repos.is_empty());
}

#[test]
fn overlapping_roots_are_deduplicated() {
    let t = TempDir::new();
    mk_repo(&t.path().join("work/proj"));
    let roots = vec![t.path().to_path_buf(), t.path().join("work")];
    let d = discover(&roots, &Options::default());
    assert_eq!(rel(t.path(), &d.repos), vec!["work/proj"]);
}

#[test]
fn missing_root_is_recorded_as_error_not_panic() {
    let t = TempDir::new();
    let gone = t.path().join("nope");
    let d = discover(&[gone.clone()], &Options::default());
    assert!(d.repos.is_empty());
    assert_eq!(d.errors.len(), 1);
    assert_eq!(d.errors[0].0, gone);
}

#[test]
fn results_are_sorted() {
    let t = TempDir::new();
    for n in ["zeta", "alpha", "mid"] {
        mk_repo(&t.path().join(n));
    }
    let d = discover(&[t.path().to_path_buf()], &Options::default());
    assert_eq!(rel(t.path(), &d.repos), vec!["alpha", "mid", "zeta"]);
}
