//! Unit tests for the installer command sequence, its preflight, and its
//! failure paths.

use super::ifw_tests::{ifw_built, ifw_commands, ifw_found, ifw_version_command, installer_path};
use super::*;

pub(super) const VERSION: &str = env!("CARGO_PKG_VERSION");

fn release(environment: &BuildEnvironment, target: &str) -> PathBuf {
    environment.target_root.join(target).join("release")
}

/// The staged Gateway, which the version check runs.
pub(super) fn gateway(environment: &BuildEnvironment, target: &str) -> PathBuf {
    sidecar::sidecar_path(&environment.workspace_root, target)
}

pub(super) fn bundle(environment: &BuildEnvironment, target: &str, directory: &str) -> PathBuf {
    release(environment, target).join("bundle").join(directory)
}

pub(super) fn output(environment: &BuildEnvironment, target: &str) -> PathBuf {
    environment.target_root.join("installer").join(target)
}

pub(super) fn node_found() -> FakeResponse {
    success("v22.0.0\n")
}

pub(super) fn version_printed() -> FakeResponse {
    success(&format!("runvil-gateway {GATEWAY_VERSION}\n"))
}

/// A Tauri build that writes `files`, with a Gateway in the slot: one the
/// fetch script staged for any target, or a given one staged for Windows.
pub(super) fn bundled(
    sidecar: PathBuf,
    files: Vec<PathBuf>,
    response: FakeResponse,
) -> FakeResponse {
    act(
        move || {
            assert!(
                sidecar.is_file(),
                "no Gateway in the slot during the bundle"
            );
            for file in files {
                write_file(&file, b"bundle");
            }
        },
        response,
    )
}

pub(super) fn expected_commands(
    environment: &BuildEnvironment,
    target: &str,
    bundle: &str,
    sign: bool,
) -> Vec<CommandSpec> {
    let mut tauri = vec![
        environment.tauri_cli.to_str().expect("UTF-8 CLI"),
        "build",
        "--ci",
        "--target",
        target,
        "--bundles",
        bundle,
    ];
    if !sign {
        tauri.extend(["--config", "tauri.unsigned.conf.json"]);
    }
    // macOS and Linux check Qt IFW once the target is known.
    let ifw = (!target.ends_with("windows-msvc")).then(|| ifw_version_command(environment));
    ifw.into_iter()
        .chain([
            command(
                environment,
                "selected-node",
                &["--version"],
                OutputMode::Capture,
            ),
            CommandSpec {
                program: gateway(environment, target),
                args: strings(&["--version"]),
                current_dir: environment.workspace_root.clone(),
                envs: Vec::new(),
                output_mode: OutputMode::Capture,
            },
            CommandSpec {
                current_dir: environment
                    .workspace_root
                    .join("crates")
                    .join("workshop")
                    .join("desktop"),
                envs: target_root_env(environment),
                ..command(environment, "selected-node", &tauri, OutputMode::Inherit)
            },
        ])
        .collect()
}

pub(super) fn request(target: &str, sign: bool) -> InstallerRequest {
    InstallerRequest {
        target: Some(target.to_owned()),
        sign,
        gateway_binary: None,
    }
}

#[test]
fn windows_unsigned_builds_the_setup_on_the_unsigned_config() {
    let mut test_environment = environment();
    // Windows never looks for Qt IFW.
    test_environment.environment.qtifw_dir = None;
    let environment = &test_environment.environment;
    let target = "x86_64-pc-windows-msvc";
    let staged = test_environment.stage_gateway(target);
    let setup = bundle(environment, target, "nsis").join(format!("Runvil_{VERSION}_x64-setup.exe"));
    // A setup left by an earlier build at another version must not make
    // the collection ambiguous.
    write_file(
        &bundle(environment, target, "nsis").join("Runvil_0.0.1_x64-setup.exe"),
        b"stale",
    );
    let mut runner = FakeRunner::with_responses(vec![
        node_found(),
        version_printed(),
        bundled(staged.clone(), vec![setup], success("")),
    ]);

    build_installer(&request(target, false), environment, &mut runner).expect("installer");

    assert_eq!(
        runner.commands,
        expected_commands(environment, target, "nsis", false)
    );
    let publish = output(environment, target).join("publish");
    assert!(
        publish
            .join(format!("Runvil_{VERSION}_x64-setup.exe"))
            .is_file()
    );
    assert_eq!(std::fs::read_dir(&publish).expect("publish").count(), 1);
    assert!(!output(environment, target).join("payload").exists());
    assert!(staged.is_file(), "a staged Gateway must outlive the build");
}

#[test]
fn windows_signed_publishes_the_setup_and_its_signature() {
    let mut test_environment = environment();
    set_signing_key(&mut test_environment.environment);
    let environment = &test_environment.environment;
    let target = "x86_64-pc-windows-msvc";
    let staged = test_environment.stage_gateway(target);
    let setup = bundle(environment, target, "nsis").join(format!("Runvil_{VERSION}_x64-setup.exe"));
    let mut runner = FakeRunner::with_responses(vec![
        node_found(),
        version_printed(),
        bundled(staged, vec![collect_signature(&setup), setup], success("")),
    ]);

    build_installer(&request(target, true), environment, &mut runner).expect("installer");

    assert_eq!(
        runner.commands,
        expected_commands(environment, target, "nsis", true)
    );
    let publish = output(environment, target).join("publish");
    let setup = publish.join(format!("Runvil_{VERSION}_x64-setup.exe"));
    assert!(setup.is_file());
    assert!(collect_signature(&setup).is_file());
    assert_eq!(std::fs::read_dir(&publish).expect("publish").count(), 2);
}

#[test]
fn linux_unsigned_from_the_host_publishes_only_the_installer() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-unknown-linux-gnu";
    let staged = test_environment.stage_gateway(target);
    let appimage = bundle(environment, target, "appimage");
    let mut responses = vec![
        success(&format!("cargo 1.89.0\nhost: {target}\n")),
        ifw_found(),
        node_found(),
        version_printed(),
        bundled(
            staged.clone(),
            vec![
                appimage.join(format!("Runvil_{VERSION}_amd64.AppImage")),
                appimage.join("Runvil.AppDir").join("AppRun"),
            ],
            success(""),
        ),
    ];
    responses.extend(ifw_built(environment, target));
    let mut runner = FakeRunner::with_responses(responses);

    build_installer(
        &InstallerRequest {
            target: None,
            sign: false,
            gateway_binary: None,
        },
        environment,
        &mut runner,
    )
    .expect("installer");

    let mut expected = vec![command(
        environment,
        "selected-cargo",
        &["-vV"],
        OutputMode::Capture,
    )];
    expected.extend(expected_commands(environment, target, "appimage", false));
    expected.extend(ifw_commands(environment, target));
    assert_eq!(runner.commands, expected);
    let output_root = output(environment, target);
    assert!(
        output_root
            .join("payload")
            .join("Runvil.AppImage")
            .is_file()
    );
    assert_eq!(
        std::fs::read(output_root.join("payload").join("runvil-gateway")).expect("payload"),
        b"gateway"
    );
    let installer = installer_path(environment, target);
    assert_eq!(
        installer.file_name().expect("name"),
        format!("Runvil_{VERSION}_amd64-setup.run").as_str()
    );
    assert!(installer.is_file());
    assert_eq!(
        std::fs::read_dir(output_root.join("publish"))
            .expect("publish")
            .count(),
        1
    );
    assert!(staged.is_file(), "a staged Gateway must outlive the build");
}

#[test]
fn linux_signed_publishes_the_appimage_and_its_signature_beside_the_installer() {
    let mut test_environment = environment();
    set_signing_key(&mut test_environment.environment);
    let environment = &test_environment.environment;
    let target = "aarch64-unknown-linux-gnu";
    let staged = test_environment.stage_gateway(target);
    let appimage =
        bundle(environment, target, "appimage").join(format!("Runvil_{VERSION}_aarch64.AppImage"));
    let mut responses = vec![
        ifw_found(),
        node_found(),
        version_printed(),
        bundled(
            staged,
            vec![collect_signature(&appimage), appimage],
            success(""),
        ),
    ];
    responses.extend(ifw_built(environment, target));
    let mut runner = FakeRunner::with_responses(responses);

    build_installer(&request(target, true), environment, &mut runner).expect("installer");

    let mut expected = expected_commands(environment, target, "appimage", true);
    expected.extend(ifw_commands(environment, target));
    assert_eq!(runner.commands, expected);
    assert!(installer_path(environment, target).is_file());
    let publish = output(environment, target).join("publish");
    let published = publish.join(format!("Runvil_{VERSION}_aarch64.AppImage"));
    assert!(published.is_file());
    assert!(collect_signature(&published).is_file());
    assert_eq!(std::fs::read_dir(&publish).expect("publish").count(), 3);
    assert!(
        output(environment, target)
            .join("payload")
            .join("Runvil.AppImage")
            .is_file()
    );
}

#[test]
fn preflight_failures_run_no_command() {
    let mut test_environment = environment();
    let target = "x86_64-unknown-linux-gnu";
    test_environment.stage_gateway(target);
    let mut runner = FakeRunner::default();
    let mut refusal = |environment: &BuildEnvironment, case: &str| {
        build_installer(&request(target, true), environment, &mut runner)
            .expect_err(case)
            .primary
    };

    let error = refusal(&test_environment.environment, "no signing key");
    assert!(error.contains("TAURI_SIGNING_PRIVATE_KEY set"), "{error}");

    set_signing_key(&mut test_environment.environment);
    std::fs::remove_file(&test_environment.environment.tauri_cli).expect("remove CLI");
    let error = refusal(&test_environment.environment, "no CLI");
    assert!(error.contains("npm ci --prefix crates/workshop"), "{error}");
    assert!(runner.commands.is_empty());
}

pub(super) fn collect_signature(path: &Path) -> PathBuf {
    crate::installer::collect::signature_path(path)
}
