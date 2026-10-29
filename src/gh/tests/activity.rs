use crate::gh::activity::*;

const REPO: &str = r#"{
  "name": "ARES", "private": true, "archived": false, "fork": true,
  "default_branch": "main", "pushed_at": "2023-11-14T22:13:20Z",
  "open_issues_count": 7, "something_new": {"ignored": [1,2,3]}
}"#;

fn ci(status: &str, conclusion: Option<&str>) -> Ci {
    Ci {
        status: status.into(),
        conclusion: conclusion.map(String::from),
        branch: Some("main".into()),
        workflow: Some("CI".into()),
        updated_at: None,
    }
}

#[test]
fn parses_a_repository() {
    let r = parse_repo(REPO).unwrap();
    assert!(r.private && r.fork && !r.archived);
    assert_eq!(r.default_branch.as_deref(), Some("main"));
    assert_eq!(r.pushed_at, Some(1_700_000_000));
    assert_eq!(r.open_issues_and_prs, 7);
}

#[test]
fn unknown_fields_and_missing_optional_ones_are_fine() {
    let r = parse_repo(r#"{"open_issues_count": 0}"#).unwrap();
    assert!(!r.private && !r.archived && !r.fork);
    assert_eq!(r.default_branch, None);
    assert_eq!(r.pushed_at, None);
}

#[test]
fn a_bad_timestamp_is_dropped_not_fatal() {
    let r = parse_repo(r#"{"open_issues_count": 1, "pushed_at": "yesterday"}"#).unwrap();
    assert_eq!(r.pushed_at, None);
}

#[test]
fn repository_responses_that_are_wrong_are_errors() {
    for bad in [
        "",
        "not json",
        "[]",
        "null",
        "42",
        r#"{"private": true}"#,
        r#"{"open_issues_count": "3"}"#,
        r#"{"open_issues_count": -1}"#,
        r#"{"open_issues_count": 3.5}"#,
        r#"{"message": "Not Found", "documentation_url": "x"}"#,
    ] {
        assert!(parse_repo(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn long_strings_are_cut() {
    let long = "a".repeat(5000);
    let r = parse_repo(&format!(r#"{{"open_issues_count": 1, "default_branch": "{long}"}}"#)).unwrap();
    assert_eq!(r.default_branch.unwrap().chars().count(), 200);
}

#[test]
fn counts_pull_requests() {
    assert_eq!(parse_pulls("[]").unwrap(), 0);
    assert_eq!(parse_pulls(r#"[{"number":1},{"number":2},{}]"#).unwrap(), 3);
    for bad in ["", "{}", r#"{"message":"x"}"#, "null", "3", "[1,"] {
        assert!(parse_pulls(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn parses_the_latest_run() {
    let text = r#"{"total_count": 9, "workflow_runs": [
        {"name": "CI", "status": "completed", "conclusion": "failure",
         "head_branch": "main", "updated_at": "2023-11-14T22:13:20Z"},
        {"name": "older", "status": "completed", "conclusion": "success"}]}"#;
    let c = parse_latest_run(text).unwrap().unwrap();
    assert_eq!(c.status, "completed");
    assert_eq!(c.conclusion.as_deref(), Some("failure"));
    assert_eq!(c.branch.as_deref(), Some("main"));
    assert_eq!(c.workflow.as_deref(), Some("CI"));
    assert_eq!(c.updated_at, Some(1_700_000_000));
    assert_eq!(c.state(), CiState::Failing);
}

#[test]
fn no_runs_is_none_and_null_conclusion_is_allowed() {
    assert_eq!(parse_latest_run(r#"{"total_count":0,"workflow_runs":[]}"#).unwrap(), None);
    let c = parse_latest_run(r#"{"workflow_runs":[{"status":"in_progress","conclusion":null}]}"#)
        .unwrap()
        .unwrap();
    assert_eq!(c.conclusion, None);
    assert_eq!(c.state(), CiState::Running);
}

#[test]
fn run_responses_that_are_wrong_are_errors() {
    for bad in [
        "",
        "[]",
        "{}",
        r#"{"workflow_runs": {}}"#,
        r#"{"workflow_runs": [{"conclusion": "success"}]}"#,
        r#"{"workflow_runs": [{"status": 5}]}"#,
        r#"{"workflow_runs": [1]}"#,
    ] {
        assert!(parse_latest_run(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn ci_state_mapping() {
    assert_eq!(ci("completed", Some("success")).state(), CiState::Passing);
    for f in ["failure", "timed_out", "startup_failure"] {
        assert_eq!(ci("completed", Some(f)).state(), CiState::Failing, "{f}");
    }
    for s in ["in_progress", "queued", "waiting", "pending", "requested"] {
        assert_eq!(ci(s, None).state(), CiState::Running, "{s}");
    }
    assert_eq!(ci("completed", Some("cancelled")).state(), CiState::Other("cancelled".into()));
    assert_eq!(ci("completed", None).state(), CiState::Other("completed".into()));
    assert_eq!(ci("weird", None).state(), CiState::Other("weird".into()));
}

fn info(open: u64) -> RepoInfo {
    parse_repo(&format!(r#"{{"open_issues_count": {open}, "default_branch": "main"}}"#)).unwrap()
}

#[test]
fn issues_exclude_pull_requests() {
    let a = combine(&info(7), 3, Ok(None), None);
    assert_eq!((a.open_prs, a.open_issues, a.prs_capped), (3, Some(4), false));
}

#[test]
fn a_capped_pull_list_makes_the_issue_count_unknown() {
    let a = combine(&info(250), PULLS_PAGE, Ok(None), None);
    assert!(a.prs_capped);
    assert_eq!(a.open_issues, None);
}

#[test]
fn issue_subtraction_never_underflows() {
    let a = combine(&info(1), 5, Ok(None), None);
    assert_eq!(a.open_issues, Some(0));
}

#[test]
fn a_ci_failure_is_recorded_without_losing_the_rest() {
    let a = combine(&info(2), 1, Err("gh: Server Error (HTTP 500)".into()), None);
    assert_eq!(a.ci, None);
    assert_eq!(a.ci_error.as_deref(), Some("gh: Server Error (HTTP 500)"));
    assert_eq!(a.open_prs, 1);
}

#[test]
fn hostile_nesting_is_an_error_not_a_crash() {
    let deep = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
    assert!(parse_pulls(&deep).is_err());
    assert!(parse_repo(&deep).is_err());
    assert!(parse_latest_run(&deep).is_err());
    let deep_obj = format!("{}1{}", r#"{"a":"#.repeat(5000), "}".repeat(5000));
    assert!(parse_repo(&deep_obj).is_err());
}

#[test]
fn huge_numbers_do_not_panic() {
    assert!(parse_repo(r#"{"open_issues_count": 18446744073709551615}"#).is_ok());
    assert!(parse_repo(r#"{"open_issues_count": 1e400}"#).is_err());
    assert!(parse_repo(r#"{"open_issues_count": 99999999999999999999999}"#).is_err());
}

fn compare_json(status: &str, ahead: u64, behind: u64) -> String {
    format!(r#"{{"status": "{status}", "ahead_by": {ahead}, "behind_by": {behind}, "total_commits": {ahead}}}"#)
}

#[test]
fn compare_statuses_map_to_sync_states() {
    assert_eq!(parse_compare(&compare_json("identical", 0, 0)), Ok(SyncState::Current));
    assert_eq!(
        parse_compare(&compare_json("ahead", 3, 0)),
        Ok(SyncState::FetchNeeded { new_commits: 3 })
    );
    assert_eq!(
        parse_compare(&compare_json("behind", 0, 2)),
        Ok(SyncState::RemoteBehind { by: 2 })
    );
    assert_eq!(
        parse_compare(&compare_json("diverged", 4, 1)),
        Ok(SyncState::Diverged { ahead_by: 4, behind_by: 1 })
    );
}

#[test]
fn compare_ignores_the_bulky_parts_of_the_response() {
    let big = format!(
        r#"{{"status": "ahead", "ahead_by": 1, "behind_by": 0, "commits": [{}], "files": [{}]}}"#,
        vec![r#"{"sha": "x"}"#; 50].join(","),
        vec![r#"{"filename": "f"}"#; 50].join(",")
    );
    assert_eq!(parse_compare(&big), Ok(SyncState::FetchNeeded { new_commits: 1 }));
}

#[test]
fn bad_compare_responses_are_errors() {
    for bad in [
        "",
        "[]",
        "{}",
        r#"{"message": "Not Found"}"#,
        r#"{"status": "ahead"}"#,
        r#"{"status": "ahead", "ahead_by": 1}"#,
        r#"{"status": "ahead", "ahead_by": "1", "behind_by": 0}"#,
        r#"{"status": "ahead", "ahead_by": -1, "behind_by": 0}"#,
        r#"{"status": 3, "ahead_by": 1, "behind_by": 0}"#,
        r#"{"status": "sideways", "ahead_by": 1, "behind_by": 0}"#,
    ] {
        assert!(parse_compare(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn only_states_with_new_remote_commits_need_a_fetch() {
    assert!(SyncState::FetchNeeded { new_commits: 1 }.needs_fetch());
    assert!(SyncState::Diverged { ahead_by: 1, behind_by: 1 }.needs_fetch());
    for s in [
        SyncState::Current,
        SyncState::RemoteBehind { by: 2 },
        SyncState::NotOnGitHub,
        SyncState::NoLocalCopy,
        SyncState::Unavailable,
    ] {
        assert!(!s.needs_fetch(), "{s:?}");
    }
}

#[test]
fn combine_carries_the_sync_state_through() {
    let a = combine(&info(1), 0, Ok(None), Some(SyncState::Current));
    assert_eq!(a.sync, Some(SyncState::Current));
    assert_eq!(combine(&info(1), 0, Ok(None), None).sync, None);
}
