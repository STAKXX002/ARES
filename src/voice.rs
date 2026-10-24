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
    /// Neutral wording, no flavor. The default.
    Plain,
    /// Warm and encouraging, with a light touch of teasing.
    Cheerful,
    /// Dry and understated.
    Deadpan,
}

impl Voice {
    pub const ALL: [Voice; 3] = [Voice::Plain, Voice::Cheerful, Voice::Deadpan];

    pub fn name(self) -> &'static str {
        match self {
            Voice::Plain => "plain",
            Voice::Cheerful => "cheerful",
            Voice::Deadpan => "deadpan",
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

    /// One line summarizing the whole scan. Every variant states the counts.
    pub fn headline(self, s: &Summary, seed: u64) -> String {
        match self {
            Voice::Plain => plain_headline(s),
            Voice::Cheerful => with_errors(cheerful_headline(s, seed), s),
            Voice::Deadpan => with_errors(deadpan_headline(s, seed), s),
        }
    }

    /// Optional closing remark. Never used to carry facts.
    pub fn footer(self, s: &Summary, seed: u64) -> Option<String> {
        match self {
            Voice::Plain => None,
            Voice::Cheerful => cheerful_footer(s, seed),
            Voice::Deadpan => deadpan_footer(s, seed),
        }
    }

    /// Optional closing remark for the `changes` report, given how many
    /// differences were found. Never carries facts.
    // Remove this allow once `ares changes` uses it.
    #[allow(dead_code)]
    pub fn changes_remark(self, found: usize, seed: u64) -> Option<String> {
        match (self, found == 0) {
            (Voice::Plain, _) => None,
            (Voice::Cheerful, true) => Some(match pick(seed, 2) {
                0 => "Nothing moved since last time. Enjoy the calm!",
                _ => "All quiet. Sometimes that is the best news.",
            }
            .to_string()),
            (Voice::Cheerful, false) => Some(match pick(seed, 2) {
                0 => "Things are moving, and that is a good sign!",
                _ => "Progress leaves footprints. Keep it up!",
            }
            .to_string()),
            (Voice::Deadpan, true) => Some(match pick(seed, 2) {
                0 => "Nothing happened. Riveting.",
                _ => "No change. I will try to contain my excitement.",
            }
            .to_string()),
            (Voice::Deadpan, false) => Some(match pick(seed, 2) {
                0 => "Things changed. Noted.",
                _ => "Evidence of activity. Encouraging, if unconfirmed.",
            }
            .to_string()),
        }
    }

    /// Optional remark under a single repository. Only given for notable
    /// repos, and never for conflicts or read errors, which stay plain.
    pub fn repo_note(self, r: &RepoReport, seed: u64) -> Option<String> {
        let st = r.state.as_ref().ok()?;
        if st.conflicted > 0 {
            return None;
        }
        let many_changes = st.changed_files >= 10;
        let many_unpushed = st.ahead.unwrap_or(0) >= 3;
        let ahead = st.ahead.unwrap_or(0);
        let n = st.changed_files;
        match self {
            Voice::Plain => None,
            Voice::Cheerful if many_changes => Some(match pick(seed, 2) {
                0 => format!("That is {n} changed files. A commit or two would make this feel lighter!"),
                _ => format!("{n} changed files and counting. Maybe split them into a few small commits?"),
            }),
            Voice::Cheerful if many_unpushed => {
                Some(format!("{ahead} commits ready to be pushed whenever you are."))
            }
            Voice::Deadpan if many_changes => Some(match pick(seed, 2) {
                0 => format!("{n} changed files. Bold."),
                _ => format!("{n} uncommitted files. Impressive, in its way."),
            }),
            Voice::Deadpan if many_unpushed => {
                Some(format!("{ahead} unpushed commits, just sitting there."))
            }
            _ => None,
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

/// Errors are warnings, so they are always stated plainly in every voice.
fn with_errors(mut line: String, s: &Summary) -> String {
    if s.errors > 0 {
        line.push_str(&format!(" {} could not be read.", s.errors));
    }
    line
}

fn all_good(s: &Summary) -> bool {
    s.dirty == 0 && s.unpushed == 0 && s.errors == 0
}

fn cheerful_headline(s: &Summary, seed: u64) -> String {
    if s.total == 0 {
        return "No repositories found yet. Double-check the scan roots and I will happily look again!"
            .to_string();
    }
    let (t, ti, d, u) = (s.total, s.tidy, s.dirty, s.unpushed);
    if all_good(s) {
        return match pick(seed, 3) {
            0 => format!("All {t} repos are tidy and pushed. Lovely work!"),
            1 => format!("Everything is committed and pushed across {t} repos. Gold star!"),
            _ => format!("{t} repos, zero loose ends. You can relax a little."),
        };
    }
    match pick(seed, 3) {
        0 => format!("Found {t} repos: {ti} tidy, {d} with loose changes, {u} with unpushed commits. Let's tidy up!"),
        1 => format!("{t} repos checked. {ti} are in great shape, {d} have uncommitted changes, {u} are waiting to be pushed. You've got this!"),
        _ => format!("Here is the lay of the land: {t} repos, {d} with changes, {u} with unpushed commits, {ti} tidy. Not bad at all!"),
    }
}

fn cheerful_footer(s: &Summary, seed: u64) -> Option<String> {
    if s.dirty > 0 {
        Some(match pick(seed, 3) {
            0 => "Maybe commit a few of those before you call it a day? Future you will say thanks.",
            1 => "No rush, but a commit or two would make tomorrow's start lovely.",
            _ => "Take a breath, then pick one repo and tidy it. One at a time works.",
        }
        .to_string())
    } else if s.unpushed > 0 {
        Some("Nothing uncommitted. Just some commits waiting to be pushed, an easy win!".to_string())
    } else {
        None
    }
}

fn deadpan_headline(s: &Summary, seed: u64) -> String {
    if s.total == 0 {
        return "No repositories found. Either restraint or the wrong folder.".to_string();
    }
    let (t, ti, d, u) = (s.total, s.tidy, s.dirty, s.unpushed);
    if all_good(s) {
        return match pick(seed, 3) {
            0 => format!("{t} repos. No issues. Suspicious, but fine."),
            1 => format!("{t} repos clean and pushed. Nothing to report. I checked twice."),
            _ => format!("All {t} repos accounted for. Nothing is on fire."),
        };
    }
    match pick(seed, 3) {
        0 => format!("{t} repos scanned. {d} dirty, {u} unpushed, {ti} tidy. They will not commit themselves."),
        1 => format!("{t} repos. {d} have uncommitted changes and {u} have unpushed commits. {ti} are fine."),
        _ => format!("Status: {ti} tidy, {d} dirty, {u} unpushed, out of {t}. Draw your own conclusions."),
    }
}

fn deadpan_footer(s: &Summary, seed: u64) -> Option<String> {
    if s.dirty > 0 {
        Some(match pick(seed, 2) {
            0 => "Consider committing. Or don't. I will keep counting.",
            _ => "The changes will still be here tomorrow. They always are.",
        }
        .to_string())
    } else if s.unpushed > 0 {
        Some("Everything is committed. Pushing is a separate decision.".to_string())
    } else {
        None
    }
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
