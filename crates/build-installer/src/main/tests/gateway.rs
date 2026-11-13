//! Unit tests for where the installer finds the Gateway: the named binary,
//! the one staged in the Workshop checkout, and the pin.

use super::ifw_tests::ifw_found;
use super::installer_tests::{VERSION, bundle, bundle_command, bundled, output, request};
use super::*;

#[test]
fn a_missing_gateway_names_the_fetch_script_and_runs_no_command() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-unknown-linux-gnu";
    let mut runner = FakeRunner::default();

    let error = build_installer(&request(target, false), environment, &mut runner)
        .expect_err("no Gateway for the installer");

    assert!(
        error.contains(&format!(
            "run `node tools/fetch-gateway-sidecar.mjs --target {target}` in the Workshop \
             checkout {}",
            environment.workshop_root.display()
        )),
        "{error}"
    );
    assert!(
        error.contains(
            &gateway::staged_path(&environment.workshop_root, target)
                .display()
                .to_string()
        ),
        "{error}"
    );
    assert!(runner.commands.is_empty());
}

#[test]
fn the_flag_wins_over_the_variable_which_wins_over_the_staged_gateway() {
    let target = "x86_64-unknown-linux-gnu";
    for (flag, variable, expected) in [
        (true, true, "flag"),
        (false, true, "variable"),
        (false, false, "staged"),
    ] {
        let mut test_environment = environment();
        let staged = test_environment.stage_gateway(target);
        let named = |name: &str| {
            test_environment
                .temp
                .path()
                .join(name)
                .join("runvil-gateway")
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
        let mut runner = FakeRunner::with_responses(vec![
            ifw_found(),
            bundled(Vec::new(), success("")),
            success("runvil-gateway 0.0.1\n"),
        ]);

        build_installer(&installer_request, environment, &mut runner)
            .expect_err("version mismatch");

        // The bundle is told a named Gateway and left to the staged one,
        // and the payload's Gateway is the same file.
        let passed = (expected != "staged").then(|| named(expected));
        assert_eq!(
            runner.commands[1],
            bundle_command(environment, target, false, passed.as_deref()),
            "{expected}"
        );
        assert_eq!(
            runner.commands[2].program,
            passed.unwrap_or(staged),
            "{expected}"
        );
        assert_eq!(runner.commands[2].args, strings(&["--version"]));
    }
}

#[test]
fn a_named_gateway_is_passed_to_a_windows_bundle_as_gateway_binary() {
    for from_variable in [false, true] {
        let mut test_environment = environment();
        let target = "x86_64-pc-windows-msvc";
        let given = test_environment.given_gateway();
        let mut installer_request = request(target, false);
        if from_variable {
            test_environment.environment.gateway_sidecar = Some(given.clone());
        } else {
            installer_request.gateway_binary = Some(given.clone());
        }
        let environment = &test_environment.environment;
        let setup =
            bundle(environment, target, "nsis").join(format!("Runvil_{VERSION}_x64-setup.exe"));
        let mut runner = FakeRunner::with_responses(vec![bundled(vec![setup], success(""))]);

        build_installer(&installer_request, environment, &mut runner).expect("installer");

        // The setup holds the Gateway, so the installer itself never runs
        // or copies the named file.
        assert_eq!(
            runner.commands,
            vec![CommandSpec {
                program: PathBuf::from("selected-cargo"),
                args: [
                    strings(&["workshop", "bundle", "--target", target, "--gateway-binary"]),
                    vec![given.clone().into_os_string()],
                ]
                .concat(),
                current_dir: environment.workshop_root.clone(),
                envs: target_root_env(environment),
                output_mode: OutputMode::Inherit,
            }],
            "from the variable: {from_variable}"
        );
        assert!(!output(environment, target).join("payload").exists());
    }
}

/// The bundle runs from the Workshop checkout, so a relative name must
/// reach it resolved against this process's working directory.
#[test]
fn a_relative_gateway_name_reaches_the_bundle_as_an_absolute_path() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-pc-windows-msvc";
    // Cargo runs a test from its package's root, which holds this file. A
    // Windows build only passes the name on.
    let relative = Path::new("Cargo.toml");
    let mut installer_request = request(target, false);
    installer_request.gateway_binary = Some(relative.to_path_buf());
    let mut runner = FakeRunner::with_responses(vec![failure("stop after the bundle")]);

    build_installer(&installer_request, environment, &mut runner).expect_err("bundle failure");

    let absolute = std::env::current_dir()
        .expect("current directory")
        .join(relative);
    assert!(absolute.is_absolute());
    assert_eq!(
        runner.commands,
        vec![bundle_command(environment, target, false, Some(&absolute))]
    );
}

#[test]
fn a_named_gateway_must_be_a_file_and_a_relative_name_resolves_once() {
    let test_environment = environment();
    let given = test_environment.given_gateway();
    let missing = test_environment.temp.path().join("missing-gateway");

    let error = gateway::named(Some(&missing)).expect_err("a missing Gateway");
    assert!(error.contains("which is not a file"), "{error}");
    assert!(error.contains(&missing.display().to_string()), "{error}");
    // A relative name is resolved once, against the current directory, and
    // never through PATH.
    let error = gateway::named(Some(Path::new("no-such-runvil-gateway"))).expect_err("a bare name");
    let resolved = std::env::current_dir()
        .expect("current directory")
        .join("no-such-runvil-gateway");
    assert!(error.contains(&resolved.display().to_string()), "{error}");

    assert_eq!(gateway::named(Some(&given)), Ok(Some(given)));
    assert_eq!(gateway::named(None), Ok(None));
}

#[test]
fn the_staged_gateway_is_named_with_tauri_s_target_suffix() {
    let root = Path::new("runvil-workshop");
    let binaries = root
        .join("crates")
        .join("workshop")
        .join("desktop")
        .join("binaries");
    for (target, staged) in [
        (
            "x86_64-unknown-linux-gnu",
            "runvil-gateway-x86_64-unknown-linux-gnu",
        ),
        (
            "aarch64-unknown-linux-gnu",
            "runvil-gateway-aarch64-unknown-linux-gnu",
        ),
        (
            "aarch64-apple-darwin",
            "runvil-gateway-aarch64-apple-darwin",
        ),
        ("x86_64-apple-darwin", "runvil-gateway-x86_64-apple-darwin"),
    ] {
        assert_eq!(
            gateway::staged_path(root, target),
            binaries.join(staged),
            "{target}"
        );
    }
}

#[test]
fn a_named_gateway_wins_over_the_staged_one_in_the_payload() {
    let test_environment = environment();
    let root = &test_environment.environment.workshop_root;
    let target = "aarch64-apple-darwin";
    let given = test_environment.given_gateway();

    let error = gateway::payload_binary(None, root, target).expect_err("nothing staged");
    assert!(
        error.contains(&format!(
            "node tools/fetch-gateway-sidecar.mjs --target {target}"
        )),
        "{error}"
    );
    assert_eq!(
        gateway::payload_binary(Some(&given), root, target),
        Ok(given.clone())
    );

    let staged = test_environment.stage_gateway(target);
    assert_eq!(gateway::payload_binary(None, root, target), Ok(staged));
    assert_eq!(
        gateway::payload_binary(Some(&given), root, target),
        Ok(given)
    );
}

#[test]
fn the_pinned_version_is_the_pin_s_version_and_a_placeholder_or_missing_pin_fails() {
    let test_environment = environment();
    let root = &test_environment.environment.workshop_root;
    assert_eq!(
        gateway::pinned_version(root),
        Ok(GATEWAY_VERSION.to_owned())
    );

    let pin = root.join("gateway-pin").join("pin.json");
    for (contents, expected) in [
        (r#"{"version": "PLACEHOLDER"}"#, "names no version"),
        (r#"{"version": ""}"#, "names no version"),
        (r#"{"tag": "gateway-v1.0.0"}"#, "names no version"),
        ("not json", "is not JSON"),
    ] {
        write_file(&pin, contents.as_bytes());
        let error = gateway::pinned_version(root).expect_err(contents);
        assert!(error.contains(expected), "{contents}: {error}");
        assert!(error.contains(&pin.display().to_string()), "{error}");
    }
    std::fs::remove_file(&pin).expect("remove the pin");
    let error = gateway::pinned_version(root).expect_err("missing pin");
    assert!(error.contains("cannot read the Gateway pin"), "{error}");
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

    assert!(error.contains("names no version"), "{error}");
    assert!(runner.commands.is_empty());
}
