use super::status::*;

const OID: &str = "0123456789abcdef0123456789abcdef01234567";
const H: &str = "1111111111111111111111111111111111111111";

/// Join records with NUL, as `git status -z` does.
fn z(records: &[&str]) -> String {
    let mut s = records.join("\0");
    s.push('\0');
    s
}

#[test]
fn empty_input_is_clean() {
    let s = parse("").unwrap();
    assert!(!s.is_dirty());
    assert_eq!(s, Status::default());
}

#[test]
fn clean_repo_with_upstream() {
    let input = z(&[
        &format!("# branch.oid {OID}"),
        "# branch.head main",
        "# branch.upstream origin/main",
        "# branch.ab +0 -0",
    ]);
    let s = parse(&input).unwrap();
    assert_eq!(s.branch.oid.as_deref(), Some(OID));
    assert_eq!(s.branch.head.as_deref(), Some("main"));
    assert!(!s.branch.detached);
    assert_eq!(s.branch.upstream.as_deref(), Some("origin/main"));
    assert_eq!(s.branch.ahead_behind, Some((0, 0)));
    assert!(!s.is_dirty());
}

#[test]
fn ahead_and_behind_counts() {
    let input = z(&["# branch.head main", "# branch.upstream origin/main", "# branch.ab +3 -12"]);
    let s = parse(&input).unwrap();
    assert_eq!(s.branch.ahead_behind, Some((3, 12)));
}

#[test]
fn no_upstream_means_no_ahead_behind() {
    let input = z(&[&format!("# branch.oid {OID}"), "# branch.head feature"]);
    let s = parse(&input).unwrap();
    assert_eq!(s.branch.upstream, None);
    assert_eq!(s.branch.ahead_behind, None);
}

#[test]
fn initial_commit_has_no_oid() {
    let input = z(&["# branch.oid (initial)", "# branch.head main"]);
    let s = parse(&input).unwrap();
    assert_eq!(s.branch.oid, None);
    assert_eq!(s.branch.head.as_deref(), Some("main"));
}

#[test]
fn detached_head() {
    let input = z(&[&format!("# branch.oid {OID}"), "# branch.head (detached)"]);
    let s = parse(&input).unwrap();
    assert!(s.branch.detached);
    assert_eq!(s.branch.head, None);
}

#[test]
fn unstaged_modification() {
    let e = format!("1 .M N... 100644 100644 100644 {H} {H} src/main.rs");
    let s = parse(&z(&[&e])).unwrap();
    assert_eq!(s.entries.len(), 1);
    assert_eq!(s.entries[0].index, FileState::Unmodified);
    assert_eq!(s.entries[0].worktree, FileState::Modified);
    assert_eq!(s.entries[0].path, "src/main.rs");
    assert_eq!((s.staged(), s.unstaged()), (0, 1));
    assert!(s.is_dirty());
}

#[test]
fn staged_addition() {
    let e = format!("1 A. N... 000000 100644 100644 {H} {H} new.txt");
    let s = parse(&z(&[&e])).unwrap();
    assert_eq!(s.entries[0].index, FileState::Added);
    assert_eq!((s.staged(), s.unstaged()), (1, 0));
}

#[test]
fn staged_and_modified_counts_in_both() {
    let e = format!("1 MM N... 100644 100644 100644 {H} {H} lib.rs");
    let s = parse(&z(&[&e])).unwrap();
    assert_eq!((s.staged(), s.unstaged()), (1, 1));
}

#[test]
fn deleted_file() {
    let e = format!("1 .D N... 100644 100644 000000 {H} {H} gone.txt");
    let s = parse(&z(&[&e])).unwrap();
    assert_eq!(s.entries[0].worktree, FileState::Deleted);
}

#[test]
fn path_with_spaces_is_preserved() {
    let e = format!("1 .M N... 100644 100644 100644 {H} {H} my notes/file name.md");
    let s = parse(&z(&[&e])).unwrap();
    assert_eq!(s.entries[0].path, "my notes/file name.md");
}

#[test]
fn path_with_newline_is_preserved() {
    let e = format!("1 .M N... 100644 100644 100644 {H} {H} odd\nname.txt");
    let s = parse(&z(&[&e])).unwrap();
    assert_eq!(s.entries[0].path, "odd\nname.txt");
}

#[test]
fn rename_consumes_original_path_record() {
    let rename = format!("2 R. N... 100644 100644 100644 {H} {H} R100 new name.rs");
    let after = format!("1 .M N... 100644 100644 100644 {H} {H} other.rs");
    let s = parse(&z(&[&rename, "old name.rs", &after])).unwrap();
    assert_eq!(s.entries.len(), 2);
    assert_eq!(s.entries[0].kind, EntryKind::RenamedOrCopied);
    assert_eq!(s.entries[0].index, FileState::Renamed);
    assert_eq!(s.entries[0].path, "new name.rs");
    assert_eq!(s.entries[0].orig_path.as_deref(), Some("old name.rs"));
    assert_eq!(s.entries[1].path, "other.rs");
}

#[test]
fn rename_missing_original_path_is_an_error() {
    let rename = format!("2 R. N... 100644 100644 100644 {H} {H} R100 new.rs");
    let e = parse(&z(&[&rename])).unwrap_err();
    assert!(e.message.contains("original path"));
}

#[test]
fn unmerged_entry_is_conflicted_not_staged() {
    let e = format!("u UU N... 100644 100644 100644 100644 {H} {H} {H} conflict.c");
    let s = parse(&z(&[&e])).unwrap();
    assert_eq!(s.conflicted(), 1);
    assert_eq!((s.staged(), s.unstaged()), (0, 0));
    assert_eq!(s.entries[0].path, "conflict.c");
    assert!(s.is_dirty());
}

#[test]
fn untracked_files_make_repo_dirty() {
    let s = parse(&z(&["? scratch.txt", "? build/out.bin"])).unwrap();
    assert_eq!(s.untracked_count(), 2);
    assert!(s.is_dirty());
}

#[test]
fn ignored_files_do_not_make_repo_dirty() {
    let s = parse(&z(&["! target/"])).unwrap();
    assert_eq!(s.ignored, vec!["target/".to_string()]);
    assert!(!s.is_dirty());
}

#[test]
fn unknown_header_is_ignored() {
    let s = parse(&z(&["# stash 2", "# branch.head main"])).unwrap();
    assert_eq!(s.branch.head.as_deref(), Some("main"));
}

#[test]
fn malformed_branch_ab_is_an_error() {
    let e = parse(&z(&["# branch.ab +x -1"])).unwrap_err();
    assert_eq!(e.record, 0);
    assert!(e.message.contains("branch.ab"));
}

#[test]
fn bad_status_code_is_an_error() {
    let e = format!("1 ZZ N... 100644 100644 100644 {H} {H} f");
    assert!(parse(&z(&[&e])).is_err());
}

#[test]
fn truncated_entry_is_an_error() {
    let e = parse(&z(&["1 .M N... 100644"])).unwrap_err();
    assert!(e.message.contains("too few fields"));
}

#[test]
fn unrecognized_record_reports_its_index() {
    let e = parse(&z(&["# branch.head main", "garbage"])).unwrap_err();
    assert_eq!(e.record, 1);
}

#[test]
fn full_realistic_status() {
    let m = format!("1 .M N... 100644 100644 100644 {H} {H} src/a.rs");
    let a = format!("1 A. N... 000000 100644 100644 {H} {H} src/b.rs");
    let input = z(&[
        &format!("# branch.oid {OID}"),
        "# branch.head dev",
        "# branch.upstream origin/dev",
        "# branch.ab +2 -0",
        &m,
        &a,
        "? notes.txt",
    ]);
    let s = parse(&input).unwrap();
    assert_eq!(s.branch.ahead_behind, Some((2, 0)));
    assert_eq!((s.staged(), s.unstaged(), s.untracked_count()), (1, 1, 1));
}
