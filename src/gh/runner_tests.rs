use super::runner::{command, run, run_with_limit_for_test, GhError, Query};
use crate::github::RepoRef;
use crate::testutil::{FakeGh, Route};
use std::ffi::OsStr;
use std::path::Path;
use std::time::{Duration, Instant};

const T: Duration = Duration::from_secs(10);

fn repo(owner: &str, name: &str) -> RepoRef {
    RepoRef {
        owner: owner.into(),
        name: name.into(),
    }
}

const GET_PREFIX: &str = "api --method GET --hostname github.com -H Accept: application/vnd.github+json ";

#[test]
fn endpoints_are_built_from_validated_parts() {
    let r = repo("STAKXX002", "ARES");
    assert_eq!(Query::Repo(&r).endpoint().unwrap(), "repos/STAKXX002/ARES");
    assert_eq!(
        Query::OpenPulls(&r).endpoint().unwrap(),
        "repos/STAKXX002/ARES/pulls?state=open&per_page=100"
    );
    assert_eq!(
        Query::LatestRun { repo: &r, branch: None }.endpoint().unwrap(),
        "repos/STAKXX002/ARES/actions/runs?per_page=1&exclude_pull_requests=true"
    );
    assert_eq!(
        Query::LatestRun { repo: &r, branch: Some("main") }.endpoint().unwrap(),
        "repos/STAKXX002/ARES/actions/runs?per_page=1&exclude_pull_requests=true&branch=main"
    );
}

#[test]
fn every_endpoint_stays_under_repos() {
    let r = repo("o", "n");
    for q in [
        Query::Repo(&r),
        Query::OpenPulls(&r),
        Query::LatestRun { repo: &r, branch: Some("dev-1.2") },
    ] {
        assert!(q.endpoint().unwrap().starts_with("repos/o/n"));
    }
}

#[test]
fn invalid_parts_are_refused_before_anything_runs() {
    let gh = FakeGh::new(&[Route::ok("repos/*", "{}")]);
    let bad_repos = [
        repo("o/../x", "n"),
        repo("o", "n;rm -rf /"),
        repo("o", "n$(x)"),
        repo("-flag", "n"),
        repo("", "n"),
        repo("o", ".."),
        repo("o x", "n"),
        repo("o", "n\nextra"),
    ];
    for r in &bad_repos {
        let e = run(&gh.program, &Query::Repo(r), T).unwrap_err();
        assert!(matches!(e, GhError::InvalidArgument(_)), "{r:?}: {e}");
    }
    let good = repo("o", "n");
    for b in ["", "-x", ".hidden", "a/b", "a b", "a&b", "a?b", "a#b", "x\ny", &"a".repeat(101)] {
        let q = Query::LatestRun { repo: &good, branch: Some(b) };
        let e = run(&gh.program, &q, T).unwrap_err();
        assert!(matches!(e, GhError::InvalidArgument(_)), "{b:?}: {e}");
    }
    assert!(gh.calls().is_empty(), "gh must never be started for an invalid request");
}

#[test]
fn a_request_is_one_get_with_fixed_flags() {
    let gh = FakeGh::new(&[Route::ok("repos/o/n", "{\"ok\":true}")]);
    let r = repo("o", "n");
    let out = run(&gh.program, &Query::Repo(&r), T).unwrap();
    assert_eq!(out.trim(), "{\"ok\":true}");
    assert_eq!(gh.calls(), vec![format!("{GET_PREFIX}repos/o/n")]);
}

#[test]
fn no_request_can_carry_a_body_or_change_the_method() {
    let gh = FakeGh::new(&[Route::ok("repos/*", "{}")]);
    let r = repo("o", "n");
    for q in [
        Query::Repo(&r),
        Query::OpenPulls(&r),
        Query::LatestRun { repo: &r, branch: Some("main") },
    ] {
        run(&gh.program, &q, T).unwrap();
    }
    let calls = gh.calls();
    assert_eq!(calls.len(), 3);
    for c in &calls {
        assert!(c.starts_with(GET_PREFIX), "{c}");
        for forbidden in [" -f ", " -F ", "--field", "--raw-field", "--input", "POST", "PATCH", "PUT", "DELETE", "graphql"] {
            assert!(!c.contains(forbidden), "{forbidden:?} in {c}");
        }
        // Exactly one request path, and it is the last argument.
        assert_eq!(c.matches("repos/").count(), 1, "{c}");
    }
}

#[test]
fn http_errors_become_failed_with_the_message() {
    let gh = FakeGh::new(&[Route::fail("repos/o/missing", 1, "gh: Not Found (HTTP 404)")]);
    let r = repo("o", "missing");
    match run(&gh.program, &Query::Repo(&r), T) {
        Err(GhError::Failed { code, message }) => {
            assert_eq!(code, Some(1));
            assert_eq!(message, "gh: Not Found (HTTP 404)");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn tokens_in_error_output_never_reach_the_error_text() {
    let secret = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    let gh = FakeGh::new(&[Route::fail(
        "repos/o/n",
        4,
        &format!("> Authorization: token {secret}\nGET https://user:{secret}@github.com/x failed"),
    )]);
    let r = repo("o", "n");
    let e = run(&gh.program, &Query::Repo(&r), T).unwrap_err();
    let text = e.to_string();
    assert!(!text.contains(secret), "{text}");
    assert!(!text.contains("abcdefghijklmnop"), "{text}");
}

#[test]
fn long_error_text_is_truncated_to_one_line() {
    let gh = FakeGh::new(&[Route::fail("repos/o/n", 1, &format!("line one\n{}", "x".repeat(2000)))]);
    let r = repo("o", "n");
    let e = run(&gh.program, &Query::Repo(&r), T).unwrap_err().to_string();
    assert!(!e.contains('\n'));
    assert!(e.chars().count() <= 303, "{}", e.chars().count());
    assert!(e.ends_with("..."));
}

#[test]
fn a_missing_gh_is_reported_as_not_installed() {
    let r = repo("o", "n");
    let e = run(Path::new("/nonexistent/gh"), &Query::Repo(&r), T).unwrap_err();
    assert!(matches!(e, GhError::NotInstalled(_)), "{e}");
    assert!(e.to_string().contains("is it installed"));
}

#[test]
fn a_hung_gh_is_stopped_at_the_timeout() {
    let slow = FakeGh::hanging();
    let r = repo("o", "n");
    let start = Instant::now();
    let e = run(&slow.program, &Query::Repo(&r), Duration::from_millis(300)).unwrap_err();
    assert!(matches!(e, GhError::Timeout(_)), "{e}");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn oversized_responses_are_rejected() {
    let big = "x".repeat(5000);
    let gh = FakeGh::new(&[Route::ok("repos/o/n", &big)]);
    let r = repo("o", "n");
    match run_with_limit_for_test(&gh.program, &Query::Repo(&r), T, 1000) {
        Err(GhError::TooLarge { limit }) => assert_eq!(limit, 1000),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_command_is_hardened() {
    let cmd = command(Path::new("gh"), "repos/o/n");
    let envs: Vec<(&OsStr, Option<&OsStr>)> = cmd.get_envs().collect();
    let get = |k: &str| envs.iter().find(|(n, _)| *n == OsStr::new(k)).map(|(_, v)| *v);
    assert_eq!(get("GH_PROMPT_DISABLED"), Some(Some(OsStr::new("1"))));
    assert_eq!(get("GH_NO_UPDATE_NOTIFIER"), Some(Some(OsStr::new("1"))));
    // `None` inside `Some` means the variable is explicitly removed.
    assert_eq!(get("GH_DEBUG"), Some(None));
    assert_eq!(get("DEBUG"), Some(None));
    let args: Vec<&OsStr> = cmd.get_args().collect();
    assert_eq!(args[0], OsStr::new("api"));
    assert_eq!(args.last().copied(), Some(OsStr::new("repos/o/n")));
}
