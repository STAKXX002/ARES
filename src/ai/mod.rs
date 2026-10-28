//! The optional AI layer. ARES works fully without it.
//!
//! Principle: ARES observes deterministically and reasons selectively. A
//! model only ever sees [`crate::facts`], never raw repositories, and its
//! output is text for a person to read. Nothing it says is executed.

// Remove these allows once `ares ask` uses them.
#[allow(dead_code)]
pub mod answer;
#[allow(dead_code)]
pub mod http;
#[allow(dead_code)]
pub mod ollama;
pub mod payload;

#[cfg(test)]
mod payload_tests;
#[cfg(test)]
mod http_tests;
#[cfg(test)]
mod ollama_tests;
