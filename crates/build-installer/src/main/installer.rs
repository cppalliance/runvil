//! `cargo installer`: gets Workshop's Tauri bundle for one target from
//! `cargo workshop bundle`, run in the Workshop checkout, and collects each
//! platform's installer payload and, under `--sign`, Workshop's updater
//! files into `target/installer/<triple>/`. Windows ends with Tauri's NSIS
//! setup, which holds the Gateway. macOS and Linux end with a Qt IFW
//! installer built from the payload, whose Gateway is the release the
//! checkout's `gateway-pin/pin.json` names, staged there by
//! `tools/fetch-gateway-sidecar.mjs` or named by `--gateway-binary`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::args::InstallerRequest;
use super::pipeline::{resolve_target, run_checked};
use super::{BuildEnvironment, CommandRunner, CommandSpec, OutputMode, gateway, workshop};

// This file is loaded through a path attribute, so its children need one
// too to resolve under `installer/`.
#[path = "installer/bundle.rs"]
pub(super) mod bundle;
#[path = "installer/collect.rs"]
pub(super) mod collect;
#[path = "installer/ifw.rs"]
pub(super) mod ifw;

pub(super) const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The bundle step's name in diagnostics.
const BUNDLE_STEP: &str = "Workshop bundle";

/// The Gateway release a macOS or Linux installer installs: its
/// executable and the version the Workshop checkout's
/// `gateway-pin/pin.json` names.
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

    /// The name of Tauri's bundle directory for the platform, under
    /// `release/bundle/` in Workshop's target directory.
    fn bundle_directory(self) -> &'static str {
        match self.system {
            System::Windows => "nsis",
            System::MacOs => "macos",
            System::Linux => "appimage",
        }
    }
}

pub(super) fn build_installer(
    request: &InstallerRequest,
    environment: &BuildEnvironment,
    runner: &mut impl CommandRunner,
) -> Result<(), String> {
    check_signing_key(request, environment)?;
    workshop::check(environment)?;
    let target = resolve_target(request.target.as_deref(), environment, runner)?;
    let platform = Platform::from_triple(&target)?;
    let named_gateway = gateway::named(
        request
            .gateway_binary
            .as_deref()
            .or(environment.gateway_sidecar.as_deref()),
    )?;
    // The Windows setup already holds the Gateway. A macOS or Linux
    // installer installs it beside Workshop, packaged by Qt IFW.
    let packaging = match platform.system {
        System::Windows => None,
        System::MacOs | System::Linux => {
            let gateway = Gateway {
                binary: gateway::payload_binary(
                    named_gateway.as_deref(),
                    &environment.workshop_root,
                    &target,
                )?,
                version: gateway::pinned_version(&environment.workshop_root)?,
            };
            Some((gateway, check_ifw(environment, runner)?))
        }
    };
    bundle_workshop(
        runner,
        environment,
        &target,
        request.sign,
        named_gateway.as_deref(),
    )?;
    if let Some((gateway, _)) = &packaging {
        check_gateway_version(runner, environment, gateway)?;
    }
    let release = environment
        .workshop_target_root()
        .join(&target)
        .join("release");
    let output = environment.target_root.join("installer").join(&target);
    collect::collect(&release, platform, request.sign, &output)?;
    let installer = match &packaging {
        Some((gateway, ifw)) => {
            build_ifw_installer(ifw, environment, platform, gateway, &output, runner)?
        }
        None => output
            .join("publish")
            .join(collect::installer_name(platform)),
    };
    println!("installer: {}", installer.display());
    Ok(())
}

fn check_signing_key(
    request: &InstallerRequest,
    environment: &BuildEnvironment,
) -> Result<(), String> {
    if request.sign && environment.signing_key.is_none() {
        return Err(
            "`--sign` needs TAURI_SIGNING_PRIVATE_KEY set to the release minisign key; it is \
             unset or empty"
                .to_owned(),
        );
    }
    Ok(())
}

fn check_ifw(
    environment: &BuildEnvironment,
    runner: &mut impl CommandRunner,
) -> Result<ifw::Ifw, String> {
    let cwd = std::env::current_dir()
        .map_err(|error| format!("cannot read the current directory: {error}"))?;
    let ifw = ifw::locate(
        environment.qtifw_dir.as_deref(),
        environment.path.as_deref(),
        &cwd,
    )?;
    let printed = run_checked(
        runner,
        &ifw::version_command(&ifw, &environment.workspace_root),
        &format!(
            "Qt IFW version check (`{} --version`)",
            ifw.installerbase.display()
        ),
    )?;
    ifw::check_version(&ifw, &printed)?;
    Ok(ifw)
}

/// Runs `cargo workshop bundle` in the Workshop checkout, which builds
/// Workshop and leaves Tauri's bundle in Workshop's target directory.
fn bundle_workshop(
    runner: &mut impl CommandRunner,
    environment: &BuildEnvironment,
    target: &str,
    sign: bool,
    named_gateway: Option<&Path>,
) -> Result<(), String> {
    run_checked(
        runner,
        &workshop::bundle_command(environment, target, sign, named_gateway),
        BUNDLE_STEP,
    )?;
    // The bundle is the last command of a Windows build, so no later
    // command would refuse to start after a cancel that arrived as this
    // one completed.
    if runner.cancel_observed() {
        return Err(format!("{BUNDLE_STEP} canceled after child completion"));
    }
    Ok(())
}

/// Runs the Gateway's `--version` and requires the version the pin names,
/// so an installer never ships a Gateway other than the pinned release.
fn check_gateway_version(
    runner: &mut impl CommandRunner,
    environment: &BuildEnvironment,
    gateway: &Gateway,
) -> Result<(), String> {
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
        Err(format!(
            "Gateway version check failed: {} printed `{}`, expected `{expected}`, the \
             version the Workshop checkout's gateway-pin/pin.json names",
            gateway.binary.display(),
            printed.trim()
        ))
    }
}

/// Places `gateway` in `output`'s payload, stages the IFW definition
/// around the payload, and builds the installer into `output/publish/`,
/// returning its path.
fn build_ifw_installer(
    ifw: &ifw::Ifw,
    environment: &BuildEnvironment,
    platform: Platform,
    gateway: &Gateway,
    output: &Path,
    runner: &mut impl CommandRunner,
) -> Result<PathBuf, String> {
    let payload = output.join("payload");
    let stage = output.join("ifw");
    let publish = output.join("publish");
    collect::place_gateway(
        &bundle::BundleSources::from_environment(environment),
        gateway,
        platform,
        &payload,
    )?;
    ifw::stage(
        &ifw::Sources::from_environment(environment),
        &payload,
        platform,
        &environment.release_date,
        &gateway.version,
        &stage,
    )?;
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

fn require_output(step: &str, path: &Path) -> Result<(), String> {
    if path.exists() {
        Ok(())
    } else {
        Err(format!(
            "{step} succeeded but wrote nothing at {}",
            path.display()
        ))
    }
}
