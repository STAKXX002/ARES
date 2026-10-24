//! Parser for the output of `git status --porcelain=v2 --branch -z`.
//!
//! This module is pure: it takes text and returns data. It never runs git
//! and never touches the filesystem, which keeps it trivial to test.
//!
//! The `-z` form is required. It separates records with NUL and does not
//! quote paths, so paths containing spaces, quotes, or newlines parse
//! correctly. Paths are decoded lossily as UTF-8.

use std::error::Error;
use std::fmt;

/// State of a file in the index (X) or the working tree (Y).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    Unmodified,
    Modified,
    TypeChanged,
    Added,
    Deleted,
    Renamed,
    Copied,
    /// Used in unmerged (conflicted) entries, for example `UU`.
    Unmerged,
}

impl FileState {
    fn from_char(c: char) -> Option<Self> {
        match c {
            '.' => Some(Self::Unmodified),
            'M' => Some(Self::Modified),
            'T' => Some(Self::TypeChanged),
            'A' => Some(Self::Added),
            'D' => Some(Self::Deleted),
            'R' => Some(Self::Renamed),
            'C' => Some(Self::Copied),
            'U' => Some(Self::Unmerged),
            _ => None,
        }
    }
}

/// Which record type an entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Record type `1`: an ordinary changed entry.
    Ordinary,
    /// Record type `2`: a renamed or copied entry.
    RenamedOrCopied,
    /// Record type `u`: an unmerged (conflicted) entry.
    Unmerged,
}

/// One tracked path with changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    pub index: FileState,
    pub worktree: FileState,
    pub path: String,
    /// Original path for renames and copies.
    pub orig_path: Option<String>,
}

/// Branch information from the `# branch.*` header lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchInfo {
    /// Current commit id, or `None` on an unborn branch.
    pub oid: Option<String>,
    /// Branch name, or `None` when HEAD is detached.
    pub head: Option<String>,
    pub detached: bool,
    pub upstream: Option<String>,
    /// `(ahead, behind)` relative to the upstream, when an upstream exists.
    pub ahead_behind: Option<(u32, u32)>,
}

/// Parsed working tree status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub branch: BranchInfo,
    pub entries: Vec<Entry>,
    pub untracked: Vec<String>,
    pub ignored: Vec<String>,
}

impl Status {
    /// Entries with a change recorded in the index.
    pub fn staged(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.kind != EntryKind::Unmerged && e.index != FileState::Unmodified)
            .count()
    }

    /// Entries with a change in the working tree that is not yet staged.
    pub fn unstaged(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.kind != EntryKind::Unmerged && e.worktree != FileState::Unmodified)
            .count()
    }

    pub fn conflicted(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.kind == EntryKind::Unmerged)
            .count()
    }

    pub fn untracked_count(&self) -> usize {
        self.untracked.len()
    }

    /// True when there is anything uncommitted. Ignored files do not count.
    pub fn is_dirty(&self) -> bool {
        !self.entries.is_empty() || !self.untracked.is_empty()
    }
}

/// Error describing which record failed to parse and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Zero-based index of the NUL-separated record.
    pub record: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "record {}: {}", self.record, self.message)
    }
}

impl Error for ParseError {}

fn err(record: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        record,
        message: message.into(),
    }
}

/// Parse `git status --porcelain=v2 --branch -z` output.
pub fn parse(input: &str) -> Result<Status, ParseError> {
    let mut status = Status::default();
    let mut records = input.split('\0').enumerate();

    while let Some((idx, rec)) = records.next() {
        if rec.is_empty() {
            continue;
        }

        if let Some(header) = rec.strip_prefix("# ") {
            parse_header(header, &mut status.branch).map_err(|m| err(idx, m))?;
        } else if let Some(rest) = rec.strip_prefix("1 ") {
            // 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
            let f: Vec<&str> = rest.splitn(8, ' ').collect();
            if f.len() != 8 {
                return Err(err(idx, "ordinary entry has too few fields"));
            }
            let (index, worktree) = parse_xy(f[0]).map_err(|m| err(idx, m))?;
            status.entries.push(Entry {
                kind: EntryKind::Ordinary,
                index,
                worktree,
                path: f[7].to_string(),
                orig_path: None,
            });
        } else if let Some(rest) = rec.strip_prefix("2 ") {
            // 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <Xscore> <path>
            // With -z the original path is the next NUL-separated record.
            let f: Vec<&str> = rest.splitn(9, ' ').collect();
            if f.len() != 9 {
                return Err(err(idx, "rename entry has too few fields"));
            }
            let (index, worktree) = parse_xy(f[0]).map_err(|m| err(idx, m))?;
            let orig = match records.next() {
                Some((_, o)) if !o.is_empty() => o.to_string(),
                _ => return Err(err(idx, "rename entry is missing its original path")),
            };
            status.entries.push(Entry {
                kind: EntryKind::RenamedOrCopied,
                index,
                worktree,
                path: f[8].to_string(),
                orig_path: Some(orig),
            });
        } else if let Some(rest) = rec.strip_prefix("u ") {
            // u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
            let f: Vec<&str> = rest.splitn(10, ' ').collect();
            if f.len() != 10 {
                return Err(err(idx, "unmerged entry has too few fields"));
            }
            let (index, worktree) = parse_xy(f[0]).map_err(|m| err(idx, m))?;
            status.entries.push(Entry {
                kind: EntryKind::Unmerged,
                index,
                worktree,
                path: f[9].to_string(),
                orig_path: None,
            });
        } else if let Some(path) = rec.strip_prefix("? ") {
            status.untracked.push(path.to_string());
        } else if let Some(path) = rec.strip_prefix("! ") {
            status.ignored.push(path.to_string());
        } else {
            return Err(err(idx, format!("unrecognized record: {:?}", rec)));
        }
    }

    Ok(status)
}

fn parse_xy(xy: &str) -> Result<(FileState, FileState), String> {
    let mut chars = xy.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(x), Some(y), None) => {
            let x = FileState::from_char(x).ok_or_else(|| format!("bad index status {:?}", x))?;
            let y = FileState::from_char(y).ok_or_else(|| format!("bad worktree status {:?}", y))?;
            Ok((x, y))
        }
        _ => Err(format!("malformed XY status {:?}", xy)),
    }
}

fn parse_header(header: &str, branch: &mut BranchInfo) -> Result<(), String> {
    if let Some(oid) = header.strip_prefix("branch.oid ") {
        branch.oid = if oid == "(initial)" {
            None
        } else {
            Some(oid.to_string())
        };
    } else if let Some(head) = header.strip_prefix("branch.head ") {
        if head == "(detached)" {
            branch.detached = true;
            branch.head = None;
        } else {
            branch.detached = false;
            branch.head = Some(head.to_string());
        }
    } else if let Some(up) = header.strip_prefix("branch.upstream ") {
        branch.upstream = Some(up.to_string());
    } else if let Some(ab) = header.strip_prefix("branch.ab ") {
        let mut parts = ab.split(' ');
        let ahead = parts
            .next()
            .and_then(|s| s.strip_prefix('+'))
            .and_then(|s| s.parse::<u32>().ok());
        let behind = parts
            .next()
            .and_then(|s| s.strip_prefix('-'))
            .and_then(|s| s.parse::<u32>().ok());
        match (ahead, behind, parts.next()) {
            (Some(a), Some(b), None) => branch.ahead_behind = Some((a, b)),
            _ => return Err(format!("malformed branch.ab header {:?}", ab)),
        }
    }
    // Unknown headers (for example `stash`) are ignored for forward compatibility.
    Ok(())
}
