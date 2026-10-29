use crate::config::load::ConfigError;
use crate::config::Config;
use crate::tests::util::TempDir;
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

#[test]
fn ai_defaults_are_local_and_unconfigured() {
    let c = Config::default();
    assert_eq!(c.ai.model, "");
    assert_eq!(c.ai.ollama_port, 11434);
    assert_eq!(c.ai.timeout_secs, 120);
    assert!(!c.ai.unload_after);
    assert_eq!(load("").unwrap().ai, c.ai);
}

#[test]
fn ai_section_loads_every_key() {
    let c = load(
        "[ai]\nmodel = \"qwen2.5:3b\"\nollama_port = 11500\ntimeout_secs = 300\nunload_after = true\n",
    )
    .unwrap();
    assert_eq!(c.ai.model, "qwen2.5:3b");
    assert_eq!(c.ai.ollama_port, 11500);
    assert_eq!(c.ai.timeout_secs, 300);
    assert!(c.ai.unload_after);
}

#[test]
fn ai_section_leaves_other_sections_alone() {
    let c = load("[ai]\nmodel = \"llama3.2:1b\"\n").unwrap();
    assert_eq!(c.scan, Config::default().scan);
    assert_eq!(c.git, Config::default().git);
    assert_eq!(c.stale, Config::default().stale);
    assert_eq!(c.ui, Config::default().ui);
}

#[test]
fn ai_model_accepts_empty_and_real_names_and_rejects_bad_ones() {
    assert_eq!(load("[ai]\nmodel = \"\"\n").unwrap().ai.model, "");
    assert!(load("[ai]\nmodel = \"library/llama3.2:1b\"\n").is_ok());
    assert!(load("[ai]\nmodel = \"hf.co/user/model:Q4_K_M\"\n").is_ok());
    for bad in ["has space", "-flag", "a..b", "semi;colon", "new\\nline", "$(x)"] {
        let e = load(&format!("[ai]\nmodel = \"{bad}\"\n")).unwrap_err();
        assert!(e.message.contains("not a valid model name"), "{bad}: {e}");
        assert_eq!(e.line, Some(2));
    }
    assert!(load("[ai]\nmodel = 5\n").is_err());
    assert!(load("[ai]\nmodel = true\n").is_err());
}

#[test]
fn ai_port_is_range_checked() {
    assert_eq!(load("[ai]\nollama_port = 1\n").unwrap().ai.ollama_port, 1);
    assert_eq!(load("[ai]\nollama_port = 65535\n").unwrap().ai.ollama_port, 65535);
    assert!(load("[ai]\nollama_port = 0\n").is_err());
    assert!(load("[ai]\nollama_port = 65536\n").is_err());
    assert!(load("[ai]\nollama_port = -1\n").is_err());
    assert!(load("[ai]\nollama_port = \"11434\"\n").is_err());
}

#[test]
fn ai_timeout_is_range_checked() {
    assert_eq!(load("[ai]\ntimeout_secs = 5\n").unwrap().ai.timeout_secs, 5);
    assert_eq!(load("[ai]\ntimeout_secs = 900\n").unwrap().ai.timeout_secs, 900);
    assert!(load("[ai]\ntimeout_secs = 4\n").is_err());
    assert!(load("[ai]\ntimeout_secs = 901\n").is_err());
    assert!(load("[ai]\ntimeout_secs = 0\n").is_err());
    assert!(load("[ai]\ntimeout_secs = \"120\"\n").is_err());
}

#[test]
fn ai_unload_after_needs_a_real_boolean() {
    assert!(!load("[ai]\nunload_after = false\n").unwrap().ai.unload_after);
    assert!(load("[ai]\nunload_after = true\n").unwrap().ai.unload_after);
    let e = load("[ai]\nunload_after = \"true\"\n").unwrap_err();
    assert!(e.message.contains("true or false"));
    assert!(load("[ai]\nunload_after = 1\n").is_err());
    assert!(load("[ai]\nunload_after = yes\n").is_err());
}

#[test]
fn ai_unknown_keys_and_misplaced_keys_are_errors() {
    let e = load("[ai]\nmodle = \"x\"\n").unwrap_err();
    assert!(e.message.contains("unknown setting [ai] modle"));
    assert!(load("[ai]\nbackend = \"groq\"\n").is_err());
    assert!(load("[ui]\nmodel = \"x\"\n").is_err());
}
