use super::activity::{CiState, PULLS_PAGE};
use super::fetch::{fetch_activity, fetch_all, MAX_NETWORK_WORKERS};
use crate::github::RepoRef;
use crate::testutil::{FakeGh, Route};
use std::time::Duration;

const T: Duration = Duration::from_secs(10);
const GET_PREFIX: &str = "api --method GET --hostname github.com -H Accept: application/vnd.github+json ";

fn repo(owner: &str, name: &str) -> RepoRef {
    RepoRef {
        owner: owner.into(),
        name: name.into(),
    }
}

const REPO_JSON: &str = r#"{"private": false, "archived": false, "fork": false,
    "default_branch": "main", "pushed_at": "2023-11-14T22:13:20Z", "open_issues_count": 5}"#;
const PULLS_JSON: &str = r#"[{"number": 1}, {"number": 2}]"#;
const RUNS_JSON: &str = r#"{"workflow_runs": [{"name": "CI", "status": "completed",
    "conclusion": "failure", "head_branch": "main", "updated_at": "2023-11-14T22:13:20Z"}]}"#;

fn happy() -> FakeGh {
    FakeGh::new(&[
        Route::ok("repos/o/n", REPO_JSON),
        Route::ok("repos/o/n/pulls*", PULLS_JSON),
        Route::ok("repos/o/n/actions/runs*", RUNS_JSON),
    ])
}

#[test]
fn a_repository_is_summarized_from_three_get_requests() {
    let gh = happy();
    let a = fetch_activity(&gh.program, &repo("o", "n"), T).unwrap();
    assert_eq!((a.open_prs, a.open_issues, a.prs_capped), (2, Some(3), false));
    assert_eq!(a.pushed_at, Some(1_700_000_000));
    assert_eq!(a.default_branch.as_deref(), Some("main"));
    let ci = a.ci.unwrap();
    assert_eq!(ci.state(), CiState::Failing);
    assert_eq!(a.ci_error, None);

    let calls = gh.calls();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|c| c.starts_with(GET_PREFIX)), "{calls:?}");
    assert!(calls[0].ends_with("repos/o/n"));
    assert!(calls[1].ends_with("repos/o/n/pulls?state=open&per_page=100"));
    assert!(calls[2].ends_with("actions/runs?per_page=1&exclude_pull_requests=true&branch=main"));
}

#[test]
fn archived_repositories_skip_the_workflow_lookup() {
    let gh = FakeGh::new(&[
        Route::ok("repos/o/n", r#"{"archived": true, "open_issues_count": 0, "default_branch": "main"}"#),
        Route::ok("repos/o/n/pulls*", "[]"),
    ]);
    let a = fetch_activity(&gh.program, &repo("o", "n"), T).unwrap();
    assert!(a.archived);
    assert_eq!(a.ci, None);
    assert_eq!(gh.calls().len(), 2);
}

#[test]
fn an_unsafe_default_branch_name_is_not_put_in_the_request() {
    let gh = FakeGh::new(&[
        Route::ok("repos/o/n", r#"{"open_issues_count": 0, "default_branch": "feat/x y&z=1"}"#),
        Route::ok("repos/o/n/pulls*", "[]"),
        Route::ok("repos/o/n/actions/runs*", r#"{"workflow_runs": []}"#),
    ]);
    let a = fetch_activity(&gh.program, &repo("o", "n"), T).unwrap();
    assert_eq!(a.ci, None);
    let last = gh.calls().pop().unwrap();
    assert!(!last.contains("branch="), "{last}");
    assert!(!last.contains("z=1"), "{last}");
}

#[test]
fn missing_workflow_access_is_not_an_error() {
    let gh = FakeGh::new(&[
        Route::ok("repos/o/n", REPO_JSON),
        Route::ok("repos/o/n/pulls*", "[]"),
        Route::fail("repos/o/n/actions/runs*", 1, "gh: Not Found (HTTP 404)"),
    ]);
    let a = fetch_activity(&gh.program, &repo("o", "n"), T).unwrap();
    assert_eq!(a.ci, None);
    assert_eq!(a.ci_error, None);
}

#[test]
fn a_workflow_failure_keeps_the_rest_of_the_data() {
    let gh = FakeGh::new(&[
        Route::ok("repos/o/n", REPO_JSON),
        Route::ok("repos/o/n/pulls*", PULLS_JSON),
        Route::fail("repos/o/n/actions/runs*", 1, "gh: Server Error (HTTP 500)"),
    ]);
    let a = fetch_activity(&gh.program, &repo("o", "n"), T).unwrap();
    assert_eq!(a.open_prs, 2);
    assert_eq!(a.ci, None);
    assert!(a.ci_error.unwrap().contains("HTTP 500"));
}

#[test]
fn a_missing_repository_stops_after_one_request() {
    let gh = FakeGh::new(&[Route::fail("repos/o/n", 1, "gh: Not Found (HTTP 404)")]);
    let e = fetch_activity(&gh.program, &repo("o", "n"), T).unwrap_err();
    assert!(e.contains("HTTP 404"), "{e}");
    assert_eq!(gh.calls().len(), 1);
}

#[test]
fn a_pull_request_failure_is_an_error_for_that_repository() {
    let gh = FakeGh::new(&[
        Route::ok("repos/o/n", REPO_JSON),
        Route::fail("repos/o/n/pulls*", 1, "gh: API rate limit exceeded (HTTP 403)"),
    ]);
    let e = fetch_activity(&gh.program, &repo("o", "n"), T).unwrap_err();
    assert!(e.contains("rate limit"), "{e}");
}

#[test]
fn garbage_responses_are_errors_not_panics() {
    let gh = FakeGh::new(&[Route::ok("repos/o/n", "<html>captive portal</html>")]);
    let e = fetch_activity(&gh.program, &repo("o", "n"), T).unwrap_err();
    assert!(e.contains("not valid JSON"), "{e}");
}

#[test]
fn a_full_page_of_pull_requests_is_reported_as_capped() {
    let many = format!("[{}]", vec!["{}"; PULLS_PAGE].join(","));
    let gh = FakeGh::new(&[
        Route::ok("repos/o/n", REPO_JSON),
        Route::ok("repos/o/n/pulls*", &many),
        Route::ok("repos/o/n/actions/runs*", r#"{"workflow_runs": []}"#),
    ]);
    let a = fetch_activity(&gh.program, &repo("o", "n"), T).unwrap();
    assert!(a.prs_capped);
    assert_eq!(a.open_prs, PULLS_PAGE);
    assert_eq!(a.open_issues, None);
}

#[test]
fn many_repositories_come_back_in_order_and_failures_stay_local() {
    let gh = FakeGh::new(&[
        Route::ok("repos/o/a", REPO_JSON),
        Route::ok("repos/o/a/pulls*", "[]"),
        Route::ok("repos/o/a/actions/runs*", r#"{"workflow_runs": []}"#),
        Route::fail("repos/o/b", 1, "gh: Not Found (HTTP 404)"),
        Route::ok("repos/o/c", r#"{"open_issues_count": 9}"#),
        Route::ok("repos/o/c/pulls*", PULLS_JSON),
        Route::ok("repos/o/c/actions/runs*", r#"{"workflow_runs": []}"#),
    ]);
    let repos = vec![repo("o", "a"), repo("o", "b"), repo("o", "c")];
    let sequential = fetch_all(&gh.program, &repos, T, 1);
    assert!(sequential[0].is_ok() && sequential[1].is_err() && sequential[2].is_ok());
    assert_eq!(sequential[2].as_ref().unwrap().open_issues, Some(7));
    for workers in [2, 4, 16] {
        assert_eq!(fetch_all(&gh.program, &repos, T, workers), sequential, "workers = {workers}");
    }
}

#[test]
fn network_parallelism_is_capped() {
    assert!(MAX_NETWORK_WORKERS <= 4);
    let gh = happy();
    let repos = vec![repo("o", "n"); 8];
    // Asking for 16 workers must still work, using at most the cap.
    let out = fetch_all(&gh.program, &repos, T, 16);
    assert_eq!(out.len(), 8);
    assert!(out.iter().all(Result::is_ok));
}

#[test]
fn a_missing_gh_gives_a_clear_error_for_every_repository() {
    let repos = vec![repo("o", "a"), repo("o", "b")];
    let out = fetch_all(std::path::Path::new("/nonexistent/gh"), &repos, T, 4);
    for r in out {
        assert!(r.unwrap_err().contains("is it installed"));
    }
}
