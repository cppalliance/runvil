//! `build-xtask` - workspace automation for the Runvil repository.
//!
//! ## Invariants
//!
//! - Tier: tooling. The tidy-style
//!   architecture checks run as tests (`cargo test -p build-xtask`);
//!   `cargo xtask tidy` prints the same report on demand.

mod debug_spelling;
mod harness_pin;
mod manifest;
mod new_crate;
mod no_doctests;
mod product;
mod site;
mod tidy;
mod type_size;
mod unsafe_allowlist;

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2) else {
        eprintln!("error: cannot locate workspace root from CARGO_MANIFEST_DIR");
        return ExitCode::FAILURE;
    };
    match args.get(1).map(String::as_str) {
        Some("new-crate") => match args.get(2) {
            Some(name) => match new_crate::scaffold(root, name) {
                Ok(dir) => {
                    println!("scaffolded {}", dir.display());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("error: {error:#}");
                    ExitCode::FAILURE
                }
            },
            None => usage(),
        },
        Some("site") => site::run(
            root,
            std::env::var_os(site::DOCS_VAR).as_deref(),
            std::env::var_os(site::HARNESS_VAR).as_deref(),
            &args[2..],
        ),
        Some("tidy") => {
            let violations = tidy::all_violations(root);
            if violations.is_empty() {
                println!("tidy: no violations");
                ExitCode::SUCCESS
            } else {
                for violation in &violations {
                    eprintln!("tidy: {violation}");
                }
                ExitCode::FAILURE
            }
        }
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: cargo xtask new-crate <workshop-name> | cargo xtask tidy | \
         cargo xtask site [--books-only]"
    );
    ExitCode::from(2)
}
