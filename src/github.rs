//! GitHub helpers. Pure parsing, no I/O.
//!
//! Remote URLs frequently contain credentials (`https://user:token@...`).
//! Parsing here keeps only the repository's owner and name and discards
//! everything else, so a token can never be stored, printed, or sent on.

/// A repository on github.com.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
}

impl RepoRef {
    /// The `owner/name` form used by GitHub tooling.
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

const MAX_URL_LEN: usize = 2048;

/// Characters that must never appear in the authority (host) part. They would
/// let a crafted URL make a naive parser read a different host than a real
/// URL parser does, for example `https://evil.com#@github.com/o/n`.
const FORBIDDEN_IN_AUTHORITY: &[char] = &['#', '?', '\\', '[', ']'];

/// Parse a Git remote URL pointing at github.com.
///
/// Accepted forms: `https://`, `http://`, `ssh://`, `git://` URLs (with
/// optional user info and port) and scp-like `git@github.com:owner/name`.
/// A trailing `.git` and trailing slash are ignored. Anything else, such as
/// another host, a URL with extra path segments, or a local path, gives
/// `None`.
pub fn parse_remote(url: &str) -> Option<RepoRef> {
    let url = url.trim();
    if url.is_empty()
        || url.len() > MAX_URL_LEN
        || url.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }
    let (host, path) = split_host_and_path(url)?;
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }
    repo_from_path(path)
}

fn split_host_and_path(url: &str) -> Option<(&str, &str)> {
    if let Some((scheme, rest)) = url.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        if !matches!(
            scheme.as_str(),
            "https" | "http" | "ssh" | "git" | "git+ssh" | "ssh+git"
        ) {
            return None;
        }
        let (authority, path) = rest.split_once('/')?;
        if authority.contains(FORBIDDEN_IN_AUTHORITY) {
            return None;
        }
        // User info (possibly holding a token) ends at the LAST '@'.
        let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = host_port.split(':').next()?;
        valid_host(host).then_some((host, path))
    } else {
        // scp-like syntax: [user@]host:path
        let (left, path) = url.split_once(':')?;
        if left.contains('/') || left.contains(FORBIDDEN_IN_AUTHORITY) {
            return None;
        }
        let host = left.rsplit_once('@').map_or(left, |(_, h)| h);
        valid_host(host).then_some((host, path))
    }
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

fn repo_from_path(path: &str) -> Option<RepoRef> {
    // Drop any query or fragment, then one leading and any trailing slashes.
    let path = path.split(['?', '#']).next()?;
    let path = path.trim_end_matches('/');
    let path = path.strip_prefix('/').unwrap_or(path);
    let path = path.strip_suffix(".git").unwrap_or(path);

    let mut parts = path.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    if !valid_owner(owner) || !valid_name(name) {
        return None;
    }
    Some(RepoRef {
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

/// GitHub account names: letters, digits, and hyphens, and never starting or
/// ending with a hyphen.
pub(crate) fn valid_owner(s: &str) -> bool {
    (1..=39).contains(&s.len())
        && !s.starts_with('-')
        && !s.ends_with('-')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub(crate) fn valid_name(s: &str) -> bool {
    (1..=100).contains(&s.len())
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}
