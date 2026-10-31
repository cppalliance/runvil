//! Unit tests for where the sidecar comes from: the flag, the environment
//! variable, an already-staged file, and the build fallback.

use super::*;

use super::pipeline::{GatewaySource, gateway_source};

use std::ffi::OsStr;
use std::path::Path;

/// A repository root with a stand-in staging script, plus the desktop
/// crate's directory, so a test can place a sidecar where the pipeline
/// looks for one.
fn repository() -> TempDir {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let tools = temp.path().join("tools");
    std::fs::create_dir_all(&tools).expect("tools directory");
    std::fs::write(
        tools.join("stage-gateway-sidecar.mjs"),
        "// stand-in; the path test runs the real script\n",
    )
    .expect("staging script");
    std::fs::create_dir_all(temp.path().join("crates").join("workshop").join("desktop"))
        .expect("desktop crate");
    temp
}

fn environment_at(temp: &TempDir) -> BuildEnvironment {
    BuildEnvironment {
        workspace_root: temp.path().to_path_buf(),
        cargo: PathBuf::from("selected-cargo"),
        node: PathBuf::from("selected-node"),
        gateway_sidecar: None,
    }
}

fn request(
    profile: Profile,
    target: Option<&str>,
    gateway_binary: Option<PathBuf>,
) -> BuildRequest {
    BuildRequest {
        profile,
        target: target.map(str::to_owned),
        gateway_binary,
    }
}

/// Writes a file where the staged sidecar belongs, returning its path.
fn stage_fake_sidecar(environment: &BuildEnvironment, target: &str) -> PathBuf {
    let path = super::super::pipeline::sidecar_path(environment, target);
    std::fs::create_dir_all(path.parent().expect("sidecar parent")).expect("binaries directory");
    std::fs::write(&path, "staged gateway").expect("staged sidecar");
    path
}

#[test]
fn a_given_gateway_binary_replaces_the_gateway_build() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-unknown-linux-gnu";
    let given = PathBuf::from("/elsewhere/target/debug/runvil-gateway");
    let mut runner = FakeRunner::with_responses(vec![success(""), success(""), success("")]);

    build_workshop(
        &request(Profile::Debug, Some(triple), Some(given.clone())),
        environment,
        &mut runner,
    )
    .expect("Workshop build");

    assert_eq!(
        runner.commands,
        vec![
            owned_command(
                environment,
                "selected-node",
                &stage_arguments(environment, triple, given.to_str().expect("UTF-8 given")),
                OutputMode::Inherit,
            ),
            workshop_command(
                environment,
                triple,
                &["build", "-p", "workshop", "--target", triple]
            ),
            owned_command(
                environment,
                "selected-node",
                &remove_arguments(environment, triple),
                OutputMode::Inherit,
            ),
        ],
        "no Gateway build runs when a binary is given"
    );
}

#[test]
fn the_environment_variable_supplies_the_binary_when_no_flag_does() {
    let test_environment = environment();
    let mut environment = test_environment.environment;
    let triple = "aarch64-apple-darwin";
    let from_environment = PathBuf::from("/ci/staged/runvil-gateway");
    environment.gateway_sidecar = Some(from_environment.clone());
    let mut runner = FakeRunner::with_responses(vec![success(""), success(""), success("")]);

    build_workshop(
        &request(Profile::Release, Some(triple), None),
        &environment,
        &mut runner,
    )
    .expect("Workshop build");

    assert_eq!(
        runner.commands[0],
        owned_command(
            &environment,
            "selected-node",
            &stage_arguments(
                &environment,
                triple,
                from_environment.to_str().expect("UTF-8 environment path"),
            ),
            OutputMode::Inherit,
        ),
        "the environment variable stages like the flag"
    );
}

#[test]
fn the_flag_wins_over_the_environment_variable() {
    let test_environment = environment();
    let mut environment = test_environment.environment;
    environment.gateway_sidecar = Some(PathBuf::from("/ci/runvil-gateway"));
    let triple = "x86_64-unknown-linux-gnu";
    let flagged = PathBuf::from("/local/runvil-gateway");
    let mut runner = FakeRunner::with_responses(vec![success(""), success(""), success("")]);

    build_workshop(
        &request(Profile::Debug, Some(triple), Some(flagged.clone())),
        &environment,
        &mut runner,
    )
    .expect("Workshop build");

    assert_eq!(
        runner.commands[0].args[5..],
        strings(&[flagged.to_str().expect("UTF-8 flag")])
    );
}

#[test]
fn an_already_staged_sidecar_is_reused_without_building_or_removing_it() {
    let temp = repository();
    let environment = environment_at(&temp);
    let triple = "x86_64-pc-windows-msvc";
    let staged = stage_fake_sidecar(&environment, triple);
    let mut runner = FakeRunner::with_responses(vec![success("")]);

    build_workshop(
        &request(Profile::Debug, Some(triple), None),
        &environment,
        &mut runner,
    )
    .expect("Workshop build");

    assert_eq!(
        runner.commands,
        vec![command(
            &environment,
            "selected-cargo",
            &["build", "-p", "workshop", "--target", triple],
            OutputMode::Inherit,
        )],
        "a staged sidecar means one Workshop build and nothing else, and the \
         build script is left to use the staged file itself"
    );
    assert_eq!(
        std::fs::read_to_string(&staged).expect("the staged sidecar survives"),
        "staged gateway",
        "the run leaves the staged file for the next build to reuse"
    );
}

#[test]
fn a_staged_sidecar_is_reused_for_the_host_target_too() {
    let temp = repository();
    let environment = environment_at(&temp);
    let triple = "x86_64-pc-windows-msvc";
    stage_fake_sidecar(&environment, triple);
    let mut runner = FakeRunner::with_responses(vec![
        success("cargo 1.89.0\nhost: x86_64-pc-windows-msvc\n"),
        success(""),
    ]);

    build_workshop(
        &request(Profile::Debug, None, None),
        &environment,
        &mut runner,
    )
    .expect("Workshop build");

    assert_eq!(
        runner.commands.len(),
        2,
        "host discovery then the Workshop build, with no Gateway build"
    );
    assert_eq!(
        runner.commands[1].args,
        strings(&["build", "-p", "workshop"]),
        "the already-staged file is reused as is"
    );
}

#[test]
fn no_staged_sidecar_and_no_binary_stops_and_names_the_fetch_script() {
    let temp = repository();
    let environment = environment_at(&temp);
    let triple = "x86_64-unknown-linux-gnu";
    let mut runner = FakeRunner::with_responses(vec![success(""), success("")]);

    let error = build_workshop(
        &request(Profile::Debug, Some(triple), None),
        &environment,
        &mut runner,
    )
    .expect_err("no sidecar anywhere");

    assert!(
        error.primary.contains("fetch-gateway-sidecar.mjs"),
        "the message names the fetch script: {error}"
    );
    assert!(
        error.primary.contains(triple),
        "the message names the target it read: {error}"
    );
    assert_eq!(
        runner.commands.len(),
        0,
        "no command runs at all: the missing sidecar is refused before anything starts"
    );
}

#[test]
fn a_staged_stub_is_still_reused_but_the_run_says_it_cannot_run() {
    let temp = repository();
    let environment = environment_at(&temp);
    let triple = "x86_64-pc-windows-msvc";
    let staged = stage_fake_sidecar(&environment, triple);
    std::fs::write(
        format!("{}.pinned", staged.display()),
        "{\n  \"kind\": \"stub\",\n  \"tag\": null\n}\n",
    )
    .expect("stub marker");
    let mut runner = FakeRunner::with_responses(vec![success("")]);

    build_workshop(
        &request(Profile::Debug, Some(triple), None),
        &environment,
        &mut runner,
    )
    .expect("Workshop build");

    assert_eq!(
        gateway_source(
            &request(Profile::Debug, Some(triple), None),
            &environment,
            triple
        ),
        GatewaySource::Staged { stub: true },
        "a stub marker is read as a stub, so the run warns about it"
    );
}

#[test]
fn a_marker_kind_other_than_stub_is_treated_as_a_real_gateway() {
    let temp = repository();
    let environment = environment_at(&temp);
    let triple = "x86_64-unknown-linux-gnu";
    let staged = stage_fake_sidecar(&environment, triple);
    std::fs::write(
        format!("{}.pinned", staged.display()),
        "{\n  \"kind\": \"release\",\n  \"tag\": \"gateway-v0.4.0\"\n}\n",
    )
    .expect("release marker");
    assert_eq!(
        gateway_source(
            &request(Profile::Debug, Some(triple), None),
            &environment,
            triple
        ),
        GatewaySource::Staged { stub: false }
    );
}

#[test]
fn a_staged_sidecar_with_no_marker_counts_as_a_real_gateway() {
    let temp = repository();
    let environment = environment_at(&temp);
    let triple = "x86_64-unknown-linux-gnu";
    stage_fake_sidecar(&environment, triple);
    assert_eq!(
        gateway_source(
            &request(Profile::Debug, Some(triple), None),
            &environment,
            triple
        ),
        GatewaySource::Staged { stub: false },
        "a hand-staged binary is not assumed to be a stub"
    );
}

#[test]
fn the_sidecar_path_matches_the_staging_script() {
    let temp = repository();
    let environment = environment_at(&temp);
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("repository root")
        .join("tools")
        .join("stage-gateway-sidecar.mjs");
    for target in [
        "x86_64-pc-windows-msvc",
        "aarch64-apple-darwin",
        "x86_64-unknown-linux-gnu",
    ] {
        let output = std::process::Command::new("node")
            .arg(&script)
            .arg("path")
            .arg("--target")
            .arg(target)
            .output()
            .expect("the staging script runs");
        assert!(output.status.success(), "{output:?}");
        let from_script = String::from_utf8(output.stdout).expect("UTF-8 path");
        let ours = super::super::pipeline::sidecar_path(&environment, target);
        let expected_name = format!(
            "runvil-gateway-{target}{}",
            if target.contains("windows") {
                ".exe"
            } else {
                ""
            }
        );
        assert_eq!(
            Path::new(from_script.trim())
                .file_name()
                .expect("a file name"),
            Path::new(&expected_name),
            "the script and the pipeline agree on the staged file's name"
        );
        assert_eq!(
            ours.file_name().expect("a file name"),
            Path::new(&expected_name),
            "the pipeline names the staged file the way Tauri expects"
        );
        assert_eq!(
            ours.parent().expect("a parent").file_name(),
            Some(OsStr::new("binaries"))
        );
    }
}
