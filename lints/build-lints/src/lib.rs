//! Dylint lints for the runvil workspace: rules that clippy has no lint
//! for, checked with `cargo dylint --all` from the workspace root.
//!
//! Each lint lives in its own module and is registered in
//! [`register_lints`]. A library holding more than one lint registers them
//! by hand rather than through `dylint_linting::declare_late_lint!`, which
//! declares a whole library around a single lint.
//!
//! ## Invariants
//!
//! - The library builds only on the nightly in `rust-toolchain.toml`, the
//!   one the `clippy_utils` rev in `Cargo.toml` targets; both move together.
//! - Every lint has a UI test under `ui/`, run by `cargo test` here.

#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;

mod io_try_without_context;

dylint_linting::dylint_library!();

/// Registers every lint in this library with the compiler session.
#[unsafe(no_mangle)]
pub fn register_lints(sess: &rustc_session::Session, lint_store: &mut rustc_lint::LintStore) {
    dylint_linting::init_config(sess);
    lint_store.register_lints(&[io_try_without_context::IO_TRY_WITHOUT_CONTEXT]);
    lint_store.register_late_lint_pass(Box::new(|_| {
        Box::new(io_try_without_context::IoTryWithoutContext)
    }));
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_CRATE_NAME"), "ui");
}
