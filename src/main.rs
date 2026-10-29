mod ai;
mod app;
mod changes;
mod cli;
mod config;
mod discovery;
mod facts;
mod gh;
mod git;
mod github;
mod proc;
mod redact;
mod render;
mod repo;
mod stale;
mod store;
mod timefmt;
mod voice;

#[cfg(test)]
mod tests;


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
