use crate::github::{parse_remote, RepoRef};

fn r(owner: &str, name: &str) -> Option<RepoRef> {
    Some(RepoRef {
        owner: owner.into(),
        name: name.into(),
    })
}

#[test]
fn https_urls_with_and_without_git_suffix() {
    assert_eq!(parse_remote("https://github.com/STAKXX002/ARES.git"), r("STAKXX002", "ARES"));
    assert_eq!(parse_remote("https://github.com/STAKXX002/ARES"), r("STAKXX002", "ARES"));
    assert_eq!(parse_remote("https://github.com/o/n/"), r("o", "n"));
    assert_eq!(parse_remote("http://github.com/o/n.git"), r("o", "n"));
}

#[test]
fn names_may_contain_dots_underscores_and_hyphens() {
    assert_eq!(parse_remote("https://github.com/STAKXX002/A.R.E.S..git"), r("STAKXX002", "A.R.E.S."));
    assert_eq!(parse_remote("git@github.com:my-org/my_repo.v2.git"), r("my-org", "my_repo.v2"));
}

#[test]
fn scp_like_ssh_syntax() {
    assert_eq!(parse_remote("git@github.com:o/n.git"), r("o", "n"));
    assert_eq!(parse_remote("git@github.com:o/n"), r("o", "n"));
    assert_eq!(parse_remote("github.com:o/n.git"), r("o", "n"));
    assert_eq!(parse_remote("git@github.com:/o/n.git"), r("o", "n"));
}

#[test]
fn ssh_and_git_scheme_urls() {
    assert_eq!(parse_remote("ssh://git@github.com/o/n.git"), r("o", "n"));
    assert_eq!(parse_remote("ssh://git@github.com:22/o/n.git"), r("o", "n"));
    assert_eq!(parse_remote("git://github.com/o/n.git"), r("o", "n"));
    assert_eq!(parse_remote("git+ssh://git@github.com/o/n"), r("o", "n"));
}

#[test]
fn host_and_scheme_are_case_insensitive() {
    assert_eq!(parse_remote("HTTPS://GitHub.COM/o/n"), r("o", "n"));
}

#[test]
fn surrounding_whitespace_is_trimmed_but_inner_whitespace_rejected() {
    assert_eq!(parse_remote("  https://github.com/o/n.git\n"), r("o", "n"));
    assert_eq!(parse_remote("https://github.com/o/n x"), None);
    assert_eq!(parse_remote("https://github.com/o /n"), None);
}

#[test]
fn credentials_are_never_kept() {
    let urls = [
        "https://user:ghp_SECRETTOKEN123@github.com/o/n.git",
        "https://ghp_SECRETTOKEN123@github.com/o/n.git",
        "https://x-access-token:ghs_SECRET@github.com/o/n",
        "https://user:pa@ss@github.com/o/n.git",
    ];
    for u in urls {
        let got = parse_remote(u).expect(u);
        assert_eq!(got, RepoRef { owner: "o".into(), name: "n".into() });
        assert!(!format!("{got:?}").contains("SECRET"));
        assert!(!got.full_name().contains("SECRET"));
    }
}

#[test]
fn other_hosts_are_rejected() {
    for u in [
        "https://gitlab.com/o/n.git",
        "git@gitlab.com:o/n.git",
        "https://github.example.com/o/n.git",
        "https://bitbucket.org/o/n",
    ] {
        assert_eq!(parse_remote(u), None, "{u}");
    }
}

#[test]
fn look_alike_and_trick_hosts_are_rejected() {
    for u in [
        "https://github.com.evil.com/o/n.git",
        "https://evilgithub.com/o/n.git",
        "https://github.com@evil.com/o/n.git",
        "https://user:pw@evil.com/o/n.git",
        "https://evil.com/github.com/o/n.git",
        "https://evil.com#@github.com/o/n.git",
        "https://evil.com?@github.com/o/n.git",
        "https://evil.com\\@github.com/o/n.git",
        "git@evil.com:github.com/o/n.git",
        "git@evil.com#@github.com:o/n.git",
        "https://github.com%2eevil.com/o/n",
    ] {
        assert_eq!(parse_remote(u), None, "{u}");
    }
}

#[test]
fn urls_that_are_not_repository_remotes_are_rejected() {
    for u in [
        "https://github.com/o/n/pull/1",
        "https://github.com/o/n/tree/main",
        "https://github.com/o",
        "https://github.com/",
        "https://github.com",
        "git@github.com:o",
        "git@github.com:",
    ] {
        assert_eq!(parse_remote(u), None, "{u}");
    }
}

#[test]
fn query_and_fragment_are_ignored_on_valid_paths() {
    assert_eq!(parse_remote("https://github.com/o/n.git?x=1"), r("o", "n"));
    assert_eq!(parse_remote("https://github.com/o/n#frag"), r("o", "n"));
}

#[test]
fn local_paths_and_odd_schemes_are_rejected() {
    for u in [
        "",
        "   ",
        "/home/user/repo",
        "../relative/repo",
        "file:///home/user/repo",
        "ftp://github.com/o/n",
        "C:\\\\repos\\\\x",
        "just-a-name",
    ] {
        assert_eq!(parse_remote(u), None, "{u:?}");
    }
}

#[test]
fn invalid_owner_and_name_characters_are_rejected() {
    for u in [
        "https://github.com/o;rm/n",
        "https://github.com/o/n;rm -rf",
        "https://github.com/o/n$(x)",
        "https://github.com/o/n`x`",
        "https://github.com/../n",
        "https://github.com/o/..",
        "https://github.com/o/.",
        "https://github.com/o_x/n",
        "https://github.com/\u{e9}/n",
        "https://github.com/o/n\u{e9}",
        "https://github.com//n",
        "https://github.com/o/.git",
    ] {
        assert_eq!(parse_remote(u), None, "{u}");
    }
}

#[test]
fn control_characters_and_overlong_input_are_rejected() {
    assert_eq!(parse_remote("https://github.com/o/n\0"), None);
    assert_eq!(parse_remote("https://github.com/o/n\x1b[2J"), None);
    let long = format!("https://github.com/o/{}", "a".repeat(5000));
    assert_eq!(parse_remote(&long), None);
    let long_name = format!("https://github.com/o/{}", "a".repeat(101));
    assert_eq!(parse_remote(&long_name), None);
    let ok_name = format!("https://github.com/o/{}", "a".repeat(100));
    assert!(parse_remote(&ok_name).is_some());
    let long_owner = format!("https://github.com/{}/n", "a".repeat(40));
    assert_eq!(parse_remote(&long_owner), None);
}

#[test]
fn full_name_joins_owner_and_name() {
    assert_eq!(parse_remote("git@github.com:o/n.git").unwrap().full_name(), "o/n");
}
