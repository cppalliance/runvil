//! Unit tests for locating Qt IFW, its version check, and the installer
//! commands, with the helpers the installer sequences share.

use super::installer_tests::{bundle, bundled, output, request, version_printed};
use super::*;
use crate::installer::Platform;
use crate::installer::ifw::{self, IFW_VERSION, Ifw};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Writes stand-ins for the two IFW programs under `root/bin/` and returns
/// `root`, the directory `QTIFWDIR` names.
pub(super) fn fake_ifw(root: &Path) -> PathBuf {
    let ifw = root.join("qtifw");
    for program in ["binarycreator", "installerbase"] {
        write_file(&ifw.join("bin").join(program), b"ifw");
    }
    ifw
}

pub(super) fn ifw_found() -> FakeResponse {
    success(&format!(
        "IFW Version: {IFW_VERSION}, built with Qt 6.9.3.\nBuild date: Mar 27 2026\n"
    ))
}

fn located(environment: &BuildEnvironment) -> Ifw {
    let bin = PathBuf::from(environment.qtifw_dir.as_ref().expect("QTIFWDIR")).join("bin");
    Ifw {
        binarycreator: bin.join("binarycreator"),
        installerbase: bin.join("installerbase"),
    }
}

pub(super) fn ifw_version_command(environment: &BuildEnvironment) -> CommandSpec {
    ifw::version_command(&located(environment), &environment.workspace_root)
}

fn platform(target: &str) -> Platform {
    Platform::from_triple(target).expect("platform")
}

fn stage_dir(environment: &BuildEnvironment, target: &str) -> PathBuf {
    output(environment, target).join("ifw")
}

/// The installer `publish/` ends with for a macOS or Linux target.
pub(super) fn installer_path(environment: &BuildEnvironment, target: &str) -> PathBuf {
    let publish = output(environment, target).join("publish");
    let platform = platform(target);
    if is_macos(target) {
        ifw::dmg_path(platform, &publish)
    } else {
        ifw::binarycreator_output(platform, &stage_dir(environment, target), &publish)
    }
}

fn built_path(environment: &BuildEnvironment, target: &str) -> PathBuf {
    let publish = output(environment, target).join("publish");
    ifw::binarycreator_output(platform(target), &stage_dir(environment, target), &publish)
}

/// binarycreator, then on macOS codesign and hdiutil, as the build runs
/// them last.
pub(super) fn ifw_commands(environment: &BuildEnvironment, target: &str) -> Vec<CommandSpec> {
    let stage = stage_dir(environment, target);
    let built = built_path(environment, target);
    let mut commands = vec![ifw::binarycreator_command(
        &located(environment),
        &stage,
        &built,
    )];
    if is_macos(target) {
        commands.push(ifw::adhoc_sign_command(&built));
        commands.push(ifw::hdiutil_command(
            &stage.join("dmg"),
            &installer_path(environment, target),
        ));
    }
    commands
}

/// Fake binarycreator, codesign, and hdiutil runs that write what the real
/// ones do.
pub(super) fn ifw_built(environment: &BuildEnvironment, target: &str) -> Vec<FakeResponse> {
    let built = built_path(environment, target);
    let config = stage_dir(environment, target)
        .join("config")
        .join("config.xml");
    let staged = move || assert!(config.is_file(), "binarycreator ran before staging");
    if is_macos(target) {
        let dmg = installer_path(environment, target);
        vec![
            act(
                move || {
                    staged();
                    write_file(&built.join("Contents").join("Info.plist"), b"app");
                },
                success(""),
            ),
            success(""),
            act(move || write_file(&dmg, b"dmg"), success("")),
        ]
    } else {
        vec![act(
            move || {
                staged();
                write_file(&built, b"run");
            },
            success(""),
        )]
    }
}

pub(super) fn is_macos(target: &str) -> bool {
    target.ends_with("apple-darwin")
}

#[test]
fn qtifw_dir_wins_over_path() {
    let temp = tempfile::tempdir().expect("temporary root");
    let configured = fake_ifw(&temp.path().join("configured"));
    let on_path = fake_ifw(&temp.path().join("on-path"));
    let path = std::env::join_paths([on_path.join("bin")]).expect("PATH");

    let found =
        ifw::locate(Some(configured.as_os_str()), Some(&path), temp.path()).expect("locate");

    assert_eq!(
        found.binarycreator,
        configured.join("bin").join("binarycreator")
    );
    assert_eq!(
        found.installerbase,
        configured.join("bin").join("installerbase")
    );
}

#[test]
fn a_qtifw_dir_without_binarycreator_fails_naming_it_even_with_one_on_path() {
    let temp = tempfile::tempdir().expect("temporary root");
    let stale = temp.path().join("stale");
    std::fs::create_dir_all(stale.join("bin")).expect("stale bin");
    let on_path = fake_ifw(&temp.path().join("on-path"));
    let path = std::env::join_paths([on_path.join("bin")]).expect("PATH");

    let error =
        ifw::locate(Some(stale.as_os_str()), Some(&path), temp.path()).expect_err("stale QTIFWDIR");

    assert!(error.contains("QTIFWDIR is"), "{error}");
    assert!(
        error.contains(
            &stale
                .join("bin")
                .join("binarycreator")
                .display()
                .to_string()
        ),
        "{error}"
    );
}

#[test]
fn the_first_path_entry_with_binarycreator_wins_and_a_directory_is_no_match() {
    let temp = tempfile::tempdir().expect("temporary root");
    let empty = temp.path().join("empty");
    std::fs::create_dir_all(&empty).expect("empty");
    let directory = temp.path().join("directory");
    std::fs::create_dir_all(directory.join("binarycreator")).expect("directory");
    let first = fake_ifw(&temp.path().join("first"));
    let second = fake_ifw(&temp.path().join("second"));
    let path = std::env::join_paths([empty, directory, first.join("bin"), second.join("bin")])
        .expect("PATH");

    let found = ifw::locate(None, Some(&path), temp.path()).expect("locate");

    assert_eq!(found.binarycreator, first.join("bin").join("binarycreator"));
    assert_eq!(found.installerbase, first.join("bin").join("installerbase"));
}

#[test]
fn no_ifw_fails_naming_qtifw_dir_and_path_and_a_lone_binarycreator_names_installerbase() {
    let temp = tempfile::tempdir().expect("temporary root");
    for qtifw_dir in [None, Some(OsString::new())] {
        let error = ifw::locate(qtifw_dir.as_deref(), None, temp.path()).expect_err("no IFW");
        assert!(error.contains("QTIFWDIR is unset"), "{error}");
        assert!(error.contains("PATH"), "{error}");
        assert!(error.contains(IFW_VERSION), "{error}");
    }

    let lone = temp.path().join("lone");
    write_file(&lone.join("bin").join("binarycreator"), b"ifw");
    let error =
        ifw::locate(Some(lone.as_os_str()), None, temp.path()).expect_err("no installerbase");
    assert!(
        error.contains(&lone.join("bin").join("installerbase").display().to_string()),
        "{error}"
    );
}

/// Each child runs in its own working directory, so a relative value must
/// come back absolute.
#[test]
fn relative_qtifw_dir_and_path_entries_resolve_against_the_working_directory() {
    let temp = tempfile::tempdir().expect("temporary root");
    let ifw = fake_ifw(&temp.path().join("tools"));
    let relative = Path::new("tools").join("qtifw");

    let found = ifw::locate(Some(relative.as_os_str()), None, temp.path()).expect("QTIFWDIR");
    assert_eq!(found.binarycreator, ifw.join("bin").join("binarycreator"));
    assert!(found.installerbase.is_absolute());

    let path = std::env::join_paths([relative.join("bin")]).expect("PATH");
    let found = ifw::locate(None, Some(&path), temp.path()).expect("PATH");
    assert_eq!(found.binarycreator, ifw.join("bin").join("binarycreator"));
    assert_eq!(found.installerbase, ifw.join("bin").join("installerbase"));
}

#[test]
fn the_version_check_requires_the_pinned_release() {
    let ifw = Ifw {
        binarycreator: PathBuf::from("/opt/ifw/bin/binarycreator"),
        installerbase: PathBuf::from("/opt/ifw/bin/installerbase"),
    };
    ifw::check_version(
        &ifw,
        "IFW Version: 4.11.0, built with Qt 6.9.3.\nBuild date: Mar 27 2026\n",
    )
    .expect("pinned version");

    let error = ifw::check_version(&ifw, "IFW Version: 4.10.0, built with Qt 6.8.0.\n")
        .expect_err("older version");
    assert!(error.contains("Qt IFW 4.10.0 found"), "{error}");
    assert!(error.contains("4.11.0 required"), "{error}");
    assert!(error.contains("/opt/ifw/bin/installerbase"), "{error}");

    let error = ifw::check_version(&ifw, "Usage: installerbase\n").expect_err("no version");
    assert!(error.contains("no `IFW Version:` line"), "{error}");
}

#[test]
fn a_failed_or_unstartable_installerbase_fails_naming_it_before_any_build() {
    for response in [
        failure("dyld: missing library"),
        FakeResponse::SpawnFailed(io::ErrorKind::PermissionDenied, "permission denied"),
    ] {
        let test_environment = environment();
        test_environment.stage_gateway("aarch64-apple-darwin");
        let environment = &test_environment.environment;
        let mut runner = FakeRunner::with_responses(vec![response]);

        let error = build_installer(
            &request("aarch64-apple-darwin", false),
            environment,
            &mut runner,
        )
        .expect_err("version check");

        let installerbase = located(environment).installerbase;
        assert!(
            error.contains(&format!("`{} --version`", installerbase.display())),
            "{error}"
        );
        assert_eq!(runner.commands, vec![ifw_version_command(environment)]);
    }
}

#[test]
fn a_missing_ifw_runs_no_build_on_macos_or_linux() {
    let mut test_environment = environment();
    test_environment.environment.qtifw_dir = None;
    for target in [
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
    ] {
        test_environment.stage_gateway(target);
    }
    let environment = &test_environment.environment;
    for target in ["x86_64-apple-darwin", "aarch64-unknown-linux-gnu"] {
        let mut runner = FakeRunner::default();
        let error =
            build_installer(&request(target, false), environment, &mut runner).expect_err("no IFW");
        assert!(error.contains("QTIFWDIR is unset"), "{error}");
        assert!(runner.commands.is_empty(), "{target}");
    }

    let mut runner =
        FakeRunner::with_responses(vec![success("cargo 1.89.0\nhost: aarch64-apple-darwin\n")]);
    build_installer(
        &InstallerRequest {
            target: None,
            sign: false,
            gateway_binary: None,
        },
        environment,
        &mut runner,
    )
    .expect_err("no IFW");
    assert_eq!(
        runner.commands,
        vec![command(
            environment,
            "selected-cargo",
            &["-vV"],
            OutputMode::Capture
        )]
    );
}

/// The responses up to the IFW stage of an unsigned macOS or Linux build.
fn bundled_responses(test_environment: &TestEnvironment, target: &str) -> Vec<FakeResponse> {
    let environment = &test_environment.environment;
    test_environment.stage_gateway(target);
    let file = if is_macos(target) {
        bundle(environment, target, "macos")
            .join("Runvil.app")
            .join("Contents")
            .join("MacOS")
            .join("runvil-workshop")
    } else {
        bundle(environment, target, "appimage").join(format!("Runvil_{VERSION}_amd64.AppImage"))
    };
    vec![
        ifw_found(),
        bundled(vec![file], success("")),
        version_printed(),
    ]
}

#[test]
fn a_failing_or_silent_installer_step_fails_naming_the_step_or_the_path() {
    let linux = "x86_64-unknown-linux-gnu";
    let macos = "aarch64-apple-darwin";
    let silent = "succeeded but wrote nothing at";
    let cases = [
        (
            linux,
            0,
            failure("bad config"),
            "Qt IFW binarycreator failed: bad config",
        ),
        (linux, 0, success(""), "Qt IFW binarycreator"),
        (macos, 0, success(""), "Qt IFW binarycreator"),
        (
            macos,
            1,
            failure("not signable"),
            "Installer app ad hoc signing (codesign) failed: not signable",
        ),
        (
            macos,
            2,
            failure("no space"),
            "DMG wrap (hdiutil) failed: no space",
        ),
        (macos, 2, success(""), "DMG wrap (hdiutil)"),
    ];
    for (target, step, response, expected) in cases {
        let test_environment = environment();
        let environment = &test_environment.environment;
        let wrote_nothing = matches!(&response, FakeResponse::Completed(result) if result.success);
        let mut responses = bundled_responses(&test_environment, target);
        let mut built = ifw_built(environment, target);
        built.truncate(step);
        responses.extend(built);
        responses.push(response);
        let mut runner = FakeRunner::with_responses(responses);

        let error = build_installer(&request(target, false), environment, &mut runner)
            .expect_err("IFW step failure");

        assert!(error.contains(expected), "{target}: {error}");
        if wrote_nothing {
            let path = if step == 0 {
                built_path(environment, target)
            } else {
                installer_path(environment, target)
            };
            let message = format!("{expected} {silent} {}", path.display());
            assert_eq!(error, message, "{target}");
        }
        assert!(!installer_path(environment, target).exists(), "{target}");
    }
}
