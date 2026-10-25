//! Git integration. Read-only by design.

/// True for a full-length lowercase hexadecimal object id (SHA-1 or SHA-256).
pub fn is_object_id(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub mod runner;
pub mod status;

#[cfg(test)]
mod runner_tests;
#[cfg(test)]
mod status_tests;
