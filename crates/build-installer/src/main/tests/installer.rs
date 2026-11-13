//! Unit tests for the installer command sequence on Windows and Linux,
//! with the helpers every sequence test shares. The macOS sequences are
//! in `bundle.rs`.

use super::ifw_tests::{ifw_built, ifw_commands, ifw_found, ifw_version_command, installer_path};
use super::*;

pub(super) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Tauri's bundle directory `directory` for `target`, in the target
/// directory `cargo workshop bundle` writes to.
pub(super) fn bundle(environment: &BuildEnvironment, target: &str, directory: &str) -> PathBuf {
    environment
        .workshop_target_root()
        .join(target)
        .join("release")
        .join("bundle")
        .join(directory)
}

pub(super) fn output(environment: &BuildEnvironment, target: &str) -> PathBuf {
    environment.target_root.join("installer").join(target)
}

pub(super) fn version_printed() -> FakeResponse {
    success(&format!("runvil-gateway {GATEWAY_VERSION}\n"))
}

/// A `cargo workshop bundle` run that writes `files`, as Tauri leaves its
/// bundle.
pub(super) fn bundled(files: Vec<PathBuf>, response: FakeResponse) -> FakeResponse {
    act(
        move || {
            for file in files {
                write_file(&file, b"bundle");
            }
        },
        response,
    )
}

/// The bundle step: Cargo's `workshop bundle`, run in the Workshop
/// checkout with the absolute target root. `gateway` is the Gateway a
/// flag or the variable named.
pub(super) fn bundle_command(
    environment: &BuildEnvironment,
    target: &str,
    sign: bool,
    gateway: Option<&Path>,
) -> CommandSpec {
    let mut args = strings(&["workshop", "bundle", "--target", target]);
    if sign {
        args.push(OsString::from("--sign"));
    }
    if let Some(gateway) = gateway {
        args.push(OsString::from("--gateway-binary"));
        args.push(gateway.as_os_str().to_owned());
    }
    CommandSpec {
        program: PathBuf::from("selected-cargo"),
        args,
        current_dir: environment.workshop_root.clone(),
        envs: target_root_env(environment),
        output_mode: OutputMode::Inherit,
    }
}

/// The commands of a build for `target` up to the collection. `named` is
/// the Gateway a flag or the variable named; without one, a macOS or
/// Linux payload takes the staged Gateway.
pub(super) fn expected_commands(
    environment: &BuildEnvironment,
    target: &str,
    sign: bool,
    named: Option<&Path>,
) -> Vec<CommandSpec> {
    let bundle = bundle_command(environment, target, sign, named);
    if target.ends_with("windows-msvc") {
        return vec![bundle];
    }
    // macOS and Linux check Qt IFW once the target is known, and the
    // payload's Gateway once the bundle is built.
    let gateway = named.map_or_else(
        || gateway::staged_path(&environment.workshop_root, target),
        Path::to_path_buf,
    );
    vec![
        ifw_version_command(environment),
        bundle,
        CommandSpec {
            program: gateway,
            args: strings(&["--version"]),
            current_dir: environment.workspace_root.clone(),
            envs: Vec::new(),
            output_mode: OutputMode::Capture,
        },
    ]
}

pub(super) fn request(target: &str, sign: bool) -> InstallerRequest {
    InstallerRequest {
        target: Some(target.to_owned()),
        sign,
        gateway_binary: None,
    }
}

pub(super) fn collect_signature(path: &Path) -> PathBuf {
    crate::installer::collect::signature_path(path)
}

#[test]
fn windows_unsigned_publishes_only_the_setup_and_needs_no_gateway_or_ifw() {
    let mut test_environment = environment();
    // The setup already holds the Gateway, so a Windows installer reads
    // neither Qt IFW, nor a staged Gateway, nor the pin.
    test_environment.environment.qtifw_dir = None;
    let environment = &test_environment.environment;
    std::fs::remove_dir_all(environment.workshop_root.join("gateway-pin")).expect("remove pin");
    let target = "x86_64-pc-windows-msvc";
    let setup = bundle(environment, target, "nsis").join(format!("Runvil_{VERSION}_x64-setup.exe"));
    let mut runner = FakeRunner::with_responses(vec![bundled(vec![setup], success(""))]);

    build_installer(&request(target, false), environment, &mut runner).expect("installer");

    assert_eq!(
        runner.commands,
        vec![CommandSpec {
            program: PathBuf::from("selected-cargo"),
            args: strings(&["workshop", "bundle", "--target", target]),
            current_dir: environment.workshop_root.clone(),
            envs: target_root_env(environment),
            output_mode: OutputMode::Inherit,
        }]
    );
    let publish = output(environment, target).join("publish");
    assert!(
        publish
            .join(format!("Runvil_{VERSION}_x64-setup.exe"))
            .is_file()
    );
    assert_eq!(std::fs::read_dir(&publish).expect("publish").count(), 1);
    assert!(!output(environment, target).join("payload").exists());
}

#[test]
fn windows_signed_publishes_the_setup_and_its_signature() {
    let mut test_environment = environment();
    set_signing_key(&mut test_environment.environment);
    let environment = &test_environment.environment;
    let target = "x86_64-pc-windows-msvc";
    let setup = bundle(environment, target, "nsis").join(format!("Runvil_{VERSION}_x64-setup.exe"));
    let mut runner = FakeRunner::with_responses(vec![bundled(
        vec![collect_signature(&setup), setup],
        success(""),
    )]);

    build_installer(&request(target, true), environment, &mut runner).expect("installer");

    assert_eq!(
        runner.commands,
        vec![CommandSpec {
            program: PathBuf::from("selected-cargo"),
            args: strings(&["workshop", "bundle", "--target", target, "--sign"]),
            current_dir: environment.workshop_root.clone(),
            envs: target_root_env(environment),
            output_mode: OutputMode::Inherit,
        }]
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
        bundled(
            vec![
                appimage.join(format!("Runvil_{VERSION}_amd64.AppImage")),
                appimage.join("Runvil.AppDir").join("AppRun"),
            ],
            success(""),
        ),
        version_printed(),
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
    expected.extend(expected_commands(environment, target, false, None));
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
    // The Gateway package states the pin's version, not Workshop's.
    let package = output_root
        .join("ifw")
        .join("packages")
        .join("com.runvil.gateway")
        .join("meta")
        .join("package.xml");
    let package = std::fs::read_to_string(package).expect("Gateway package");
    assert!(
        package.contains(&format!("<Version>{GATEWAY_VERSION}</Version>")),
        "{package}"
    );
}

#[test]
fn linux_signed_publishes_the_appimage_and_its_signature_beside_the_installer() {
    let mut test_environment = environment();
    set_signing_key(&mut test_environment.environment);
    let environment = &test_environment.environment;
    let target = "aarch64-unknown-linux-gnu";
    test_environment.stage_gateway(target);
    let appimage =
        bundle(environment, target, "appimage").join(format!("Runvil_{VERSION}_aarch64.AppImage"));
    let mut responses = vec![
        ifw_found(),
        bundled(vec![collect_signature(&appimage), appimage], success("")),
        version_printed(),
    ];
    responses.extend(ifw_built(environment, target));
    let mut runner = FakeRunner::with_responses(responses);

    build_installer(&request(target, true), environment, &mut runner).expect("installer");

    let mut expected = expected_commands(environment, target, true, None);
    expected.extend(ifw_commands(environment, target));
    assert_eq!(runner.commands, expected);
    assert_eq!(
        runner.commands[1].args,
        strings(&["workshop", "bundle", "--target", target, "--sign"])
    );
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

/// Without `CARGO_TARGET_DIR`, the Cargo that builds Workshop runs in the
/// Workshop checkout and writes to that checkout's `target`, while the
/// installer output stays in this workspace's.
#[test]
fn a_target_root_from_cargo_configuration_is_left_to_cargo() {
    let mut test_environment = environment();
    test_environment.environment.target_root_from_env = false;
    let environment = &test_environment.environment;
    let target = "x86_64-pc-windows-msvc";
    let setup = environment
        .workshop_root
        .join("target")
        .join(target)
        .join("release")
        .join("bundle")
        .join("nsis")
        .join(format!("Runvil_{VERSION}_x64-setup.exe"));
    let mut runner = FakeRunner::with_responses(vec![bundled(vec![setup], success(""))]);

    build_installer(&request(target, false), environment, &mut runner).expect("installer");

    assert_eq!(runner.commands.len(), 1);
    assert!(
        runner
            .commands
            .iter()
            .all(|command| command.envs.is_empty())
    );
    assert!(
        environment
            .target_root
            .join("installer")
            .join(target)
            .join("publish")
            .join(format!("Runvil_{VERSION}_x64-setup.exe"))
            .is_file()
    );
}

#[test]
fn signing_without_the_key_fails_first_and_runs_no_command() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    // The key is checked before the Workshop checkout, which is missing
    // here.
    std::fs::remove_dir_all(&environment.workshop_root).expect("remove the checkout");
    let mut runner = FakeRunner::default();

    let error = build_installer(
        &request("x86_64-unknown-linux-gnu", true),
        environment,
        &mut runner,
    )
    .expect_err("no signing key");

    assert!(error.contains("TAURI_SIGNING_PRIVATE_KEY set"), "{error}");
    assert!(runner.commands.is_empty());
}
