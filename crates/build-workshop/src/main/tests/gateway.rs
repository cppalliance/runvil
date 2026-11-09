//! Unit tests for where both modes find the Gateway: the given binary, the
//! staged slot and its stub marker, the pin, and the copy beside Workshop.

use super::installer_tests::{node_found, request};
use super::*;

fn build_request(target: Option<&str>, gateway_binary: Option<PathBuf>) -> BuildRequest {
    BuildRequest {
        profile: Profile::Debug,
        target: target.map(str::to_owned),
        gateway_binary,
    }
}

#[test]
fn a_missing_gateway_names_the_fetch_script_and_runs_no_command() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-unknown-linux-gnu";
    let fetch = format!("node tools/fetch-gateway-sidecar.mjs --target {target}");

    let mut runner = FakeRunner::default();
    let error = build_workshop(&build_request(Some(target), None), environment, &mut runner)
        .expect_err("no Gateway for the build");
    assert!(error.primary.contains(&fetch), "{error}");
    assert!(runner.commands.is_empty());

    let error = build_installer(&request(target, false), environment, &mut runner)
        .expect_err("no Gateway for the installer");
    assert!(error.primary.contains(&fetch), "{error}");
    assert!(runner.commands.is_empty());
}

#[test]
fn a_staged_stub_refuses_the_installer_but_builds_workshop() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-unknown-linux-gnu";
    let stub = test_environment.stage_stub(target);

    let mut runner = FakeRunner::default();
    let error = build_installer(&request(target, false), environment, &mut runner)
        .expect_err("a stub installer");
    assert!(error.primary.contains("compile-only stub"), "{error}");
    assert!(
        error.primary.contains(&format!(
            "node tools/fetch-gateway-sidecar.mjs --target {target}"
        )),
        "{error}"
    );
    assert!(runner.commands.is_empty());

    let profile = environment.target_root.join(target).join("debug");
    let mut runner =
        FakeRunner::with_responses(vec![workshop_built_into(profile.clone(), success(""))]);
    build_workshop(&build_request(Some(target), None), environment, &mut runner)
        .expect("a stub builds Workshop");
    assert_eq!(runner.commands.len(), 1);
    assert!(profile.join("runvil-gateway").is_file());
    assert!(stub.is_file(), "a staged stub must outlive the build");
}

#[test]
fn the_flag_wins_over_the_variable_which_wins_over_the_slot() {
    let target = "x86_64-pc-windows-msvc";
    for (flag, variable, expected) in [
        (true, true, "flag"),
        (false, true, "variable"),
        (false, false, "slot"),
    ] {
        let mut test_environment = environment();
        let slot = test_environment.stage_gateway(target);
        let named = |name: &str| {
            test_environment
                .temp
                .path()
                .join(name)
                .join("runvil-gateway.exe")
        };
        write_file(&named("flag"), b"flag");
        write_file(&named("variable"), b"variable");
        if variable {
            test_environment.environment.gateway_sidecar = Some(named("variable"));
        }
        let environment = &test_environment.environment;
        let mut installer_request = request(target, false);
        if flag {
            installer_request.gateway_binary = Some(named("flag"));
        }
        // A mismatched version stops the run after the version check.
        let mut runner =
            FakeRunner::with_responses(vec![node_found(), success("runvil-gateway 0.0.1\n")]);

        build_installer(&installer_request, environment, &mut runner)
            .expect_err("version mismatch");

        let program = if expected == "slot" {
            slot
        } else {
            named(expected)
        };
        assert_eq!(runner.commands[1].program, program, "{expected}");
        assert_eq!(runner.commands[1].args, strings(&["--version"]));
    }
}

#[test]
fn a_given_gateway_is_staged_for_a_windows_build_and_removed_after() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-pc-windows-msvc";
    let given = test_environment.given_gateway(target);
    let slot = test_environment.sidecar(target);
    let staged = slot.clone();
    let mut runner = FakeRunner::with_responses(vec![act(
        move || {
            assert_eq!(
                std::fs::read(&staged).expect("staged during the build"),
                b"given gateway"
            );
        },
        success(""),
    )]);

    build_workshop(
        &build_request(Some(target), Some(given.clone())),
        environment,
        &mut runner,
    )
    .expect("Workshop build");

    assert_eq!(runner.commands.len(), 1);
    // The desktop build script copies what this variable names into the
    // slot, and the child would otherwise inherit the caller's value.
    assert!(
        runner.commands[0].envs.contains(&(
            OsString::from("RUNVIL_GATEWAY_SIDECAR"),
            given.clone().into_os_string()
        )),
        "{:?}",
        runner.commands[0].envs
    );
    assert!(!slot.exists(), "a given Gateway's staging was left");
    assert!(given.is_file(), "staging moved the given Gateway");
}

#[test]
fn a_given_gateway_never_replaces_one_already_staged_for_windows() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-pc-windows-msvc";
    let slot = test_environment.stage_gateway(target);
    let before = std::fs::read(&slot).expect("the staged Gateway");
    let given = test_environment.given_gateway(target);
    let mut runner = FakeRunner::default();

    let error = build_workshop(
        &build_request(Some(target), Some(given)),
        environment,
        &mut runner,
    )
    .expect_err("a Gateway is already staged");

    assert!(error.primary.contains("already staged"), "{error}");
    assert!(runner.commands.is_empty());
    assert_eq!(std::fs::read(&slot).expect("kept"), before);
}

#[test]
fn a_host_build_copies_a_given_gateway_into_the_profile_directory() {
    let mut test_environment = environment();
    let target = "aarch64-unknown-linux-gnu";
    let given = test_environment.given_gateway(target);
    test_environment.environment.gateway_sidecar = Some(given);
    let environment = &test_environment.environment;
    // Without `--target`, Cargo writes to the target root's profile
    // directory, with no triple.
    let profile = environment.target_root.join("debug");
    let mut runner = FakeRunner::with_responses(vec![
        success(&format!("cargo 1.89.0\nhost: {target}\n")),
        workshop_built_without_sidecar(
            test_environment.sidecar(target),
            workshop_built_into(profile.clone(), success("")),
        ),
    ]);

    build_workshop(&build_request(None, None), environment, &mut runner).expect("Workshop build");

    assert_eq!(
        std::fs::read(profile.join("runvil-gateway")).expect("Gateway beside Workshop"),
        b"given gateway"
    );
    assert!(!test_environment.sidecar(target).exists());
}

/// A Gateway from an earlier run may still be running from the profile
/// directory, so the copy must replace the name and leave that file alone.
#[cfg(unix)]
#[test]
fn a_host_build_replaces_the_gateway_beside_workshop_without_writing_into_it() {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "aarch64-unknown-linux-gnu";
    let given = test_environment.given_gateway(target);
    std::fs::set_permissions(&given, std::fs::Permissions::from_mode(0o644)).expect("mode");
    let profile = environment.target_root.join(target).join("debug");
    let beside = profile.join("runvil-gateway");
    write_file(&beside, b"an earlier gateway");
    let earlier = std::fs::metadata(&beside).expect("earlier").ino();
    let mut runner =
        FakeRunner::with_responses(vec![workshop_built_into(profile.clone(), success(""))]);

    build_workshop(
        &build_request(Some(target), Some(given)),
        environment,
        &mut runner,
    )
    .expect("Workshop build");

    assert_eq!(std::fs::read(&beside).expect("replaced"), b"given gateway");
    let metadata = std::fs::metadata(&beside).expect("replaced");
    assert_eq!(metadata.permissions().mode() & 0o777, 0o755);
    assert_ne!(metadata.ino(), earlier, "the earlier file was written into");
    assert!(!profile.join("runvil-gateway.new").exists());
}

#[test]
fn an_unpinned_gateway_refuses_the_installer_before_any_command() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "aarch64-apple-darwin";
    test_environment.stage_gateway(target);
    test_environment.pin("PLACEHOLDER");
    let mut runner = FakeRunner::default();

    let error = build_installer(&request(target, false), environment, &mut runner)
        .expect_err("an unpinned Gateway");

    assert!(error.primary.contains("names no version"), "{error}");
    assert!(runner.commands.is_empty());
}
