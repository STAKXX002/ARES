// Remove this allow once the git module is wired into the CLI.
#[allow(dead_code)]
mod git;

#[cfg(test)]
mod testutil;

fn main() {
    println!("ares {}", env!("CARGO_PKG_VERSION"));
}
