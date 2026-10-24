//! Human-readable wording for reports.
//!
//! A voice only chooses words around the facts. It never changes, hides, or
//! reorders them: the per-repository fact lines are printed by the CLI
//! identically for every voice. The default voice is `plain`.

use crate::repo::RepoReport;

/// Counts across all inspected repositories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub total: usize,
    /// Readable, nothing uncommitted, nothing unpushed.
    pub tidy: usize,
    /// With uncommitted changes.
    pub dirty: usize,
    /// With commits not yet pushed to their upstream.
    pub unpushed: usize,
    /// Repositories whose state could not be read.
    pub errors: usize,
}

impl Summary {
    pub fn from_reports(reports: &[RepoReport]) -> Summary {
        let mut s = Summary {
            total: reports.len(),
            tidy: 0,
            dirty: 0,
            unpushed: 0,
            errors: 0,
        };
        for r in reports {
            match &r.state {
                Err(_) => s.errors += 1,
                Ok(st) => {
                    if st.is_dirty() {
                        s.dirty += 1;
                    }
                    if st.has_unpushed() {
                        s.unpushed += 1;
                    }
                    if !st.is_dirty() && !st.has_unpushed() {
                        s.tidy += 1;
                    }
                }
            }
        }
        s
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Plain,
}

impl Voice {
    pub const ALL: [Voice; 1] = [Voice::Plain];

    pub fn name(self) -> &'static str {
        match self {
            Voice::Plain => "plain",
        }
    }

    pub fn from_name(s: &str) -> Option<Voice> {
        let s = s.trim().to_ascii_lowercase();
        Voice::ALL.iter().copied().find(|v| v.name() == s)
    }

    /// Comma-separated list of valid voice names, for error messages.
    pub fn names() -> String {
        Voice::ALL
            .iter()
            .map(|v| v.name())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn headline(self, s: &Summary, _seed: u64) -> String {
        match self {
            Voice::Plain => plain_headline(s),
        }
    }

    pub fn footer(self, _s: &Summary, _seed: u64) -> Option<String> {
        match self {
            Voice::Plain => None,
        }
    }

    pub fn repo_note(self, _r: &RepoReport, _seed: u64) -> Option<String> {
        match self {
            Voice::Plain => None,
        }
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

fn plain_headline(s: &Summary) -> String {
    if s.total == 0 {
        return "No repositories found.".to_string();
    }
    let mut out = format!(
        "{}: {} with uncommitted changes, {} with unpushed commits.",
        plural(s.total, "repository", "repositories"),
        s.dirty,
        s.unpushed
    );
    if s.errors > 0 {
        out.push_str(&format!(" {} could not be read.", s.errors));
    }
    out
}

/// Deterministic mixing function (splitmix64) for choosing wording variants.
fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Pick an index in `0..n` from a seed.
pub fn pick(seed: u64, n: usize) -> usize {
    (mix(seed) % n as u64) as usize
}

/// Build a seed from a key (such as a repo name) and a day number, so
/// wording is stable within a day and varies from day to day.
pub fn seed_from(key: &str, day: u64) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    mix(h ^ day)
}
