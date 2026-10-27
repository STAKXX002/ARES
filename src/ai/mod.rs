//! The optional AI layer. ARES works fully without it.
//!
//! Principle: ARES observes deterministically and reasons selectively. A
//! model only ever sees [`crate::facts`], never raw repositories, and its
//! output is text for a person to read. Nothing it says is executed.

// Remove this allow once the Ollama backend uses it.
#[allow(dead_code)]
pub mod http;
pub mod payload;

#[cfg(test)]
mod payload_tests;
#[cfg(test)]
mod http_tests;
