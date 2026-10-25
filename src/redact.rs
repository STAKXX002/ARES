//! Remove credentials from text before it is shown or stored.
//!
//! Error output from `gh` and `git` can contain tokens (for example when
//! the user has debug logging enabled in their environment) or URLs with
//! embedded passwords. Anything that came from an external program passes
//! through [`redact`] before ARES prints it.

const REDACTED: &str = "[redacted]";

/// GitHub token prefixes, with the minimum length of the secret that
/// follows. Real tokens are far longer; the floor avoids hiding ordinary
/// words that merely start with a prefix.
const TOKEN_PREFIXES: &[(&str, usize)] = &[
    ("github_pat_", 20),
    ("ghp_", 20),
    ("gho_", 20),
    ("ghu_", 20),
    ("ghs_", 20),
    ("ghr_", 20),
];

fn is_token_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn redact_tokens(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    'outer: while i < bytes.len() {
        // Only match at a word start, so `xghp_...` inside another word is
        // still caught by scanning from its own start below.
        for (prefix, min) in TOKEN_PREFIXES {
            if bytes[i..].starts_with(prefix.as_bytes()) {
                let mut end = i + prefix.len();
                while end < bytes.len() && is_token_char(bytes[end]) {
                    end += 1;
                }
                if end - (i + prefix.len()) >= *min {
                    out.push_str(REDACTED);
                    i = end;
                    continue 'outer;
                }
            }
        }
        // Copy one full character (the input is valid UTF-8).
        let ch_len = s[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&s[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn redact_userinfo(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find("://") {
        let (head, tail) = rest.split_at(pos + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c == '/' || c.is_whitespace())
            .unwrap_or(tail.len());
        let authority = &tail[..end];
        match authority.rsplit_once('@') {
            Some((_, host)) => {
                out.push_str("***@");
                out.push_str(host);
            }
            None => out.push_str(authority),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn redact_auth_headers(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let lower = line.to_ascii_lowercase();
        match lower.find("authorization:") {
            Some(pos) => {
                out.push_str(&line[..pos + "authorization:".len()]);
                out.push(' ');
                out.push_str(REDACTED);
            }
            None => out.push_str(line),
        }
    }
    out
}

/// Replace tokens, URL passwords, and authorization headers.
pub fn redact(s: &str) -> String {
    redact_userinfo(&redact_tokens(&redact_auth_headers(s)))
}
