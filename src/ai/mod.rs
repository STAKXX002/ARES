//! The optional AI layer. ARES works fully without it.
//!
//! Principle: ARES observes deterministically and reasons selectively. A
//! model only ever sees [`crate::facts`], never raw repositories, and its
//! output is text for a person to read. Nothing it says is executed.

// Remove these allows once `ares ask` uses them.
pub mod answer;
pub mod ask;
pub mod http;
pub mod ollama;
pub mod payload;

#[cfg(test)]
mod tests;
