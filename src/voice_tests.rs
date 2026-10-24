use crate::repo::{RepoReport, RepoState};
use crate::voice::{pick, seed_from, Summary, Voice};
use std::collections::HashSet;
use std::path::Path;

fn state(changed: usize, ahead: Option<u32>, conflicted: usize) -> RepoState {
    RepoState {
        branch: Some("main".into()),
        detached: false,
        unborn: false,
        upstream: ahead.map(|_| "origin/main".to_string()),
        ahead,
        behind: ahead.map(|_| 0),
        staged: 0,
        unstaged: changed,
        untracked: 0,
        conflicted,
        changed_files: changed,
    }
}

fn report(name: &str, st: Result<RepoState, String>) -> RepoReport {
    RepoReport::new(&Path::new("/r").join(name), st)
}

fn numbers(s: &str) -> HashSet<u64> {
    s.split(|c: char| !c.is_ascii_digit())
        .filter(|t| !t.is_empty())
        .map(|t| t.parse().unwrap())
        .collect()
}

fn summary() -> Summary {
    Summary {
        total: 17,
        tidy: 10,
        dirty: 5,
        unpushed: 3,
        errors: 2,
    }
}

#[test]
fn voice_names_parse_case_insensitively() {
    assert_eq!(Voice::from_name("plain"), Some(Voice::Plain));
    assert_eq!(Voice::from_name("  Cheerful "), Some(Voice::Cheerful));
    assert_eq!(Voice::from_name("DEADPAN"), Some(Voice::Deadpan));
    assert_eq!(Voice::from_name("nope"), None);
    assert_eq!(Voice::from_name(""), None);
}

#[test]
fn names_lists_every_voice() {
    let n = Voice::names();
    for v in Voice::ALL {
        assert!(n.contains(v.name()));
    }
}

#[test]
fn summary_counts_each_category() {
    let reports = vec![
        report("tidy", Ok(state(0, Some(0), 0))),
        report("dirty", Ok(state(3, Some(0), 0))),
        report("unpushed", Ok(state(0, Some(2), 0))),
        report("both", Ok(state(1, Some(1), 0))),
        report("broken", Err("not a git repository".into())),
    ];
    let s = Summary::from_reports(&reports);
    assert_eq!(
        s,
        Summary {
            total: 5,
            tidy: 1,
            dirty: 2,
            unpushed: 2,
            errors: 1
        }
    );
}

#[test]
fn plain_headline_is_exact_and_neutral() {
    let none = Summary { total: 0, tidy: 0, dirty: 0, unpushed: 0, errors: 0 };
    assert_eq!(Voice::Plain.headline(&none, 0), "No repositories found.");

    let one = Summary { total: 1, tidy: 0, dirty: 1, unpushed: 0, errors: 0 };
    assert_eq!(
        Voice::Plain.headline(&one, 0),
        "1 repository: 1 with uncommitted changes, 0 with unpushed commits."
    );

    assert_eq!(
        Voice::Plain.headline(&summary(), 0),
        "17 repositories: 5 with uncommitted changes, 3 with unpushed commits. 2 could not be read."
    );
}

#[test]
fn plain_never_adds_flavor() {
    let s = summary();
    assert_eq!(Voice::Plain.footer(&s, 1), None);
    let r = report("big", Ok(state(50, Some(9), 0)));
    assert_eq!(Voice::Plain.repo_note(&r, 1), None);
}

#[test]
fn every_voice_and_seed_states_the_real_numbers() {
    let s = summary();
    for v in Voice::ALL {
        for seed in 0..64 {
            let nums = numbers(&v.headline(&s, seed));
            for want in [17, 5, 3, 2] {
                assert!(nums.contains(&want), "{:?} seed {} lost {}", v, seed, want);
            }
        }
    }
}

#[test]
fn flavored_voices_also_state_tidy_count() {
    let s = summary();
    for v in [Voice::Cheerful, Voice::Deadpan] {
        for seed in 0..64 {
            assert!(numbers(&v.headline(&s, seed)).contains(&10));
        }
    }
}

#[test]
fn all_good_headline_states_the_total() {
    let s = Summary { total: 4, tidy: 4, dirty: 0, unpushed: 0, errors: 0 };
    for v in Voice::ALL {
        for seed in 0..32 {
            assert!(numbers(&v.headline(&s, seed)).contains(&4), "{v:?}");
        }
    }
}

#[test]
fn read_errors_are_stated_plainly_in_every_voice() {
    let s = summary();
    for v in Voice::ALL {
        for seed in 0..32 {
            assert!(
                v.headline(&s, seed).contains("2 could not be read."),
                "{v:?} seed {seed}"
            );
        }
    }
}

#[test]
fn zero_repos_is_handled_in_every_voice() {
    let s = Summary { total: 0, tidy: 0, dirty: 0, unpushed: 0, errors: 0 };
    for v in Voice::ALL {
        let h = v.headline(&s, 7);
        assert!(h.contains("No repositories found"), "{v:?}: {h}");
    }
}

#[test]
fn flavored_voices_differ_from_plain() {
    let s = summary();
    let plain = Voice::Plain.headline(&s, 3);
    assert_ne!(Voice::Cheerful.headline(&s, 3), plain);
    assert_ne!(Voice::Deadpan.headline(&s, 3), plain);
}

#[test]
fn output_is_deterministic_for_a_seed_and_varies_across_seeds() {
    let s = summary();
    for v in [Voice::Cheerful, Voice::Deadpan] {
        assert_eq!(v.headline(&s, 42), v.headline(&s, 42));
        let distinct: HashSet<String> = (0..64).map(|seed| v.headline(&s, seed)).collect();
        assert!(distinct.len() > 1, "{v:?} never varied");
    }
}

#[test]
fn footers_depend_on_what_is_pending() {
    let good = Summary { total: 3, tidy: 3, dirty: 0, unpushed: 0, errors: 0 };
    let dirty = Summary { total: 3, tidy: 1, dirty: 2, unpushed: 0, errors: 0 };
    let unpushed = Summary { total: 3, tidy: 2, dirty: 0, unpushed: 1, errors: 0 };
    for v in [Voice::Cheerful, Voice::Deadpan] {
        assert_eq!(v.footer(&good, 1), None, "{v:?}");
        assert!(v.footer(&dirty, 1).is_some(), "{v:?}");
        assert!(v.footer(&unpushed, 1).is_some(), "{v:?}");
    }
}

#[test]
fn notes_appear_only_for_notable_repos() {
    let quiet = report("quiet", Ok(state(2, Some(1), 0)));
    let big = report("big", Ok(state(14, Some(0), 0)));
    let ahead = report("ahead", Ok(state(0, Some(6), 0)));
    for v in [Voice::Cheerful, Voice::Deadpan] {
        assert_eq!(v.repo_note(&quiet, 1), None, "{v:?}");
        let n = v.repo_note(&big, 1).expect("note for many changes");
        assert!(numbers(&n).contains(&14), "{v:?}: {n}");
        let a = v.repo_note(&ahead, 1).expect("note for many unpushed");
        assert!(numbers(&a).contains(&6), "{v:?}: {a}");
    }
}

#[test]
fn conflicts_and_errors_never_get_flavor() {
    let conflicted = report("c", Ok(state(30, Some(5), 2)));
    let broken = report("b", Err("boom".into()));
    for v in Voice::ALL {
        assert_eq!(v.repo_note(&conflicted, 1), None, "{v:?}");
        assert_eq!(v.repo_note(&broken, 1), None, "{v:?}");
    }
}

#[test]
fn all_wording_is_plain_ascii() {
    let sums = [
        summary(),
        Summary { total: 4, tidy: 4, dirty: 0, unpushed: 0, errors: 0 },
        Summary { total: 0, tidy: 0, dirty: 0, unpushed: 0, errors: 0 },
        Summary { total: 3, tidy: 2, dirty: 0, unpushed: 1, errors: 0 },
        Summary { total: 3, tidy: 1, dirty: 2, unpushed: 0, errors: 0 },
    ];
    let big = report("big", Ok(state(14, Some(0), 0)));
    let ahead = report("ahead", Ok(state(0, Some(6), 0)));
    for v in Voice::ALL {
        for seed in 0..64 {
            for s in &sums {
                assert!(v.headline(s, seed).is_ascii());
                assert!(v.footer(s, seed).map_or(true, |t| t.is_ascii()));
            }
            assert!(v.repo_note(&big, seed).map_or(true, |t| t.is_ascii()));
            assert!(v.repo_note(&ahead, seed).map_or(true, |t| t.is_ascii()));
        }
    }
}

#[test]
fn seeds_are_stable_and_sensitive_to_inputs() {
    assert_eq!(seed_from("StakML", 100), seed_from("StakML", 100));
    assert_ne!(seed_from("StakML", 100), seed_from("StakML", 101));
    assert_ne!(seed_from("StakML", 100), seed_from("StakOS", 100));
}

#[test]
fn pick_stays_in_range_and_covers_all_options() {
    let mut seen = HashSet::new();
    for seed in 0..200 {
        let i = pick(seed, 3);
        assert!(i < 3);
        seen.insert(i);
    }
    assert_eq!(seen.len(), 3);
}
