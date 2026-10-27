//! The optional AI layer. ARES works fully without it.
//!
//! Principle: ARES observes deterministically and reasons selectively. A
//! model only ever sees [`crate::facts`], never raw repositories, and its
//! output is text for a person to read. Nothing it says is executed.

pub mod payload;
