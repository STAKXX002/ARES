mod app;
mod cli;
mod config;
mod discovery;
mod git;
mod render;
mod repo;
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

use std::io;

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let env = app::Env::from_process();
    let stdout = io::stdout();
    let stderr = io::stderr();
    let code = app::run(&args, &env, &mut stdout.lock(), &mut stderr.lock());
    std::process::exit(code);
}
