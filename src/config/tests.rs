use super::*;

#[test]
fn defaults_scan_nothing_until_configured() {
    let c = Config::default();
    assert!(c.scan.roots.is_empty());
}

#[test]
fn default_voice_is_plain() {
    assert_eq!(Config::default().ui.voice, "plain");
}

#[test]
fn defaults_mirror_discovery_defaults() {
    let c = Config::default();
    let d = discovery::Options::default();
    assert_eq!(c.scan.max_depth, d.max_depth);
    assert_eq!(c.scan.exclude_dirs, d.exclude_dirs);
    let o = c.scan_options();
    assert_eq!(o.max_depth, d.max_depth);
}

#[test]
fn git_timeout_converts_to_duration() {
    let mut c = Config::default();
    c.git.timeout_secs = 3;
    assert_eq!(c.git_timeout(), Duration::from_secs(3));
}

#[test]
fn xdg_config_home_wins_when_absolute() {
    let p = config_path_from(Some("/x/conf"), Some("/home/u")).unwrap();
    assert_eq!(p, PathBuf::from("/x/conf/ares/config.toml"));
}

#[test]
fn relative_or_empty_xdg_falls_back_to_home() {
    let want = PathBuf::from("/home/u/.config/ares/config.toml");
    assert_eq!(config_path_from(Some("rel/path"), Some("/home/u")).unwrap(), want);
    assert_eq!(config_path_from(Some(""), Some("/home/u")).unwrap(), want);
    assert_eq!(config_path_from(None, Some("/home/u")).unwrap(), want);
}

#[test]
fn no_home_and_no_xdg_gives_none() {
    assert_eq!(config_path_from(None, None), None);
    assert_eq!(config_path_from(None, Some("")), None);
}

#[test]
fn default_parallelism_is_modest() {
    assert_eq!(Config::default().git.max_parallel, 4);
}
