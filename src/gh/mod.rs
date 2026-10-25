//! GitHub access through the user's existing `gh` login.
//!
//! ARES never handles a token itself. It asks `gh` to make a small, fixed
//! set of read-only API requests (see [`runner::Query`]).

pub mod activity;
pub mod fetch;
pub mod runner;

#[cfg(test)]
mod activity_tests;
#[cfg(test)]
mod fetch_tests;
#[cfg(test)]
mod runner_tests;
