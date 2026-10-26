use crate::cli::*;
use std::path::PathBuf;

fn args(s: &str) -> Vec<String> {
    s.split_whitespace().map(|t| t.to_string()).collect()
}

fn cmd(s: &str) -> Command {
    parse(&args(s)).unwrap().command
}

fn err(s: &str) -> String {
    parse(&args(s)).unwrap_err().0
}

#[test]
fn no_arguments_shows_help() {
    assert_eq!(cmd(""), Command::Help);
}

#[test]
fn help_and_version_flags() {
    assert_eq!(cmd("--help"), Command::Help);
    assert_eq!(cmd("-h"), Command::Help);
    assert_eq!(cmd("--version"), Command::Version);
    assert_eq!(cmd("-V"), Command::Version);
    assert_eq!(cmd("help"), Command::Help);
    assert_eq!(cmd("version"), Command::Version);
}

#[test]
fn help_flag_wins_after_a_command() {
    assert_eq!(cmd("repos --help"), Command::Help);
}

#[test]
fn repos_defaults() {
    assert_eq!(cmd("repos"), Command::Repos(ReposArgs::default()));
}

#[test]
fn repos_dirty_flag() {
    match cmd("repos --dirty") {
        Command::Repos(r) => assert!(r.dirty_only),
        other => panic!("{other:?}"),
    }
}

#[test]
fn repos_roots_from_flag_equals_form_and_positional() {
    match cmd("repos --root /a --root=/b /c --dirty") {
        Command::Repos(r) => {
            assert_eq!(
                r.roots,
                vec![PathBuf::from("/a"), PathBuf::from("/b"), PathBuf::from("/c")]
            );
            assert!(r.dirty_only);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn global_options_work_before_and_after_the_command() {
    let a = parse(&args("--voice cheerful repos --config /x.toml")).unwrap();
    assert_eq!(a.global.voice.as_deref(), Some("cheerful"));
    assert_eq!(a.global.config_path, Some(PathBuf::from("/x.toml")));
    assert!(matches!(a.command, Command::Repos(_)));
}

#[test]
fn equals_forms_for_global_options() {
    let a = parse(&args("--voice=deadpan --config=/y.toml config")).unwrap();
    assert_eq!(a.global.voice.as_deref(), Some("deadpan"));
    assert_eq!(a.global.config_path, Some(PathBuf::from("/y.toml")));
    assert_eq!(a.command, Command::Config);
}

#[test]
fn missing_option_values_are_errors() {
    assert!(err("repos --root").contains("--root needs a value"));
    assert!(err("--config").contains("--config needs a value"));
    assert!(err("--voice").contains("--voice needs a value"));
    assert!(err("--config=").contains("non-empty"));
    assert!(err("repos --root=").contains("non-empty"));
}

#[test]
fn unknown_command_lists_the_valid_ones() {
    let e = err("frobnicate");
    assert!(e.contains("unknown command 'frobnicate'"));
    for c in COMMANDS {
        assert!(e.contains(c), "{e}");
    }
}

#[test]
fn unknown_option_for_repos_is_an_error() {
    assert!(err("repos --delete").contains("unknown option '--delete'"));
    assert!(err("repos -x").contains("unknown option '-x'"));
}

#[test]
fn config_takes_no_arguments() {
    assert_eq!(cmd("config"), Command::Config);
    assert!(err("config extra").contains("no arguments"));
}

#[test]
fn flag_order_does_not_matter() {
    assert_eq!(cmd("repos --dirty /a"), cmd("repos /a --dirty"));
}

#[test]
fn help_text_mentions_every_command_and_voice() {
    let h = help_text();
    for c in COMMANDS {
        assert!(h.contains(c), "help is missing {c}");
    }
    assert!(h.contains("plain"));
    assert!(h.contains("cheerful"));
    assert!(h.contains("deadpan"));
    assert!(h.contains("never modifies your repositories"));
    assert!(h.contains("Only `scan` writes"));
}

#[test]
fn status_command_accepts_roots_but_not_dirty() {
    match cmd("status --root /a /b") {
        Command::Status(r) => {
            assert_eq!(r.roots, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
            assert!(!r.dirty_only);
        }
        other => panic!("{other:?}"),
    }
    assert!(err("status --dirty").contains("unknown option '--dirty' for 'status'"));
}

#[test]
fn bare_status_has_no_roots() {
    assert_eq!(cmd("status"), Command::Status(ReposArgs::default()));
}

#[test]
fn scan_command_accepts_roots_but_not_dirty() {
    match cmd("scan /a --root /b") {
        Command::Scan(r) => assert_eq!(r.roots, vec![PathBuf::from("/a"), PathBuf::from("/b")]),
        other => panic!("{other:?}"),
    }
    assert!(err("scan --dirty").contains("unknown option '--dirty' for 'scan'"));
    assert_eq!(cmd("scan"), Command::Scan(ReposArgs::default()));
}

#[test]
fn changes_command_takes_no_arguments() {
    assert_eq!(cmd("changes"), Command::Changes);
    assert!(err("changes now").contains("'changes' takes no arguments"));
    assert!(err("changes --dirty").contains("takes no arguments"));
}

#[test]
fn github_command_accepts_roots_but_not_dirty() {
    match cmd("github /a --root /b") {
        Command::Github(r) => assert_eq!(r.roots, vec![PathBuf::from("/a"), PathBuf::from("/b")]),
        other => panic!("{other:?}"),
    }
    assert!(err("github --dirty").contains("unknown option '--dirty' for 'github'"));
    assert_eq!(cmd("github"), Command::Github(ReposArgs::default()));
}

#[test]
fn online_is_accepted_only_by_github() {
    match cmd("github --online /a") {
        Command::Github(r) => {
            assert!(r.online);
            assert_eq!(r.roots, vec![PathBuf::from("/a")]);
        }
        other => panic!("{other:?}"),
    }
    match cmd("github") {
        Command::Github(r) => assert!(!r.online, "online must be opt-in"),
        other => panic!("{other:?}"),
    }
    for c in ["repos", "status", "scan"] {
        assert!(err(&format!("{c} --online")).contains("unknown option '--online'"), "{c}");
    }
}

#[test]
fn help_describes_online_as_optional() {
    let h = help_text();
    assert!(h.contains("--online"));
    assert!(h.contains("Offline by default"));
}

#[test]
fn stale_command_takes_days_and_roots() {
    match cmd("stale --days 30 /a --root /b") {
        Command::Stale(r) => {
            assert_eq!(r.days, Some(30));
            assert_eq!(r.roots, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        }
        other => panic!("{other:?}"),
    }
    match cmd("stale --days=7") {
        Command::Stale(r) => assert_eq!(r.days, Some(7)),
        other => panic!("{other:?}"),
    }
    match cmd("stale") {
        Command::Stale(r) => assert_eq!(r.days, None, "the config decides when --days is absent"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn stale_days_must_be_a_sensible_whole_number() {
    for bad in ["0", "3651", "-5", "abc", "1.5", "", "99999999999999999999"] {
        let e = parse(&["stale".to_string(), format!("--days={bad}")]).unwrap_err().0;
        assert!(e.contains("--days must be a whole number from 1 to 3650"), "{bad:?}: {e}");
    }
    assert!(err("stale --days").contains("--days needs a value"));
    assert!(parse(&["stale".to_string(), "--days=1".to_string()]).is_ok());
    assert!(parse(&["stale".to_string(), "--days=3650".to_string()]).is_ok());
}

#[test]
fn days_is_only_accepted_by_stale() {
    for c in ["repos", "status", "scan", "github"] {
        assert!(err(&format!("{c} --days 5")).contains("unknown option '--days'"), "{c}");
    }
}
