use super::ask::*;
use super::answer::Verdict;
use super::ollama::{BackendError, Ollama, Settings};
use crate::facts::{build, Facts};
use crate::repo::{RepoReport, RepoState};
use crate::stale::{StaleItem, StaleReport};
use crate::testutil::{http_response, Behavior, FakeHttp};
use std::path::Path;
use std::time::Duration;

fn st(changed: usize, ahead: u32) -> RepoState {
    RepoState {
        branch: Some("main".into()),
        detached: false,
        unborn: false,
        upstream: Some("origin/main".into()),
        ahead: Some(ahead),
        behind: Some(0),
        staged: 0,
        unstaged: changed,
        untracked: 0,
        conflicted: 0,
        changed_files: changed,
    }
}

fn rep(name: &str, changed: usize, ahead: u32) -> RepoReport {
    RepoReport {
        path: Path::new("/r").join(name),
        name: name.into(),
        state: Ok(st(changed, ahead)),
    }
}

/// Facts (ids shown for the sample):
///  F1 summary, F2 summary, F3 StakOS uncommitted (3 files),
///  F4 ARES unpushed (10 commits), F5 StakOS quiet (108 days),
///  F6 STM32_NEST quiet (200 days)
fn facts() -> Facts {
    let reports = vec![rep("StakOS", 3, 0), rep("ARES", 0, 10), rep("STM32_NEST", 0, 0)];
    let stale = StaleReport {
        days: 90,
        total: 3,
        inactive: vec![
            StaleItem { name: "StakOS".into(), path: Path::new("/r/StakOS").into(), age_days: 108, dirty_files: 3, unpushed: 0 },
            StaleItem { name: "STM32_NEST".into(), path: Path::new("/r/STM32_NEST").into(), age_days: 200, dirty_files: 0, unpushed: 0 },
        ],
        ..StaleReport::default()
    };
    build(&reports, &stale, None)
}


fn ollama(port: u16) -> Ollama {
    Ollama::new(Settings {
        model: "qwen2.5:1.5b".into(),
        port,
        timeout: Duration::from_secs(5),
        unload_after: false,
    })
    .unwrap()
}

/// Wrap `reply` the way Ollama's /api/generate does.
fn generate(reply: &str) -> Behavior {
    let body = serde_json::json!({"model": "m", "response": reply, "done": true}).to_string();
    Behavior::Raw(http_response(200, &body))
}

fn run(reply: &str, alias: bool) -> Outcome {
    let srv = FakeHttp::serve(vec![generate(reply)]);
    ask("PROMPT", &facts(), alias, false, &ollama(srv.addr.port())).unwrap()
}

const GOOD: &str = r#"{"statements":[
  {"text":"StakOS has 3 files uncommitted.","cites":["F3"]},
  {"text":"ARES has 10 commits not yet pushed.","cites":["F4"]}]}"#;

#[test]
fn the_sample_facts_are_what_the_tests_assume() {
    let f = facts();
    assert_eq!(f.find("F3").unwrap().predicate, "has 3 files uncommitted (0 staged, 3 unstaged, 0 untracked).");
    assert_eq!(f.find("F4").unwrap().predicate, "has 10 commits not yet pushed to its upstream.");
    assert_eq!(f.find("F5").unwrap().predicate, "has had no commit for 108 days.");
}

#[test]
fn a_good_reply_is_answered_with_every_statement_verified() {
    match run(GOOD, false) {
        Outcome::Answered { shown, rejected } => {
            assert_eq!(shown.len(), 2);
            assert!(shown.iter().all(|c| c.verdict == Verdict::Verified));
            assert!(rejected.is_empty());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_prompt_reaches_the_model_exactly_once() {
    let srv = FakeHttp::serve(vec![generate(GOOD)]);
    ask("THE PROMPT TEXT", &facts(), false, false, &ollama(srv.addr.port())).unwrap();
    let reqs = srv.requests();
    assert_eq!(reqs.len(), 1);
    assert!(reqs[0].contains("THE PROMPT TEXT"));
}

#[test]
fn a_fenced_reply_still_works() {
    let fenced = format!("```json\n{GOOD}\n```");
    assert!(matches!(run(&fenced, false), Outcome::Answered { .. }));
}

#[test]
fn bad_statements_are_dropped_and_the_good_ones_kept() {
    let reply = r#"{"statements":[
      {"text":"StakOS has 3 files uncommitted.","cites":["F3"]},
      {"text":"ARES has 99 commits not yet pushed.","cites":["F4"]},
      {"text":"Everything is on fire.","cites":["F77"]}]}"#;
    match run(reply, false) {
        Outcome::Answered { shown, rejected } => {
            assert_eq!(shown.len(), 1);
            assert_eq!(shown[0].statement.cites, vec!["F3".to_string()]);
            assert_eq!(rejected.len(), 2);
            assert!(rejected[0].contains("number 99"));
            assert!(rejected[1].contains("F77"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_reply_citing_only_invented_facts_falls_back() {
    let reply = r#"{"statements":[{"text":"Nothing is wrong.","cites":["F40"]},{"text":"All good.","cites":["F41","F42"]}]}"#;
    match run(reply, false) {
        Outcome::Unusable { reason, rejected } => {
            assert_eq!(reason, "none of the model's statements passed the checks");
            assert_eq!(rejected.len(), 2);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn garbage_replies_fall_back_with_a_reason() {
    for (reply, want) in [
        ("I think you should look at StakOS.", "no JSON object"),
        ("{not json}", "not valid JSON"),
        (r#"{"answer":"hello"}"#, "no list of statements"),
        ("", "no JSON object"),
        (r#"{"statements":[]}"#, "no statements"),
        (r#"{"statements":[{"text":"","cites":[]},{"cites":["F1"]}]}"#, "no statements"),
    ] {
        match run(reply, false) {
            Outcome::Unusable { reason, .. } => assert!(reason.contains(want), "{reply:?}: {reason}"),
            other => panic!("{reply:?}: {other:?}"),
        }
    }
}

#[test]
fn only_uncited_remarks_do_not_count_as_an_answer() {
    let reply = r#"{"statements":[{"text":"The facts do not say.","cites":[]}]}"#;
    match run(reply, false) {
        Outcome::Unusable { reason, rejected } => {
            assert!(reason.contains("no statement backed by a measured fact"));
            assert!(rejected.is_empty());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_uncited_remark_next_to_a_verified_statement_is_kept_and_marked() {
    let reply = r#"{"statements":[
      {"text":"StakOS has 3 files uncommitted.","cites":["F3"]},
      {"text":"Worth a look soon.","cites":[]}]}"#;
    match run(reply, false) {
        Outcome::Answered { shown, .. } => {
            assert_eq!(shown[0].verdict, Verdict::Verified);
            assert_eq!(shown[1].verdict, Verdict::Inferred);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_truncated_or_unreadable_server_reply_is_unusable_not_an_error() {
    let cut = Behavior::Raw(http_response(200, r#"{"response":"{\"statem","done":true,"done_reason":"length"}"#));
    let srv = FakeHttp::serve(vec![cut]);
    match ask("P", &facts(), false, false, &ollama(srv.addr.port())).unwrap() {
        Outcome::Unusable { reason, .. } => assert!(reason.contains("cut off")),
        other => panic!("{other:?}"),
    }
    let odd = Behavior::Raw(http_response(200, "not json at all"));
    let srv = FakeHttp::serve(vec![odd]);
    match ask("P", &facts(), false, false, &ollama(srv.addr.port())).unwrap() {
        Outcome::Unusable { reason, .. } => assert!(reason.contains("not understood")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn backend_problems_are_errors() {
    // Nothing listening: bind a port, then free it.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    assert_eq!(
        ask("P", &facts(), false, false, &ollama(port)).unwrap_err(),
        BackendError::NotRunning(port)
    );
    let srv = FakeHttp::serve(vec![Behavior::Raw(http_response(404, r#"{"error":"model 'x' not found"}"#))]);
    assert!(matches!(
        ask("P", &facts(), false, false, &ollama(srv.addr.port())).unwrap_err(),
        BackendError::ModelMissing(_)
    ));
}

#[test]
fn aliased_replies_are_checked_with_aliases_and_shown_with_real_names() {
    // In the payload StakOS is R1, ARES is R2.
    let reply = r#"{"statements":[
      {"text":"R1 has 3 files uncommitted.","cites":["F3"]},
      {"text":"R2 has 10 commits not yet pushed.","cites":["F4"]}]}"#;
    let facts = facts();
    match judge(reply, &facts, true, false) {
        Outcome::Answered { shown, rejected } => {
            assert_eq!(shown.len(), 2);
            let text = render_answer(&shown, &rejected, &facts, true, "m");
            assert!(text.contains("- StakOS has 3 files uncommitted. [F3]\n"), "{text}");
            assert!(text.contains("- ARES has 10 commits not yet pushed. [F4]\n"), "{text}");
            assert!(!text.contains("R1"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn rendering_an_answer_shows_citations_marks_inferred_and_lists_rejections() {
    let reply = r#"{"statements":[
      {"text":"StakOS has 3 files uncommitted.","cites":["F3","F5"]},
      {"text":"Worth a look soon.","cites":[]},
      {"text":"ARES has 77 commits not yet pushed.","cites":["F4"]}]}"#;
    let f = facts();
    let Outcome::Answered { shown, rejected } = judge(reply, &f, false, false) else { panic!() };
    let text = render_answer(&shown, &rejected, &f, false, "qwen2.5:1.5b");
    assert_eq!(
        text,
        "- StakOS has 3 files uncommitted. [F3, F5]\n\
         - Worth a look soon. (inferred, no fact cited)\n\
         \n\
         1 statement was rejected by the checks:\n\
         \x20 - mentioned the number 77, which is not in the cited facts\n\
         \n\
         Answered by qwen2.5:1.5b through Ollama on this machine; statements are checked against measured facts, this is a summary not advice.\n"
    );
}

#[test]
fn many_rejections_are_summarised() {
    let rejected: Vec<String> = (1..=8).map(|i| format!("reason {i}")).collect();
    let text = render_answer(&[], &rejected, &facts(), false, "m");
    assert!(text.contains("8 statements were rejected by the checks:"));
    assert!(text.contains("reason 5\n"));
    assert!(!text.contains("reason 6"));
    assert!(text.contains("and 3 more"));
}

#[test]
fn the_fallback_lists_every_fact_with_its_id_and_real_names() {
    let text = render_facts(&facts());
    assert!(text.starts_with("Measured facts:\n"));
    assert!(text.contains("  F1   3 repositories were scanned.\n"), "{text}");
    assert!(text.contains("  F3   StakOS has 3 files uncommitted (0 staged, 3 unstaged, 0 untracked).\n"), "{text}");
    assert!(text.contains("  F6   STM32_NEST has had no commit for 200 days.\n"), "{text}");
    assert!(!text.contains("more"));
}

#[test]
fn the_fallback_is_capped() {
    let mut f = Facts::default();
    for i in 1..=70 {
        f.facts.push(crate::facts::Fact {
            id: i,
            kind: crate::facts::Kind::Summary,
            subject: None,
            predicate: format!("fact number {i}."),
        });
    }
    f.omitted = 5;
    let text = render_facts(&f);
    assert!(text.contains("fact number 60."));
    assert!(!text.contains("fact number 61."));
    assert!(text.contains("... and 15 more"));
}
