//! Subprocess coverage for the Workshop checkout the orchestrator takes
//! from `RUNVIL_WORKSHOP`. Each run stops before any command.

use std::process::Command;

// A Windows installer needs no Gateway and no Qt IFW, so nothing else in
// the environment decides how far a run gets.
const TARGET: &str = "x86_64-pc-windows-msvc";

#[test]
fn an_unset_or_empty_runvil_workshop_fails_naming_the_variable() {
    for value in [None, Some("")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_build-installer"));
        command.args(["--target", TARGET]);
        match value {
            Some(value) => command.env("RUNVIL_WORKSHOP", value),
            None => command.env_remove("RUNVIL_WORKSHOP"),
        };

        let output = command.output().expect("run the orchestrator");

        assert!(!output.status.success(), "{value:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).trim_end(),
            "build-installer failed: RUNVIL_WORKSHOP is unset or empty; set it to the root of \
             a runvil-workshop checkout",
            "{value:?}"
        );
    }
}

#[test]
fn a_relative_runvil_workshop_resolves_against_the_working_directory() {
    let temp = tempfile::tempdir().expect("temporary test root");

    let output = Command::new(env!("CARGO_BIN_EXE_build-installer"))
        .args(["--target", TARGET])
        .env("RUNVIL_WORKSHOP", "no-such-checkout")
        .current_dir(temp.path())
        .output()
        .expect("run the orchestrator");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The working directory may be reported through another name than the
    // one the test gave it, so only the join is compared.
    let joined = format!(
        "{}no-such-checkout is not a directory; set RUNVIL_WORKSHOP",
        std::path::MAIN_SEPARATOR
    );
    assert!(
        stderr.starts_with("build-installer failed: the Workshop checkout "),
        "{stderr}"
    );
    assert!(stderr.contains(&joined), "{stderr}");
}
