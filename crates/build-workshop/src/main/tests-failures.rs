//! Unit tests for failure, interruption, and sidecar cleanup paths.
//!
//! With the Gateway a separate product, every case here names its sidecar
//! up front, so the first command a run issues is the staging script. The
//! cleanup that follows is the run's own: a sidecar it staged is removed,
//! a sidecar it found already in place is left alone.

use super::*;

/// A binary path the run stages from, so no case needs a Gateway build.
fn given() -> PathBuf {
    PathBuf::from("/ci/staged/runvil-gateway")
}

#[test]
fn a_staging_failure_still_removes_any_staged_sidecar() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-pc-windows-msvc";
    let mut runner = FakeRunner::with_responses(vec![failure("copy broke"), success("")]);

    let error = build_workshop(
        &BuildRequest {
            profile: Profile::Debug,
            target: Some(triple.to_owned()),
            gateway_binary: Some(given()),
        },
        environment,
        &mut runner,
    )
    .expect_err("staging failure");

    assert!(
        error.primary.contains("Gateway sidecar staging failed"),
        "{error}"
    );
    assert!(error.primary.contains("copy broke"), "{error}");
    assert_eq!(error.cleanup, None);
    assert_eq!(runner.commands.len(), 2);
    assert_eq!(
        runner.commands[1],
        command(
            environment,
            "selected-node",
            &[
                environment
                    .workspace_root
                    .join("tools")
                    .join("stage-gateway-sidecar.mjs")
                    .to_str()
                    .expect("UTF-8 script"),
                "remove",
                "--target",
                triple,
            ],
            OutputMode::Inherit,
        )
    );
}

#[test]
fn workshop_failure_is_primary_when_cleanup_also_fails() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-unknown-linux-gnu";
    let mut runner = FakeRunner::with_responses(vec![
        success(""),
        failure("workshop broke"),
        FakeResponse::SpawnFailed(io::ErrorKind::NotFound, "node disappeared"),
    ]);

    let error = build_workshop(
        &BuildRequest {
            profile: Profile::Release,
            target: Some(triple.to_owned()),
            gateway_binary: Some(given()),
        },
        environment,
        &mut runner,
    )
    .expect_err("Workshop and cleanup failure");

    assert!(error.primary.contains("Workshop build failed"), "{error}");
    assert!(error.primary.contains("workshop broke"), "{error}");
    let cleanup = error.cleanup.expect("separate cleanup failure");
    assert!(cleanup.contains("Gateway sidecar cleanup"), "{cleanup}");
    assert!(cleanup.contains("node disappeared"), "{cleanup}");
}

#[test]
fn stage_failure_is_followed_by_cleanup() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let mut runner = FakeRunner::with_responses(vec![failure("copy failed"), success("")]);

    let error = build_workshop(
        &BuildRequest {
            profile: Profile::Debug,
            target: Some("x86_64-unknown-linux-gnu".to_owned()),
            gateway_binary: Some(given()),
        },
        environment,
        &mut runner,
    )
    .expect_err("staging failure");

    assert!(error.primary.contains("Gateway sidecar staging failed"));
    assert_eq!(runner.commands.len(), 2);
    assert_eq!(
        runner.commands[1].args[1..],
        strings(&["remove", "--target", "x86_64-unknown-linux-gnu"])
    );
}

#[test]
fn interruption_before_staging_does_not_run_removal() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let mut runner = FakeRunner::with_responses(vec![FakeResponse::InterruptedBeforeStart]);

    let error = build_workshop(
        &BuildRequest {
            profile: Profile::Debug,
            target: Some("x86_64-pc-windows-msvc".to_owned()),
            gateway_binary: Some(given()),
        },
        environment,
        &mut runner,
    )
    .expect_err("staging interruption");

    assert!(
        error
            .primary
            .contains("Gateway sidecar staging interrupted"),
        "{error}"
    );
    assert_eq!(error.cleanup, None);
    assert_eq!(runner.commands.len(), 1);
}

#[test]
fn a_given_sidecar_is_never_built_and_the_target_is_reported() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-unknown-linux-gnu";
    let mut runner = FakeRunner::with_responses(vec![success(""), success(""), success("")]);

    build_workshop(
        &BuildRequest {
            profile: Profile::Debug,
            target: Some(triple.to_owned()),
            gateway_binary: Some(given()),
        },
        environment,
        &mut runner,
    )
    .expect("Workshop build");

    assert!(
        runner.commands.iter().all(|command| {
            !(command.args.len() > 2 && command.args[0] == "build" && command.args[2] == "gateway")
        }),
        "no Gateway build runs: {:?}",
        runner.commands
    );
    assert_eq!(
        runner.commands[0].args,
        strings(&[
            environment
                .workspace_root
                .join("tools")
                .join("stage-gateway-sidecar.mjs")
                .to_str()
                .expect("UTF-8 script"),
            "stage",
            "--target",
            triple,
            "--source",
            "/ci/staged/runvil-gateway",
        ]),
        "the first command stages the given binary"
    );
}

#[test]
fn interruption_during_staging_runs_target_cleanup_once() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-pc-windows-msvc";
    let mut runner =
        FakeRunner::with_responses(vec![FakeResponse::InterruptedAfterStart, success("")]);

    let error = build_workshop(
        &BuildRequest {
            profile: Profile::Debug,
            target: Some(triple.to_owned()),
            gateway_binary: Some(given()),
        },
        environment,
        &mut runner,
    )
    .expect_err("staging interruption");

    assert!(
        error
            .primary
            .contains("Gateway sidecar staging interrupted"),
        "{error}"
    );
    let removals = runner
        .commands
        .iter()
        .filter(|command| command.args[1..] == strings(&["remove", "--target", triple]))
        .count();
    assert_eq!(removals, 1);
}

#[test]
fn interruption_raced_with_completion_runs_cleanup_once() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-pc-windows-msvc";
    let mut runner = FakeRunner::with_responses(vec![
        success(""),
        FakeResponse::CompletedAndInterrupted,
        success(""),
    ]);

    let error = build_workshop(
        &BuildRequest {
            profile: Profile::Debug,
            target: Some(triple.to_owned()),
            gateway_binary: Some(given()),
        },
        environment,
        &mut runner,
    )
    .expect_err("Workshop completion race");

    assert!(
        error.primary.contains("Workshop build interrupted"),
        "{error}"
    );
    let removals = runner
        .commands
        .iter()
        .filter(|command| command.args[1..] == strings(&["remove", "--target", triple]))
        .count();
    assert_eq!(removals, 1);
}

#[test]
fn interruption_preserves_cleanup_failure_diagnostics() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let mut runner = FakeRunner::with_responses(vec![
        success(""),
        FakeResponse::InterruptedAfterStart,
        failure("remove broke"),
    ]);

    let error = build_workshop(
        &BuildRequest {
            profile: Profile::Debug,
            target: Some("x86_64-unknown-linux-gnu".to_owned()),
            gateway_binary: Some(given()),
        },
        environment,
        &mut runner,
    )
    .expect_err("Workshop interruption and cleanup failure");

    assert!(
        error.primary.contains("Workshop build interrupted"),
        "{error}"
    );
    let cleanup = error.cleanup.expect("cleanup diagnostic");
    assert!(
        cleanup.contains("Gateway sidecar cleanup failed"),
        "{cleanup}"
    );
    assert!(cleanup.contains("remove broke"), "{cleanup}");
}

#[test]
fn cleanup_failure_after_success_fails_the_command_clearly() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let mut runner =
        FakeRunner::with_responses(vec![success(""), success(""), failure("remove failed")]);

    let error = build_workshop(
        &BuildRequest {
            profile: Profile::Debug,
            target: Some("x86_64-unknown-linux-gnu".to_owned()),
            gateway_binary: Some(given()),
        },
        environment,
        &mut runner,
    )
    .expect_err("cleanup failure");

    assert!(
        error.primary.contains("Gateway sidecar cleanup failed"),
        "{error}"
    );
    assert!(error.primary.contains("remove failed"), "{error}");
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
