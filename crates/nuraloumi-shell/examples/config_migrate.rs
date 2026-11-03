//! Buildable CLI while the shared integration lane owns Cargo bin registration.
#[path = "../src/bin/config.rs"]
mod cli;

fn main() {
    cli::main();
}
