//! Reading and checking what a model says.
//!
//! A model's reply is untrusted text. ARES asks for a JSON list of
//! statements that each cite fact ids, then checks what can be checked
//! mechanically before showing anything:
//!
//! 1. every cited id must be a real fact (exact form, `F3` and not `F03`);
//! 2. every number in the text must appear in the cited facts;
//! 3. every repository named must be the subject of a cited fact;
//! 4. a statement that cites nothing may not contain a number or a
//!    repository name, so it can only be a general remark.
//!
//! This catches invented numbers, invented citations, and claims attached to
//! the wrong repository. It cannot judge whether a sentence is a *good*
//! reading of the facts, so shown statements are labelled with their sources
//! and the original facts remain available.

use crate::facts::Facts;
use serde_json::Value;
use std::collections::HashSet;

pub const MAX_STATEMENTS: usize = 20;
pub const MAX_TEXT: usize = 500;
pub const MAX_CITES: usize = 20;
const MAX_CITE_LEN: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub text: String,
    pub cites: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Cites real facts, and its numbers and repositories match them.
    Verified,
    /// Cites nothing; a general remark with no specific claims.
    Inferred,
    /// Failed a check. The reason is shown to the user instead of the text.
    Rejected(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub statement: Statement,
    pub verdict: Verdict,
}

fn tidy(s: &str, max: usize) -> String {
    let flat: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    flat.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Remove a Markdown code fence if the model wrapped its JSON in one.
fn unfence(s: &str) -> &str {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.split_once('\n').map_or(rest, |(_, body)| body);
        return rest.trim_end().trim_end_matches("```").trim();
    }
    t
}

/// Parse a model reply into statements.
///
/// Tolerates a code fence or a little text around the JSON. Anything that
/// does not have the expected shape is an error, and malformed individual
/// items are skipped.
pub fn parse(reply: &str) -> Result<Vec<Statement>, String> {
    let body = unfence(reply);
    let (start, end) = match (body.find('{'), body.rfind('}')) {
        (Some(s), Some(e)) if s < e => (s, e),
        _ => return Err("the reply contains no JSON object".to_string()),
    };
    let value: Value = serde_json::from_str(&body[start..=end])
        .map_err(|_| "the reply is not valid JSON".to_string())?;
    let list = value
        .get("statements")
        .and_then(Value::as_array)
        .ok_or_else(|| "the reply has no list of statements".to_string())?;

    let mut out = Vec::new();
    for item in list.iter().take(MAX_STATEMENTS) {
        let text = match item.get("text").and_then(Value::as_str) {
            Some(t) => tidy(t, MAX_TEXT),
            None => continue,
        };
        if text.is_empty() {
            continue;
        }
        let mut cites: Vec<String> = Vec::new();
        if let Some(arr) = item.get("cites").and_then(Value::as_array) {
            for c in arr.iter().filter_map(Value::as_str).take(MAX_CITES) {
                let c = tidy(c, MAX_CITE_LEN);
                if !c.is_empty() && !cites.contains(&c) {
                    cites.push(c);
                }
            }
        }
        out.push(Statement { text, cites });
    }
    Ok(out)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Byte ranges where `name` appears in `text` as a whole word (not inside a
/// longer word or identifier).
fn whole_word_spans(text: &str, name: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    if name.is_empty() {
        return spans;
    }
    let mut from = 0;
    while let Some(pos) = text[from..].find(name) {
        let start = from + pos;
        let end = start + name.len();
        let before_ok = text[..start].chars().next_back().map_or(true, |c| !is_word_char(c));
        let after_ok = text[end..].chars().next().map_or(true, |c| !is_word_char(c));
        if before_ok && after_ok {
            spans.push((start, end));
        }
        from = start + text[start..].chars().next().map_or(1, char::len_utf8);
    }
    spans
}

/// Which of `names` the text mentions, as indexes into `names`.
///
/// Longer names are matched first and claim their part of the text, so a
/// mention of `my-app.v2` is not also counted as a mention of `my-app`.
fn mentioned(text: &str, names: &[String]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..names.len())
        .filter(|&i| names[i].chars().count() >= 2)
        .collect();
    order.sort_by(|&a, &b| names[b].len().cmp(&names[a].len()).then(a.cmp(&b)));

    let mut claimed: Vec<(usize, usize)> = Vec::new();
    let mut found = Vec::new();
    for i in order {
        let free: Vec<(usize, usize)> = whole_word_spans(text, &names[i])
            .into_iter()
            .filter(|(s, e)| !claimed.iter().any(|(cs, ce)| s < ce && cs < e))
            .collect();
        if !free.is_empty() {
            claimed.extend(free);
            found.push(i);
        }
    }
    found.sort_unstable();
    found
}

/// Maximal runs of ASCII digits.
fn numbers_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn shown_names(facts: &Facts, alias: bool) -> Vec<String> {
    (0..facts.names.len())
        .map(|i| if alias { format!("R{}", i + 1) } else { facts.names[i].clone() })
        .collect()
}

/// Check each statement against the facts. `alias` says whether the payload
/// used `R1`, `R2`, ... in place of real names.
pub fn check(statements: &[Statement], facts: &Facts, alias: bool) -> Vec<Checked> {
    let names = shown_names(facts, alias);
    statements
        .iter()
        .map(|s| Checked {
            statement: s.clone(),
            verdict: verdict_for(s, facts, &names),
        })
        .collect()
}

fn verdict_for(s: &Statement, facts: &Facts, names: &[String]) -> Verdict {
    let numbers = numbers_in(&s.text);
    let named = mentioned(&s.text, names);

    if s.cites.is_empty() {
        return if !numbers.is_empty() || !named.is_empty() {
            Verdict::Rejected("made a specific claim without citing a fact".to_string())
        } else {
            Verdict::Inferred
        };
    }

    let mut cited = Vec::new();
    for label in &s.cites {
        match facts.find(label) {
            Some(f) => cited.push(f),
            None => return Verdict::Rejected(format!("cited {label}, which is not one of the facts")),
        }
    }

    let mut allowed: HashSet<String> = HashSet::new();
    let mut covered: HashSet<usize> = HashSet::new();
    for f in &cited {
        allowed.extend(numbers_in(&f.predicate));
        allowed.extend(numbers_in(&f.label()));
        if let Some(i) = f.subject {
            allowed.extend(numbers_in(&facts.names[i]));
            allowed.extend(numbers_in(&names[i]));
            covered.insert(i);
        }
    }
    // Wrong-repository claims are reported before number mismatches: they are
    // the more useful reason, and repository aliases contain digits.
    if let Some(&i) = named.iter().find(|i| !covered.contains(i)) {
        return Verdict::Rejected(format!("mentioned {}, which the cited facts do not cover", names[i]));
    }
    if let Some(n) = numbers.iter().find(|n| !allowed.contains(*n)) {
        return Verdict::Rejected(format!("mentioned the number {n}, which is not in the cited facts"));
    }
    Verdict::Verified
}

/// Replace `R1`, `R2`, ... with the real names, for display on this machine.
pub fn de_alias(text: &str, facts: &Facts) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let at_word_start = i == 0 || !is_word_char(chars[i - 1]);
        if chars[i] == 'R' && at_word_start {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            let ends_word = j >= chars.len() || !is_word_char(chars[j]);
            if j > i + 1 && ends_word {
                let digits: String = chars[i + 1..j].iter().collect();
                if let Some(name) = digits
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=facts.names.len()).contains(n))
                    .map(|n| &facts.names[n - 1])
                {
                    out.push_str(name);
                    i = j;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// What to show: the statements that passed, and why the others did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub shown: Vec<Checked>,
    /// One reason per rejected statement, in order.
    pub rejected: Vec<String>,
}

pub fn review(checked: Vec<Checked>) -> Review {
    let mut shown = Vec::new();
    let mut rejected = Vec::new();
    for c in checked {
        match &c.verdict {
            Verdict::Rejected(reason) => rejected.push(reason.clone()),
            _ => shown.push(c),
        }
    }
    Review { shown, rejected }
}
