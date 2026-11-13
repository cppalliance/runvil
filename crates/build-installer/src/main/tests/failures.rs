//! Unit tests for the installer's failure and cancellation paths.

use super::ifw_tests::ifw_found;
use super::installer_tests::{bundle_command, bundled, output, request};
use super::*;

#[test]
fn a_missing_named_gateway_fails_before_any_command() {
    for target in ["aarch64-pc-windows-msvc", "x86_64-unknown-linux-gnu"] {
        let test_environment = environment();
        let environment = &test_environment.environment;
        test_environment.stage_gateway(target);
        let missing = test_environment.temp.path().join("runvil-gateway");
        let mut installer_request = request(target, false);
        installer_request.gateway_binary = Some(missing.clone());
        let mut runner = FakeRunner::default();

        let error = build_installer(&installer_request, environment, &mut runner)
            .expect_err("a missing Gateway");

        assert!(error.contains("which is not a file"), "{target}: {error}");
        assert!(
            error.contains(&missing.display().to_string()),
            "{target}: {error}"
        );
        assert!(runner.commands.is_empty(), "{target}");
    }
}

#[test]
fn a_cancel_raced_with_the_bundle_s_completion_fails_and_collects_nothing() {
    for target in ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"] {
        let test_environment = environment();
        let environment = &test_environment.environment;
        test_environment.stage_gateway(target);
        let ifw = (!target.ends_with("windows-msvc")).then(ifw_found);
        let mut runner = FakeRunner::with_responses(
            ifw.into_iter()
                .chain([bundled(Vec::new(), FakeResponse::CompletedAndCanceled)])
                .collect(),
        );

        let error = build_installer(&request(target, false), environment, &mut runner)
            .expect_err("bundle completion race");

        assert_eq!(
            error, "Workshop bundle canceled after child completion",
            "{target}"
        );
        assert_eq!(
            runner.commands.last(),
            Some(&bundle_command(environment, target, false, None)),
            "{target}: no command follows the bundle"
        );
        assert!(!output(environment, target).exists(), "{target}");
    }
}

#[test]
fn malformed_host_output_fails_before_building() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let mut runner = FakeRunner::with_responses(vec![success("cargo 1.89.0\n")]);

    let error = build_installer(
        &InstallerRequest {
            target: None,
            sign: false,
            gateway_binary: None,
        },
        environment,
        &mut runner,
    )
    .expect_err("missing host");

    assert!(error.contains("Cargo host triple"), "{error}");
    assert_eq!(runner.commands.len(), 1);
}

#[test]
fn a_gateway_other_than_the_pinned_release_fails_naming_the_pin() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-unknown-linux-gnu";
    let staged = test_environment.stage_gateway(target);
    let mut runner = FakeRunner::with_responses(vec![
        ifw_found(),
        bundled(Vec::new(), success("")),
        success("runvil-gateway 0.0.1\n"),
    ]);

    let error = build_installer(&request(target, false), environment, &mut runner)
        .expect_err("version mismatch");

    assert!(error.contains("printed `runvil-gateway 0.0.1`"), "{error}");
    assert!(
        error.contains(&format!("expected `runvil-gateway {GATEWAY_VERSION}`")),
        "{error}"
    );
    assert!(
        error.contains("the Workshop checkout's gateway-pin/pin.json"),
        "{error}"
    );
    assert_eq!(runner.commands.len(), 3);
    assert!(!output(environment, target).exists());
    assert!(staged.is_file(), "a staged Gateway must outlive the build");
}

#[test]
fn a_failed_version_check_collects_nothing() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "aarch64-apple-darwin";
    test_environment.stage_gateway(target);
    let mut runner = FakeRunner::with_responses(vec![
        ifw_found(),
        bundled(Vec::new(), success("")),
        failure("bad CPU type in executable"),
    ]);

    let error = build_installer(&request(target, false), environment, &mut runner)
        .expect_err("version check failure");

    assert_eq!(
        error,
        "Gateway version check failed: bad CPU type in executable"
    );
    assert_eq!(runner.commands.len(), 3);
    assert!(!output(environment, target).exists());
}

#[test]
fn cancellation_at_the_version_check_collects_nothing_and_keeps_the_staged_gateway() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-unknown-linux-gnu";
    let staged = test_environment.stage_gateway(target);
    let mut runner = FakeRunner::with_responses(vec![
        ifw_found(),
        bundled(Vec::new(), success("")),
        FakeResponse::CanceledBeforeStart,
    ]);

    let error = build_installer(&request(target, false), environment, &mut runner)
        .expect_err("version check cancellation");

    assert!(error.contains("Gateway version check canceled"), "{error}");
    assert_eq!(runner.commands.len(), 3);
    assert!(!output(environment, target).exists());
    assert!(staged.is_file());
}

#[test]
fn a_failed_unstartable_or_canceled_bundle_collects_nothing() {
    let cases = || {
        [
            (
                failure("bundle broke"),
                "Workshop bundle failed: bundle broke",
            ),
            (
                FakeResponse::SpawnFailed(io::ErrorKind::NotFound, "cargo not found"),
                "Workshop bundle could not start: cargo not found",
            ),
            (
                FakeResponse::CanceledAfterStart,
                "Workshop bundle canceled: canceled",
            ),
        ]
    };
    for (target, given) in [
        ("x86_64-pc-windows-msvc", true),
        ("x86_64-pc-windows-msvc", false),
        ("x86_64-unknown-linux-gnu", true),
        ("x86_64-unknown-linux-gnu", false),
    ] {
        for (response, expected) in cases() {
            let test_environment = environment();
            let environment = &test_environment.environment;
            let staged = test_environment.stage_gateway(target);
            let mut installer_request = request(target, false);
            let named = given.then(|| test_environment.given_gateway());
            installer_request.gateway_binary.clone_from(&named);
            let ifw = (!target.ends_with("windows-msvc")).then(ifw_found);
            let mut runner = FakeRunner::with_responses(
                ifw.into_iter()
                    .chain([bundled(Vec::new(), response)])
                    .collect(),
            );

            let error = build_installer(&installer_request, environment, &mut runner)
                .expect_err("bundle failure");

            assert_eq!(error, expected, "{target}");
            assert_eq!(
                runner.commands.last(),
                Some(&bundle_command(
                    environment,
                    target,
                    false,
                    named.as_deref()
                )),
                "{target}: the bundle is the last command, told the named Gateway"
            );
            assert!(!output(environment, target).exists(), "{target}");
            assert!(staged.is_file(), "{target}: the staged Gateway is left");
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

    assert!(error.contains("x86_64-unknown-freebsd"), "{error}");
    assert!(runner.commands.is_empty());
}
