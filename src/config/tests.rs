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
fn default_parallelism_follows_core_count_within_bounds() {
    assert_eq!(default_parallelism(0), 2);
    assert_eq!(default_parallelism(1), 2);
    assert_eq!(default_parallelism(4), 4);
    assert_eq!(default_parallelism(8), 8);
    assert_eq!(default_parallelism(64), 8);
}

#[test]
fn configured_default_is_within_the_allowed_range() {
    let p = Config::default().git.max_parallel;
    assert!((1..=MAX_PARALLEL_LIMIT).contains(&p));
    assert!((2..=8).contains(&p));
}

#[test]
fn data_path_prefers_absolute_xdg_data_home() {
    assert_eq!(
        data_path_from(Some("/x/data"), Some("/home/u")).unwrap(),
        PathBuf::from("/x/data/ares/ares.db")
    );
}

#[test]
fn data_path_falls_back_to_home_local_share() {
    let want = PathBuf::from("/home/u/.local/share/ares/ares.db");
    assert_eq!(data_path_from(None, Some("/home/u")).unwrap(), want);
    assert_eq!(data_path_from(Some(""), Some("/home/u")).unwrap(), want);
    assert_eq!(data_path_from(Some("relative"), Some("/home/u")).unwrap(), want);
}

#[test]
fn data_path_is_none_without_any_base() {
    assert_eq!(data_path_from(None, None), None);
    assert_eq!(data_path_from(None, Some("")), None);
}
