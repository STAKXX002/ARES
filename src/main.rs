// Remove these allows once the modules are wired into the CLI.
#[allow(dead_code)]
mod config;
#[allow(dead_code)]
mod discovery;
#[allow(dead_code)]
mod git;
#[allow(dead_code)]
mod repo;

#[cfg(test)]
mod discovery_tests;
#[cfg(test)]
mod repo_tests;
#[cfg(test)]
mod testutil;

fn main() {
    println!("ares {}", env!("CARGO_PKG_VERSION"));
}
