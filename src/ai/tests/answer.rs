use crate::ai::answer::*;
use crate::facts::{build, Facts};
use crate::repo::{RepoReport, RepoState};
use crate::stale::{StaleItem, StaleReport};
use std::path::Path;

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

fn stmt(text: &str, cites: &[&str]) -> Statement {
    Statement {
        text: text.into(),
        cites: cites.iter().map(|c| c.to_string()).collect(),
    }
}

fn verdict(text: &str, cites: &[&str]) -> Verdict {
    check(&[stmt(text, cites)], &facts(), false).remove(0).verdict
}

fn rejected(v: &Verdict) -> &str {
    match v {
        Verdict::Rejected(r) => r,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

#[test]
fn the_sample_facts_are_what_the_tests_assume() {
    let f = facts();
    assert_eq!(f.find("F3").unwrap().predicate, "has 3 files uncommitted (0 staged, 3 unstaged, 0 untracked).");
    assert_eq!(f.find("F4").unwrap().predicate, "has 10 commits not yet pushed to its upstream.");
    assert_eq!(f.find("F5").unwrap().predicate, "has had no commit for 108 days.");
    assert_eq!(f.find("F6").unwrap().predicate, "has had no commit for 200 days.");
}

// ---------- parsing ----------

#[test]
fn a_plain_reply_parses() {
    let r = parse(r#"{"statements":[{"text":"StakOS is quiet.","cites":["F5"]}]}"#).unwrap();
    assert_eq!(r, vec![stmt("StakOS is quiet.", &["F5"])]);
}

#[test]
fn code_fences_and_surrounding_text_are_tolerated() {
    let json = r#"{"statements":[{"text":"x","cites":[]}]}"#;
    for wrapped in [
        format!("```json\n{json}\n```"),
        format!("```\n{json}\n```"),
        format!("Here is the answer:\n{json}\nHope that helps!"),
        format!("  \n{json}  \n"),
    ] {
        assert_eq!(parse(&wrapped).unwrap().len(), 1, "{wrapped:?}");
    }
}

#[test]
fn replies_without_the_expected_shape_are_errors() {
    for bad in [
        "",
        "I cannot help with that.",
        "{",
        "}{",
        "{not json}",
        "[]",
        r#"{"answer":"x"}"#,
        r#"{"statements":"x"}"#,
        r#"{"statements":{"text":"x"}}"#,
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn an_empty_list_is_valid_and_empty() {
    assert_eq!(parse(r#"{"statements":[]}"#).unwrap(), vec![]);
}

#[test]
fn malformed_items_are_skipped_and_the_rest_kept() {
    let r = parse(r#"{"statements":[1,"x",{"cites":["F1"]},{"text":5},{"text":"   "},{"text":"ok","cites":["F1"]}]}"#).unwrap();
    assert_eq!(r, vec![stmt("ok", &["F1"])]);
}

#[test]
fn cites_are_cleaned_deduplicated_and_non_strings_ignored() {
    let r = parse(r#"{"statements":[{"text":"t","cites":["F1","F1"," F2 ",3,null,"F3\n",""]}]}"#).unwrap();
    assert_eq!(r[0].cites, vec!["F1", "F2", "F3"]);
    let none = parse(r#"{"statements":[{"text":"t","cites":"F1"}]}"#).unwrap();
    assert!(none[0].cites.is_empty());
}

#[test]
fn text_is_flattened_and_capped_and_lists_are_limited() {
    let r = parse(r#"{"statements":[{"text":"a\nb\t\u001b[2Jc   d","cites":[]}]}"#).unwrap();
    assert_eq!(r[0].text, "a b [2Jc d");
    let long = format!(r#"{{"statements":[{{"text":"{}","cites":[]}}]}}"#, "x".repeat(5000));
    assert_eq!(parse(&long).unwrap()[0].text.chars().count(), MAX_TEXT);
    let many: Vec<String> = (0..100).map(|i| format!(r#"{{"text":"s{i}","cites":[]}}"#)).collect();
    let r = parse(&format!(r#"{{"statements":[{}]}}"#, many.join(","))).unwrap();
    assert_eq!(r.len(), MAX_STATEMENTS);
    let cites: Vec<String> = (0..100).map(|i| format!("\"F{i}\"")).collect();
    let r = parse(&format!(r#"{{"statements":[{{"text":"t","cites":[{}]}}]}}"#, cites.join(","))).unwrap();
    assert_eq!(r[0].cites.len(), MAX_CITES);
}

// ---------- checking ----------

#[test]
fn a_statement_that_matches_its_citations_is_verified() {
    assert_eq!(verdict("StakOS has had no commit for 108 days.", &["F5"]), Verdict::Verified);
    assert_eq!(verdict("StakOS has 3 files uncommitted and no commit for 108 days.", &["F3", "F5"]), Verdict::Verified);
}

#[test]
fn a_statement_about_several_repositories_needs_a_citation_for_each() {
    assert_eq!(verdict("StakOS and STM32_NEST are quiet.", &["F5", "F6"]), Verdict::Verified);
    let r = verdict("StakOS and STM32_NEST are quiet.", &["F5"]);
    assert!(rejected(&r).contains("STM32_NEST"), "{r:?}");
}

#[test]
fn unknown_and_non_canonical_citations_are_rejected() {
    for cite in ["F99", "F0", "F03", "f5", "F+5", "5", "", "FACT5"] {
        let v = verdict("StakOS is quiet.", &[cite]);
        assert!(rejected(&v).contains("not one of the facts"), "{cite:?}: {v:?}");
    }
}

#[test]
fn one_bad_citation_rejects_the_statement_even_if_others_are_fine() {
    let v = verdict("StakOS has had no commit for 108 days.", &["F5", "F77"]);
    assert!(rejected(&v).contains("F77"));
}

#[test]
fn invented_numbers_are_rejected() {
    let v = verdict("StakOS has had no commit for 200 days.", &["F5"]);
    assert!(rejected(&v).contains("number 200"), "{v:?}");
    let v = verdict("StakOS has 4 files uncommitted.", &["F3"]);
    assert!(rejected(&v).contains("number 4"), "{v:?}");
}

#[test]
fn numbers_from_any_cited_fact_or_the_repository_name_are_allowed() {
    // 108 from F5, 3 from F3.
    assert_eq!(verdict("StakOS: 3 files uncommitted, quiet for 108 days.", &["F3", "F5"]), Verdict::Verified);
    // The 32 in the repository name is part of the subject of F6.
    assert_eq!(verdict("STM32_NEST has had no commit for 200 days.", &["F6"]), Verdict::Verified);
    // A number from a fact that was not cited is not allowed.
    assert!(matches!(verdict("StakOS has had no commit for 200 days.", &["F5", "F3"]), Verdict::Rejected(_)));
}

#[test]
fn numbers_are_matched_as_whole_runs_not_substrings() {
    // "10" is in F4, but "100" is not the same number.
    assert!(matches!(verdict("ARES has 100 commits not yet pushed.", &["F4"]), Verdict::Rejected(_)));
    assert_eq!(verdict("ARES has 10 commits not yet pushed.", &["F4"]), Verdict::Verified);
    assert!(matches!(verdict("ARES has 1 commit not yet pushed.", &["F4"]), Verdict::Rejected(_)));
}

#[test]
fn a_claim_attached_to_the_wrong_repository_is_rejected() {
    // F4 is about ARES, not StakOS.
    let v = verdict("StakOS has 10 commits not yet pushed.", &["F4"]);
    assert!(rejected(&v).contains("StakOS"), "{v:?}");
}

#[test]
fn statements_without_citations_may_only_be_general() {
    assert_eq!(verdict("The facts do not say anything about that.", &[]), Verdict::Inferred);
    // A general remark that cites a real fact and makes no specific claim passes.
    assert_eq!(verdict("Most projects look healthy.", &["F1"]), Verdict::Verified);
    let v = verdict("About 5 projects need work.", &[]);
    assert!(rejected(&v).contains("without citing"), "{v:?}");
    let v = verdict("StakOS probably needs attention.", &[]);
    assert!(rejected(&v).contains("without citing"), "{v:?}");
}

#[test]
fn names_inside_longer_words_do_not_count_as_mentions() {
    // "ARES" appears inside SQUARES, but not as a word of its own.
    assert_eq!(verdict("SQUARES are quiet, as the facts say.", &["F5"]), Verdict::Verified);
    assert_eq!(verdict("The word SQUARES is not a repository.", &[]), Verdict::Inferred);
    assert!(matches!(verdict("ARES is fine.", &[]), Verdict::Rejected(_)));
    assert!(matches!(verdict("(ARES), please.", &[]), Verdict::Rejected(_)));
    assert!(matches!(verdict("Is ARES? Yes.", &[]), Verdict::Rejected(_)));
}

#[test]
fn names_with_punctuation_are_matched_whole() {
    let reports = vec![rep("my-app.v2", 1, 0), rep("my-app", 0, 0)];
    let f = build(&reports, &StaleReport { days: 90, ..StaleReport::default() }, None);
    // F3 is about my-app.v2.
    let c = check(&[stmt("my-app.v2 has 1 file uncommitted.", &["F3"])], &f, false);
    assert_eq!(c[0].verdict, Verdict::Verified);
    // Mentioning the shorter name is a different repository and is not covered.
    let c = check(&[stmt("my-app has 1 file uncommitted.", &["F3"])], &f, false);
    assert!(matches!(c[0].verdict, Verdict::Rejected(_)), "{:?}", c[0].verdict);
}

#[test]
fn alias_mode_checks_against_aliases_not_real_names() {
    let f = facts();
    let ok = check(&[stmt("R1 has 3 files uncommitted.", &["F3"])], &f, true);
    assert_eq!(ok[0].verdict, Verdict::Verified);
    let wrong = check(&[stmt("R2 has 3 files uncommitted.", &["F3"])], &f, true);
    assert!(rejected(&wrong[0].verdict).contains("R2"));
    // R1 must not be confused with R10 or R11.
    let reports: Vec<RepoReport> = (0..12).map(|i| rep(&format!("p{i}"), if i == 0 { 1 } else { 0 }, 0)).collect();
    let f12 = build(&reports, &StaleReport { days: 90, ..StaleReport::default() }, None);
    let c = check(&[stmt("R1 has 1 file uncommitted.", &["F3"])], &f12, true);
    assert_eq!(c[0].verdict, Verdict::Verified);
    let c = check(&[stmt("R10 has 1 file uncommitted.", &["F3"])], &f12, true);
    assert!(matches!(c[0].verdict, Verdict::Rejected(_)));
}

#[test]
fn de_aliasing_restores_real_names_only_for_exact_alias_tokens() {
    let f = facts();
    assert_eq!(de_alias("R1 and R2 are quiet.", &f), "StakOS and ARES are quiet.");
    assert_eq!(de_alias("(R3).", &f), "(STM32_NEST).");
    for untouched in ["R99 stays", "R0 stays", "R1x stays", "xR1 stays", "R stays", "R-1 stays", "AR1 stays"] {
        let out = de_alias(untouched, &f);
        assert!(!out.contains("StakOS") || untouched.starts_with("R-1"), "{untouched} -> {out}");
    }
    assert_eq!(de_alias("R1x R99 xR1", &f), "R1x R99 xR1");
    assert_eq!(de_alias("caf\u{e9} R1 \u{4e2d}", &f), "caf\u{e9} StakOS \u{4e2d}");
}

#[test]
fn review_splits_shown_from_rejected_and_keeps_reasons() {
    let f = facts();
    let checked = check(
        &[
            stmt("StakOS has had no commit for 108 days.", &["F5"]),
            stmt("StakOS has had no commit for 999 days.", &["F5"]),
            stmt("The facts do not cover that.", &[]),
            stmt("Nothing is fine.", &["F42"]),
        ],
        &f,
        false,
    );
    let r = review(checked);
    assert_eq!(r.shown.len(), 2);
    assert_eq!(r.shown[0].verdict, Verdict::Verified);
    assert_eq!(r.shown[1].verdict, Verdict::Inferred);
    assert_eq!(r.rejected.len(), 2);
    assert!(r.rejected[0].contains("999"));
    assert!(r.rejected[1].contains("F42"));
}

#[test]
fn a_statement_cannot_smuggle_text_that_looks_like_a_citation_into_verification() {
    // Citing is done only through the `cites` list; "[F5]" in the text does not count.
    let v = verdict("StakOS has had no commit for 108 days [F5].", &[]);
    assert!(matches!(v, Verdict::Rejected(_)), "{v:?}");
}

#[test]
fn checking_is_deterministic_and_leaves_the_statements_unchanged() {
    let f = facts();
    let s = vec![stmt("StakOS is quiet for 108 days.", &["F5"])];
    let a = check(&s, &f, false);
    assert_eq!(a, check(&s, &f, false));
    assert_eq!(a[0].statement, s[0]);
}

#[test]
fn a_longer_name_claims_its_text_but_a_separate_mention_of_the_shorter_one_still_counts() {
    let reports = vec![rep("my-app.v2", 1, 0), rep("my-app", 0, 0)];
    let f = build(&reports, &StaleReport { days: 90, ..StaleReport::default() }, None);
    let c = check(&[stmt("my-app.v2 has 1 file uncommitted, unlike my-app.", &["F3"])], &f, false);
    assert!(rejected(&c[0].verdict).contains("my-app,") || rejected(&c[0].verdict).contains("my-app"), "{:?}", c[0].verdict);
}

#[test]
fn the_wrong_repository_is_reported_before_a_number_mismatch() {
    // Both are wrong here; the repository is the more useful thing to say.
    let v = verdict("ARES has 999 files uncommitted.", &["F3"]);
    assert!(rejected(&v).contains("ARES"), "{v:?}");
}
