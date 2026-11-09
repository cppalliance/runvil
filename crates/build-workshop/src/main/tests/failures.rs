//! Unit tests for failure, cancellation, and sidecar cleanup paths of both
//! modes. A given Gateway exercises the Windows staging and cleanup.

use super::ifw_tests::ifw_found;
use super::installer_tests::{bundled, node_found, output, request, version_printed};
use super::*;

/// A debug build for `target` with a Gateway given from outside the slot,
/// which Windows targets stage for the build and remove after.
fn given_request(target: &str, gateway: PathBuf) -> BuildRequest {
    BuildRequest {
        profile: Profile::Debug,
        target: Some(target.to_owned()),
        gateway_binary: Some(gateway),
    }
}

/// Stands in for a file the cleanup cannot remove: a directory with content
/// at the sidecar's path.
fn block_sidecar_removal(sidecar: PathBuf) -> impl FnOnce() {
    move || {
        std::fs::remove_file(&sidecar).expect("remove the staged sidecar");
        write_file(&sidecar.join("held"), b"held");
    }
}

#[test]
fn workshop_failure_is_primary_when_cleanup_also_fails() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-pc-windows-msvc";
    let given = test_environment.given_gateway(triple);
    let mut runner = FakeRunner::with_responses(vec![act(
        block_sidecar_removal(test_environment.sidecar(triple)),
        failure("workshop broke"),
    )]);

    let error = build_workshop(&given_request(triple, given), environment, &mut runner)
        .expect_err("Workshop and cleanup failure");

    assert!(error.primary.contains("Workshop build failed"), "{error}");
    assert!(error.primary.contains("workshop broke"), "{error}");
    let cleanup = error.cleanup.expect("separate cleanup failure");
    assert!(
        cleanup.contains("Gateway sidecar cleanup failed"),
        "{cleanup}"
    );
    assert!(cleanup.contains("cannot remove"), "{cleanup}");
}

#[test]
fn a_missing_given_gateway_fails_before_any_build_and_stages_nothing() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "aarch64-pc-windows-msvc";
    let missing = environment.target_root.join("runvil-gateway.exe");
    let mut runner = FakeRunner::default();

    let error = build_workshop(
        &given_request(triple, missing.clone()),
        environment,
        &mut runner,
    )
    .expect_err("a missing Gateway");

    assert!(error.primary.contains("which is not a file"), "{error}");
    assert!(
        error.primary.contains(&missing.display().to_string()),
        "{error}"
    );
    assert!(runner.commands.is_empty());
    assert!(!test_environment.sidecar(triple).exists());
}

#[test]
fn cancellation_before_staging_stages_nothing() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-pc-windows-msvc";
    let given = test_environment.given_gateway(triple);
    let mut runner = FakeRunner {
        cancel_observed: true,
        ..FakeRunner::default()
    };

    let error = build_workshop(&given_request(triple, given), environment, &mut runner)
        .expect_err("pre-staging cancellation");

    assert!(
        error.primary.contains("Gateway sidecar staging canceled"),
        "{error}"
    );
    assert_eq!(error.cleanup, None);
    assert!(runner.commands.is_empty());
    assert!(!test_environment.sidecar(triple).exists());
}

#[test]
fn cancellation_raced_with_completion_removes_the_sidecar() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-pc-windows-msvc";
    let given = test_environment.given_gateway(triple);
    let mut runner = FakeRunner::with_responses(vec![workshop_built_with_sidecar(
        test_environment.sidecar(triple),
        FakeResponse::CompletedAndCanceled,
    )]);

    let error = build_workshop(&given_request(triple, given), environment, &mut runner)
        .expect_err("Workshop completion race");

    assert!(
        error
            .primary
            .contains("Workshop build canceled after child completion"),
        "{error}"
    );
    assert!(!test_environment.sidecar(triple).exists());
}

#[test]
fn cancellation_raced_with_completion_fails_a_build_without_a_sidecar() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-unknown-linux-gnu";
    let given = test_environment.given_gateway(triple);
    let profile = environment.target_root.join(triple).join("debug");
    let mut runner = FakeRunner::with_responses(vec![workshop_built_without_sidecar(
        test_environment.sidecar(triple),
        workshop_built_into(profile, FakeResponse::CompletedAndCanceled),
    )]);

    let error = build_workshop(&given_request(triple, given), environment, &mut runner)
        .expect_err("Workshop completion race");

    assert!(
        error
            .primary
            .contains("Workshop build canceled after child completion"),
        "{error}"
    );
    assert_eq!(error.cleanup, None);
}

#[test]
fn cancellation_preserves_cleanup_failure_diagnostics() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-pc-windows-msvc";
    let given = test_environment.given_gateway(triple);
    let mut runner = FakeRunner::with_responses(vec![act(
        block_sidecar_removal(test_environment.sidecar(triple)),
        FakeResponse::CanceledAfterStart,
    )]);

    let error = build_workshop(&given_request(triple, given), environment, &mut runner)
        .expect_err("Workshop cancellation and cleanup failure");

    assert!(error.primary.contains("Workshop build canceled"), "{error}");
    let cleanup = error.cleanup.expect("cleanup diagnostic");
    assert!(
        cleanup.contains("Gateway sidecar cleanup failed"),
        "{cleanup}"
    );
}

#[test]
fn cleanup_failure_after_success_fails_the_command_clearly() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-pc-windows-msvc";
    let given = test_environment.given_gateway(triple);
    let mut runner = FakeRunner::with_responses(vec![act(
        block_sidecar_removal(test_environment.sidecar(triple)),
        success(""),
    )]);

    let error = build_workshop(&given_request(triple, given), environment, &mut runner)
        .expect_err("cleanup failure");

    assert!(
        error.primary.contains("Gateway sidecar cleanup failed"),
        "{error}"
    );
    assert_eq!(error.cleanup, None);
}

#[test]
fn malformed_host_output_fails_before_building() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let mut runner = FakeRunner::with_responses(vec![success("cargo 1.89.0\n")]);

    let error = build_workshop(
        &BuildRequest {
            profile: Profile::Debug,
            target: None,
            gateway_binary: None,
        },
        environment,
        &mut runner,
    )
    .expect_err("missing host");

    assert!(error.primary.contains("Cargo host triple"), "{error}");
    assert_eq!(runner.commands.len(), 1);
}

#[test]
fn a_gateway_other_than_the_pinned_release_fails_naming_the_pin() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-pc-windows-msvc";
    let staged = test_environment.stage_gateway(target);
    let mut runner =
        FakeRunner::with_responses(vec![node_found(), success("runvil-gateway 0.0.1\n")]);

    let error = build_installer(&request(target, false), environment, &mut runner)
        .expect_err("version mismatch");

    assert!(
        error.primary.contains("printed `runvil-gateway 0.0.1`"),
        "{error}"
    );
    assert!(
        error
            .primary
            .contains(&format!("expected `runvil-gateway {GATEWAY_VERSION}`")),
        "{error}"
    );
    assert!(error.primary.contains("gateway-pin/pin.json"), "{error}");
    assert_eq!(runner.commands.len(), 2);
    assert!(staged.is_file(), "a staged Gateway must outlive the build");
}

#[test]
fn a_failed_version_check_stops_before_the_bundle() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "aarch64-pc-windows-msvc";
    test_environment.stage_gateway(target);
    let mut runner =
        FakeRunner::with_responses(vec![node_found(), failure("bad CPU type in executable")]);

    let error = build_installer(&request(target, false), environment, &mut runner)
        .expect_err("version check failure");

    assert!(
        error
            .primary
            .contains("Gateway version check failed: bad CPU type in executable"),
        "{error}"
    );
    assert_eq!(runner.commands.len(), 2);
}

#[test]
fn cancellation_before_the_version_check_runs_no_bundle_and_keeps_the_staged_gateway() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-pc-windows-msvc";
    let staged = test_environment.stage_gateway(target);
    let mut runner =
        FakeRunner::with_responses(vec![node_found(), FakeResponse::CanceledBeforeStart]);

    let error = build_installer(&request(target, false), environment, &mut runner)
        .expect_err("version check cancellation");

    assert!(
        error.primary.contains("Gateway version check canceled"),
        "{error}"
    );
    assert_eq!(runner.commands.len(), 2);
    assert!(staged.is_file());
}

#[test]
fn a_failed_unstartable_or_canceled_bundle_collects_nothing_and_removes_only_a_given_gateway() {
    let cases = || {
        [
            (
                failure("bundle broke"),
                "Workshop bundle failed: bundle broke",
            ),
            (
                FakeResponse::SpawnFailed(io::ErrorKind::NotFound, "node not found"),
                "Workshop bundle could not start: node not found",
            ),
            (FakeResponse::CanceledAfterStart, "Workshop bundle canceled"),
        ]
    };
    for (target, given) in [
        ("x86_64-pc-windows-msvc", true),
        ("x86_64-pc-windows-msvc", false),
        ("x86_64-unknown-linux-gnu", false),
    ] {
        for (response, expected) in cases() {
            let test_environment = environment();
            let environment = &test_environment.environment;
            let mut installer_request = request(target, false);
            let named = given.then(|| test_environment.given_gateway(target));
            if given {
                installer_request.gateway_binary.clone_from(&named);
            } else {
                test_environment.stage_gateway(target);
            }
            let ifw = (!sidecar::bundles_sidecar(target)).then(ifw_found);
            let mut runner = FakeRunner::with_responses(
                ifw.into_iter()
                    .chain([
                        node_found(),
                        version_printed(),
                        bundled(test_environment.sidecar(target), Vec::new(), response),
                    ])
                    .collect(),
            );

            let error = build_installer(&installer_request, environment, &mut runner)
                .expect_err("bundle failure");

            assert!(error.primary.contains(expected), "{target}: {error}");
            // The bundle's build script copies what this variable names
            // into the slot, so the bundle is told the checked Gateway.
            let bundle = runner.commands.last().expect("the bundle command");
            let told = bundle
                .envs
                .iter()
                .find(|(key, _)| key == "RUNVIL_GATEWAY_SIDECAR")
                .map(|(_, value)| PathBuf::from(value));
            assert_eq!(told, named, "{target}");
            assert_eq!(
                test_environment.sidecar(target).exists(),
                !given,
                "{target}: only a given Gateway is removed"
            );
            assert!(!output(environment, target).exists(), "{target}");
        }
    }
}

#[test]
fn an_unsupported_target_fails_before_building() {
    let test_environment = environment();
    let mut runner = FakeRunner::default();

    let error = build_installer(
        &request("x86_64-unknown-freebsd", false),
        &test_environment.environment,
        &mut runner,
    )
    .expect_err("unsupported target");

    assert!(error.primary.contains("x86_64-unknown-freebsd"), "{error}");
    assert!(runner.commands.is_empty());
}

#[test]
fn a_missing_node_fails_before_the_gateway_version_check() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-unknown-linux-gnu";
    test_environment.stage_gateway(target);
    let mut runner = FakeRunner::with_responses(vec![
        ifw_found(),
        FakeResponse::SpawnFailed(io::ErrorKind::NotFound, "program not found"),
    ]);

    let error = build_installer(&request(target, false), environment, &mut runner)
        .expect_err("missing node");

    assert!(
        error
            .primary
            .contains("the Tauri CLI runs on `node` from PATH) could not start"),
        "{error}"
    );
    assert_eq!(runner.commands.len(), 2);
}
