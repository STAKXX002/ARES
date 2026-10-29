//! Putting the AI pieces together: send the payload to the local model, check
//! what comes back, and decide what a person gets to read.
//!
//! Three outcomes are possible. The model's statements that pass the checks
//! are shown with their citations. If none pass, or the reply is garbage, the
//! measured facts are shown instead, so `ares ask` is never worse than
//! `ares status`. Problems reaching Ollama at all (not running, model
//! missing, timeout) are errors, reported by the caller.

use super::answer::{self, Checked, Verdict};
use super::ollama::{BackendError, Ollama};
use crate::facts::Facts;

/// Shown after every answer.
pub const NOTE: &str = "statements are checked against measured facts, this is a summary not advice";

/// How many rejection reasons and fallback facts are listed before cutting off.
const MAX_REASONS: usize = 5;
const MAX_FALLBACK_FACTS: usize = 60;

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// At least one statement passed. `shown` keeps the model's order and
    /// holds verified statements and uncited general remarks.
    Answered {
        shown: Vec<Checked>,
        rejected: Vec<String>,
    },
    /// Nothing usable came back; show the facts instead.
    Unusable {
        reason: String,
        rejected: Vec<String>,
    },
}

/// Send `prompt` to the model and check the reply against `facts`.
///
/// `alias` must match what the payload was built with. Errors are only for
/// not being able to get an answer at all; a bad answer is an
/// [`Outcome::Unusable`].
pub fn ask(prompt: &str, facts: &Facts, alias: bool, ollama: &Ollama) -> Result<Outcome, BackendError> {
    let reply = match ollama.complete(prompt) {
        Ok(text) => text,
        Err(e @ (BackendError::Truncated | BackendError::BadResponse(_))) => {
            return Ok(Outcome::Unusable {
                reason: e.to_string(),
                rejected: Vec::new(),
            })
        }
        Err(e) => return Err(e),
    };
    Ok(judge(&reply, facts, alias))
}

/// Check a raw model reply. Split out from [`ask`] so it needs no server.
pub fn judge(reply: &str, facts: &Facts, alias: bool) -> Outcome {
    let statements = match answer::parse(reply) {
        Ok(s) => s,
        Err(reason) => {
            return Outcome::Unusable {
                reason,
                rejected: Vec::new(),
            }
        }
    };
    let review = answer::review(answer::check(&statements, facts, alias));
    if review.shown.iter().any(|c| c.verdict == Verdict::Verified) {
        return Outcome::Answered {
            shown: review.shown,
            rejected: review.rejected,
        };
    }
    let reason = if statements.is_empty() {
        "the model gave no statements"
    } else if review.shown.is_empty() {
        "none of the model's statements passed the checks"
    } else {
        "the model gave no statement backed by a measured fact"
    };
    Outcome::Unusable {
        reason: reason.to_string(),
        rejected: review.rejected,
    }
}

fn show(text: &str, facts: &Facts, alias: bool) -> String {
    if alias {
        answer::de_alias(text, facts)
    } else {
        text.to_string()
    }
}

/// The answer a person reads. Real names are restored when `alias` was on.
pub fn render_answer(shown: &[Checked], rejected: &[String], facts: &Facts, alias: bool, model: &str) -> String {
    let mut out = String::new();
    for c in shown {
        let text = show(&c.statement.text, facts, alias);
        match c.verdict {
            Verdict::Verified => out.push_str(&format!("- {text} [{}]\n", c.statement.cites.join(", "))),
            _ => out.push_str(&format!("- {text} (inferred, no fact cited)\n")),
        }
    }
    if !rejected.is_empty() {
        let n = rejected.len();
        out.push_str(&format!(
            "\n{n} {} rejected by the checks:\n",
            if n == 1 { "statement was" } else { "statements were" }
        ));
        for reason in rejected.iter().take(MAX_REASONS) {
            out.push_str(&format!("  - {}\n", show(reason, facts, alias)));
        }
        if n > MAX_REASONS {
            out.push_str(&format!("  - and {} more\n", n - MAX_REASONS));
        }
    }
    out.push_str(&format!("\nAnswered by {model} through Ollama on this machine; {NOTE}.\n"));
    out
}

/// What is shown when the model gave nothing usable: the measured facts
/// themselves, with their ids, using real names (this never leaves the machine).
pub fn render_facts(facts: &Facts) -> String {
    let mut out = String::from("Measured facts:\n");
    for f in facts.facts.iter().take(MAX_FALLBACK_FACTS) {
        let sentence = match facts.subject_name(f, false) {
            Some(name) => format!("{name} {}", f.predicate),
            None => f.predicate.clone(),
        };
        out.push_str(&format!("  {:<5}{sentence}\n", f.label()));
    }
    let more = facts.facts.len().saturating_sub(MAX_FALLBACK_FACTS) + facts.omitted;
    if more > 0 {
        out.push_str(&format!("  ... and {more} more; `ares status` shows the full picture\n"));
    }
    out
}
