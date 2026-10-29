use crate::redact::redact;

#[test]
fn github_tokens_of_every_kind_are_removed() {
    for t in [
        "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
        "gho_abcdefghijklmnopqrstuvwxyz0123456789",
        "ghu_abcdefghijklmnopqrstuvwxyz0123456789",
        "ghs_abcdefghijklmnopqrstuvwxyz0123456789",
        "ghr_abcdefghijklmnopqrstuvwxyz0123456789",
        "github_pat_11ABCDEFG0abcdefghijkl_mnopqrstuvwxyz0123456789",
    ] {
        let text = format!("failed with token {t} while calling");
        let out = redact(&text);
        assert!(!out.contains(t), "{out}");
        assert!(out.contains("[redacted]"));
        assert!(out.starts_with("failed with token "));
        assert!(out.ends_with(" while calling"));
    }
}

#[test]
fn several_tokens_in_one_text() {
    let out = redact("a ghp_aaaaaaaaaaaaaaaaaaaaaaaa b gho_bbbbbbbbbbbbbbbbbbbbbbbb c");
    assert_eq!(out, "a [redacted] b [redacted] c");
}

#[test]
fn short_lookalikes_and_ordinary_words_are_untouched() {
    for t in ["ghp_", "ghp_short", "the ghost_story", "ghp", "github_pat_x", "light"] {
        assert_eq!(redact(t), t);
    }
}

#[test]
fn passwords_in_urls_are_removed() {
    assert_eq!(
        redact("fatal: unable to access 'https://user:hunter2@github.com/o/n.git/': error"),
        "fatal: unable to access 'https://***@github.com/o/n.git/': error"
    );
    assert_eq!(redact("https://tokenonly@github.com/o/n"), "https://***@github.com/o/n");
    assert_eq!(redact("https://user:p@ss@host.com/x"), "https://***@host.com/x");
}

#[test]
fn urls_without_user_info_are_unchanged() {
    let t = "see https://github.com/o/n and ssh://git@example.com is fine";
    // `git@` is user info too, so it is hidden even though it is not secret.
    assert_eq!(redact("see https://github.com/o/n"), "see https://github.com/o/n");
    assert!(redact(t).contains("https://github.com/o/n"));
}

#[test]
fn authorization_headers_are_removed_whatever_the_case() {
    let out = redact("> GET /user\n> Authorization: token abc123SECRET\n> Accept: x");
    assert!(!out.contains("abc123SECRET"));
    assert!(out.contains("Authorization: [redacted]"));
    assert!(out.contains("> Accept: x"));
    assert!(!redact("authorization: Bearer topsecret").contains("topsecret"));
    assert!(!redact("AUTHORIZATION:bearer topsecret").contains("topsecret"));
}

#[test]
fn text_without_secrets_and_unicode_pass_through() {
    for t in ["", "plain error", "gh: Not Found (HTTP 404)", "caf\u{e9} \u{4e2d}\u{6587} ok"] {
        assert_eq!(redact(t), t);
    }
}

#[test]
fn a_token_followed_by_punctuation_is_still_removed() {
    let out = redact("token=ghp_abcdefghijklmnopqrstuvwxyz0123, retry");
    assert_eq!(out, "token=[redacted], retry");
}
