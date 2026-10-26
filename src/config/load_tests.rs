use super::load::ConfigError;
use super::Config;
use crate::testutil::TempDir;
use std::fs;
use std::path::{Path, PathBuf};

const HOME: &str = "/home/u";

fn load(text: &str) -> Result<Config, ConfigError> {
    Config::from_toml(text, Some(Path::new(HOME)))
}

#[test]
fn empty_text_gives_defaults() {
    assert_eq!(load("").unwrap(), Config::default());
}

#[test]
fn full_example_loads() {
    let c = load(
        r#"
# example
[scan]
roots = ["~/code", "/srv/work"]
max_depth = 4
exclude_dirs = ["node_modules", "target"]

[git]
timeout_secs = 20

[ui]
voice = "deadpan"
"#,
    )
    .unwrap();
    assert_eq!(
        c.scan.roots,
        vec![PathBuf::from("/home/u/code"), PathBuf::from("/srv/work")]
    );
    assert_eq!(c.scan.max_depth, 4);
    assert_eq!(c.scan.exclude_dirs, vec!["node_modules", "target"]);
    assert_eq!(c.git.timeout_secs, 20);
    assert_eq!(c.ui.voice, "deadpan");
}

#[test]
fn partial_config_keeps_other_defaults() {
    let c = load("[ui]\nvoice = \"plain\"\n").unwrap();
    assert_eq!(c.scan, Config::default().scan);
    assert_eq!(c.git, Config::default().git);
}

#[test]
fn tilde_alone_expands_to_home() {
    let c = load("[scan]\nroots = [\"~\"]\n").unwrap();
    assert_eq!(c.scan.roots, vec![PathBuf::from(HOME)]);
}

#[test]
fn tilde_without_home_is_an_error() {
    let e = Config::from_toml("[scan]\nroots = [\"~/x\"]\n", None).unwrap_err();
    assert!(e.message.contains("HOME"));
    assert_eq!(e.line, Some(2));
}

#[test]
fn relative_root_is_rejected() {
    let e = load("[scan]\nroots = [\"projects\"]\n").unwrap_err();
    assert!(e.message.contains("absolute"));
}

#[test]
fn empty_root_is_rejected() {
    assert!(load("[scan]\nroots = [\"\"]\n").is_err());
}

#[test]
fn unknown_key_is_rejected_with_line() {
    let e = load("[scan]\nroot = [\"/x\"]\n").unwrap_err();
    assert_eq!(e.line, Some(2));
    assert!(e.message.contains("unknown setting"));
}

#[test]
fn unknown_section_is_rejected() {
    assert!(load("[nope]\nk = 1\n").is_err());
}

#[test]
fn key_outside_section_is_rejected() {
    let e = load("voice = \"plain\"\n").unwrap_err();
    assert!(e.message.contains("[section]"));
}

#[test]
fn wrong_types_are_rejected() {
    assert!(load("[scan]\nmax_depth = \"six\"\n").is_err());
    assert!(load("[scan]\nroots = \"/x\"\n").is_err());
    assert!(load("[ui]\nvoice = 3\n").is_err());
    assert!(load("[git]\ntimeout_secs = [\"1\"]\n").is_err());
}

#[test]
fn numeric_ranges_are_enforced() {
    assert!(load("[scan]\nmax_depth = -1\n").is_err());
    assert!(load("[scan]\nmax_depth = 33\n").is_err());
    assert!(load("[scan]\nmax_depth = 0\n").is_ok());
    assert!(load("[git]\ntimeout_secs = 0\n").is_err());
    assert!(load("[git]\ntimeout_secs = 301\n").is_err());
    assert!(load("[git]\ntimeout_secs = 300\n").is_ok());
}

#[test]
fn exclude_dirs_must_be_plain_names() {
    assert!(load("[scan]\nexclude_dirs = [\"a/b\"]\n").is_err());
    assert!(load("[scan]\nexclude_dirs = [\"\"]\n").is_err());
}

#[test]
fn empty_exclude_dirs_is_allowed_and_replaces_defaults() {
    let c = load("[scan]\nexclude_dirs = []\n").unwrap();
    assert!(c.scan.exclude_dirs.is_empty());
}

#[test]
fn syntax_error_reports_line() {
    let e = load("[scan]\nthis is not valid\n").unwrap_err();
    assert_eq!(e.line, Some(2));
}

#[test]
fn missing_file_yields_defaults() {
    let t = TempDir::new();
    let c = Config::load(&t.path().join("absent.toml"), None).unwrap();
    assert_eq!(c, Config::default());
}

#[test]
fn file_is_read_and_errors_name_the_path() {
    let t = TempDir::new();
    let good = t.path().join("good.toml");
    fs::write(&good, "[git]\ntimeout_secs = 5\n").unwrap();
    assert_eq!(Config::load(&good, None).unwrap().git.timeout_secs, 5);

    let bad = t.path().join("bad.toml");
    fs::write(&bad, "[git]\ntimeout_secs = 0\n").unwrap();
    let e = Config::load(&bad, None).unwrap_err();
    assert!(e.message.contains("bad.toml"));
    assert_eq!(e.line, Some(2));
}

#[test]
fn unreadable_path_is_an_error_not_defaults() {
    let t = TempDir::new();
    // A directory where a file is expected.
    let e = Config::load(t.path(), None).unwrap_err();
    assert!(e.message.contains("cannot read"));
}

#[test]
fn max_parallel_is_loaded_and_range_checked() {
    assert_eq!(load("[git]\nmax_parallel = 8\n").unwrap().git.max_parallel, 8);
    assert!(load("[git]\nmax_parallel = 0\n").is_err());
    assert!(load("[git]\nmax_parallel = 17\n").is_err());
    assert!(load("[git]\nmax_parallel = 16\n").is_ok());
    assert!(load("[git]\nmax_parallel = \"4\"\n").is_err());
}

#[test]
fn stale_days_is_loaded_and_range_checked() {
    assert_eq!(load("[stale]\ndays = 30\n").unwrap().stale.days, 30);
    assert!(load("[stale]\ndays = 1\n").is_ok());
    assert!(load("[stale]\ndays = 3650\n").is_ok());
    assert!(load("[stale]\ndays = 0\n").is_err());
    assert!(load("[stale]\ndays = 3651\n").is_err());
    assert!(load("[stale]\ndays = \"90\"\n").is_err());
    assert!(load("[stale]\nweeks = 3\n").is_err());
}
