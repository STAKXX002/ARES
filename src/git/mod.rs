//! Git integration. Read-only by design.

pub mod runner;
pub mod status;

#[cfg(test)]
mod runner_tests;
#[cfg(test)]
mod status_tests;
