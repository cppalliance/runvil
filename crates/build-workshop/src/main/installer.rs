//! `cargo workshop installer`: builds Workshop for one target in release,
//! bundles it with the pinned Tauri CLI, and collects each platform's
//! installer payload and, under `--sign`, Workshop's updater files into
//! `target/installer/<triple>/`. The payload's Gateway is the release
//! `gateway-pin/pin.json` names, staged by `tools/fetch-gateway-sidecar.mjs`
//! or named by `--gateway-binary`. Windows ends with Tauri's NSIS setup;
//! macOS and Linux end with a Qt IFW installer built from the payload.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::args::InstallerRequest;
use super::pipeline::{resolve_target, run_checked, with_staged_sidecar};
use super::sidecar::{self, GatewaySource};
use super::{BuildEnvironment, BuildError, CommandRunner, CommandSpec, OutputMode, StepError};

// This file is loaded through a path attribute, so its children need one
// too to resolve under `installer/`.
#[path = "installer/bundle.rs"]
pub(super) mod bundle;
#[path = "installer/collect.rs"]
pub(super) mod collect;
#[path = "installer/ifw.rs"]
pub(super) mod ifw;

pub(super) const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Applied to unsigned builds: it turns off `createUpdaterArtifacts`, which
/// would otherwise demand the release key.
const UNSIGNED_CONFIG: &str = "tauri.unsigned.conf.json";
/// The Tauri CLI first runs after the Gateway version check, so a missing
/// `node` is found up front instead.
const NODE_CHECK: &str = "`node --version` (the Tauri CLI runs on `node` from PATH)";

/// The Gateway release an installer carries: its executable and the
/// version `gateway-pin/pin.json` names.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct Gateway {
    pub(super) binary: PathBuf,
    pub(super) version: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum System {
    Windows,
    MacOs,
    Linux,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Arch {
    X86_64,
    Aarch64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Platform {
    pub(super) system: System,
    pub(super) arch: Arch,
}

impl Platform {
    pub(super) fn from_triple(target: &str) -> Result<Self, String> {
        let parts: Vec<&str> = target.split('-').collect();
        let arch = match parts.first() {
            Some(&"x86_64") => Some(Arch::X86_64),
            Some(&"aarch64") => Some(Arch::Aarch64),
            _ => None,
        };
        let system = match parts.get(1..) {
            Some(["pc", "windows", "msvc"]) => Some(System::Windows),
            Some(["apple", "darwin"]) => Some(System::MacOs),
            Some(["unknown", "linux", "gnu"]) => Some(System::Linux),
            _ => None,
        };
        match (system, arch) {
            (Some(system), Some(arch)) => Ok(Self { system, arch }),
            _ => Err(format!(
                "installer target `{target}` is not supported; supported: x86_64 or aarch64 \
                 with pc-windows-msvc, apple-darwin, or unknown-linux-gnu"
            )),
        }
    }

    /// The Tauri bundle format, which is also its output directory's name
    /// except for `app`.
    pub(super) fn bundle(self) -> &'static str {
        match self.system {
            System::Windows => "nsis",
            System::MacOs => "app",
            System::Linux => "appimage",
        }
    }

    fn bundle_directory(self) -> &'static str {
        match self.system {
            System::MacOs => "macos",
            System::Windows | System::Linux => self.bundle(),
        }
    }
}

pub(super) fn build_installer(
    request: &InstallerRequest,
    environment: &BuildEnvironment,
    runner: &mut impl CommandRunner,
) -> Result<(), BuildError> {
    preflight(request, environment)?;
    let target = resolve_target(request.target.as_deref(), environment, runner)?;
    let platform = Platform::from_triple(&target).map_err(failure)?;
    let given = request
        .gateway_binary
        .as_deref()
        .or(environment.gateway_sidecar.as_deref());
    let source =
        sidecar::gateway_source(given, &environment.workspace_root, &target).map_err(failure)?;
    if source == (GatewaySource::Staged { stub: true }) {
        return Err(failure(format!(
            "the Gateway staged for {target} at {} is a compile-only stub, which an installer \
             cannot ship; run `node tools/fetch-gateway-sidecar.mjs --target {target}` to stage \
             the release gateway-pin/pin.json names, or pass --gateway-binary",
            sidecar::sidecar_path(&environment.workspace_root, &target).display()
        )));
    }
    let gateway = Gateway {
        binary: source.binary(&environment.workspace_root, &target),
        version: sidecar::pinned_version(&environment.workspace_root).map_err(failure)?,
    };
    let ifw = match platform.system {
        System::Windows => None,
        System::MacOs | System::Linux => Some(check_ifw(environment, runner)?),
    };
    run_checked(runner, &node_check_command(environment), NODE_CHECK)?;
    let release = environment.target_root.join(&target).join("release");
    // A bundle left by an earlier build, possibly at another version, would
    // make the collection ambiguous.
    remove_directory(&release.join("bundle").join(platform.bundle_directory()))?;
    check_gateway_version(runner, environment, &gateway)?;
    with_staged_sidecar(
        environment,
        &target,
        runner,
        "Workshop bundle",
        &source,
        |runner| {
            run_checked(
                runner,
                &tauri_build_command(environment, &target, platform, request.sign),
                "Workshop bundle",
            )
            .map(|_| ())
        },
    )?;
    let output = environment.target_root.join("installer").join(&target);
    collect::collect(
        &environment.workspace_root,
        &release,
        &gateway,
        platform,
        request.sign,
        &output,
    )
    .map_err(failure)?;
    let installer = match &ifw {
        Some(ifw) => build_ifw_installer(ifw, environment, platform, &output, runner)?,
        None => output
            .join("publish")
            .join(collect::installer_name(platform)),
    };
    println!("installer: {}", installer.display());
    Ok(())
}

fn check_ifw(
    environment: &BuildEnvironment,
    runner: &mut impl CommandRunner,
) -> Result<ifw::Ifw, BuildError> {
    let cwd = std::env::current_dir()
        .map_err(|error| failure(format!("cannot read the current directory: {error}")))?;
    let ifw = ifw::locate(
        environment.qtifw_dir.as_deref(),
        environment.path.as_deref(),
        &cwd,
    )
    .map_err(failure)?;
    let printed = run_checked(
        runner,
        &ifw::version_command(&ifw, &environment.workspace_root),
        &format!(
            "Qt IFW version check (`{} --version`)",
            ifw.installerbase.display()
        ),
    )?;
    ifw::check_version(&ifw, &printed).map_err(failure)?;
    Ok(ifw)
}

/// Stages the IFW definition around `output`'s payload and builds the
/// installer into `output/publish/`, returning its path.
fn build_ifw_installer(
    ifw: &ifw::Ifw,
    environment: &BuildEnvironment,
    platform: Platform,
    output: &Path,
    runner: &mut impl CommandRunner,
) -> Result<PathBuf, BuildError> {
    let stage = output.join("ifw");
    let publish = output.join("publish");
    ifw::stage(
        &ifw::Sources::in_workspace(&environment.workspace_root),
        &output.join("payload"),
        platform,
        &environment.release_date,
        &stage,
    )
    .map_err(failure)?;
    let built = ifw::binarycreator_output(platform, &stage, &publish);
    run_checked(
        runner,
        &ifw::binarycreator_command(ifw, &stage, &built),
        "Qt IFW binarycreator",
    )?;
    require_output("Qt IFW binarycreator", &built)?;
    if platform.system != System::MacOs {
        return Ok(built);
    }
    run_checked(
        runner,
        &ifw::adhoc_sign_command(&built),
        "Installer app ad hoc signing (codesign)",
    )?;
    let dmg = ifw::dmg_path(platform, &publish);
    let app_folder = stage.join("dmg");
    run_checked(
        runner,
        &ifw::hdiutil_command(&app_folder, &dmg),
        "DMG wrap (hdiutil)",
    )?;
    require_output("DMG wrap (hdiutil)", &dmg)?;
    Ok(dmg)
}

fn require_output(step: &str, path: &Path) -> Result<(), BuildError> {
    if path.exists() {
        Ok(())
    } else {
        Err(failure(format!(
            "{step} succeeded but wrote nothing at {}",
            path.display()
        )))
    }
}

fn preflight(request: &InstallerRequest, environment: &BuildEnvironment) -> Result<(), BuildError> {
    if !environment.tauri_cli.is_file() {
        return Err(failure(format!(
            "the Tauri CLI is missing at {}; run `npm ci --prefix crates/workshop` first",
            environment.tauri_cli.display()
        )));
    }
    if request.sign && environment.signing_key.is_none() {
        return Err(failure(
            "`--sign` needs TAURI_SIGNING_PRIVATE_KEY set to the release minisign key; it is \
             unset or empty"
                .to_owned(),
        ));
    }
    Ok(())
}

/// Runs the Gateway's `--version` and requires the version the pin names,
/// so an installer never ships a Gateway other than the pinned release.
fn check_gateway_version(
    runner: &mut impl CommandRunner,
    environment: &BuildEnvironment,
    gateway: &Gateway,
) -> Result<(), StepError> {
    let printed = run_checked(
        runner,
        &CommandSpec {
            program: gateway.binary.clone(),
            args: vec![OsString::from("--version")],
            current_dir: environment.workspace_root.clone(),
            envs: Vec::new(),
            output_mode: OutputMode::Capture,
        },
        "Gateway version check",
    )?;
    let expected = format!("runvil-gateway {}", gateway.version);
    if printed.trim() == expected {
        Ok(())
    } else {
        Err(StepError::failed(format!(
            "Gateway version check failed: {} printed `{}`, expected `{expected}`, the \
             version gateway-pin/pin.json names",
            gateway.binary.display(),
            printed.trim()
        )))
    }
}

fn tauri_build_command(
    environment: &BuildEnvironment,
    target: &str,
    platform: Platform,
    sign: bool,
) -> CommandSpec {
    let mut args = vec![environment.tauri_cli.clone().into_os_string()];
    args.extend(
        [
            "build",
            "--ci",
            "--target",
            target,
            "--bundles",
            platform.bundle(),
        ]
        .map(OsString::from),
    );
    if !sign {
        args.extend(["--config", UNSIGNED_CONFIG].map(OsString::from));
    }
    CommandSpec {
        program: environment.node.clone(),
        args,
        current_dir: environment
            .workspace_root
            .join("crates")
            .join("workshop")
            .join("desktop"),
        envs: environment.cargo_envs(),
        output_mode: OutputMode::Inherit,
    }
}

fn node_check_command(environment: &BuildEnvironment) -> CommandSpec {
    CommandSpec {
        program: environment.node.clone(),
        args: vec![OsString::from("--version")],
        current_dir: environment.workspace_root.clone(),
        envs: Vec::new(),
        output_mode: OutputMode::Capture,
    }
}

fn remove_directory(path: &Path) -> Result<(), BuildError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(failure(format!("cannot clear {}: {error}", path.display()))),
    }
}

fn failure(message: String) -> BuildError {
    BuildError {
        primary: message,
        cleanup: None,
    }
}
