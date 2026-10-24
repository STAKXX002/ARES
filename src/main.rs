// Remove these allows once the modules are wired into the CLI.
#[allow(dead_code)]
mod cli;
#[allow(dead_code)]
mod config;
#[allow(dead_code)]
mod discovery;
#[allow(dead_code)]
mod git;
#[allow(dead_code)]
mod render;
#[allow(dead_code)]
mod repo;
#[allow(dead_code)]
mod voice;

#[cfg(test)]
mod cli_tests;
#[cfg(test)]
mod discovery_tests;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod repo_tests;
#[cfg(test)]
mod testutil;
#[cfg(test)]
mod voice_tests;

fn main() {
    println!("ares {}", env!("CARGO_PKG_VERSION"));
}
