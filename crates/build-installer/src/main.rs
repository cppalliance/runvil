//! Builds each platform's installer around Workshop's Tauri bundle and the
//! pinned Gateway release. Workshop is its own product in
//! `cppalliance/runvil-workshop`, so nothing here builds it: the bundle
//! comes from `cargo workshop bundle`, run in the checkout
//! `RUNVIL_WORKSHOP` names. For Windows targets that bundle is the
//! installer, Tauri's NSIS setup with the Gateway inside. macOS and Linux
//! payloads hold the Gateway beside Workshop, and Qt IFW packages them into
//! one installer. The Gateway is its own product in
//! `cppalliance/runvil-gateway`, so nothing here compiles it. The
//! `manifest` mode verifies every platform's published files as one release
//! and writes its `latest.json` and `SHA256SUMS`.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::{Child, ExitCode};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "main/args.rs"]
mod args;
#[path = "main/gateway.rs"]
mod gateway;
#[path = "main/installer.rs"]
mod installer;
#[path = "main/manifest.rs"]
mod manifest;
#[path = "main/pipeline.rs"]
mod pipeline;
#[path = "main/runner.rs"]
mod runner;
#[path = "main/workshop.rs"]
mod workshop;

use args::{InstallerRequest, ManifestRequest, Request, parse_arguments};
use installer::build_installer;
use runner::install_cancel_handler;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputMode {
    Capture,
    Inherit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CommandSpec {
    program: PathBuf,
    args: Vec<OsString>,
    current_dir: PathBuf,
    envs: Vec<(OsString, OsString)>,
    output_mode: OutputMode,
}

#[derive(Debug, Eq, PartialEq)]
struct CommandResult {
    success: bool,
    stdout: String,
    stderr: String,
}

trait CommandRunner {
    /// Refuses to start once a cancel has been requested.
    fn run(&mut self, command: &CommandSpec) -> io::Result<CommandResult>;

    fn cancel_observed(&self) -> bool;
}

#[derive(Debug)]
struct ProcessRunner {
    cancel: CancelController,
}

#[derive(Clone, Debug)]
struct CancelController {
    state: Arc<CancelState>,
}

#[derive(Debug)]
struct CancelState {
    generation: AtomicUsize,
    active_child: Mutex<Option<Child>>,
    termination_error: Mutex<Option<String>>,
}

#[derive(Debug)]
struct BuildEnvironment {
    /// This repository, which holds the IFW definition, the Gateway bundle
    /// template, and `LICENSE`.
    workspace_root: PathBuf,
    /// The runvil-workshop checkout `RUNVIL_WORKSHOP` names. Its
    /// `cargo workshop bundle` builds Workshop's bundle, and it holds the
    /// icons, the Gateway pin, and the staged Gateway.
    workshop_root: PathBuf,
    /// The installer output goes to `installer/<triple>/` in this
    /// directory.
    target_root: PathBuf,
    /// Whether `CARGO_TARGET_DIR` chose `target_root`; otherwise Cargo's
    /// own configuration does, and children are left to it.
    target_root_from_env: bool,
    cargo: PathBuf,
    signing_key: Option<OsString>,
    /// The Gateway binary `RUNVIL_GATEWAY_SIDECAR` names, when set.
    gateway_sidecar: Option<PathBuf>,
    /// `QTIFWDIR` and `PATH`, where the macOS and Linux installers find
    /// Qt IFW.
    qtifw_dir: Option<OsString>,
    path: Option<OsString>,
    /// Today's UTC date, `YYYY-MM-DD`, for the IFW packages' `ReleaseDate`.
    release_date: String,
}

impl BuildEnvironment {
    fn discover() -> Result<Self, anyhow::Error> {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir
            .parent()
            .and_then(|crates| crates.parent())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot derive the workspace root from {}",
                    manifest_dir.display()
                )
            })?
            .to_path_buf();
        let workshop_root = match std::env::var_os(workshop::WORKSHOP_VARIABLE) {
            Some(value) if !value.is_empty() => std::path::absolute(&value).map_err(|error| {
                anyhow::anyhow!(
                    "cannot resolve the Workshop checkout {}: {error}",
                    PathBuf::from(&value).display()
                )
            })?,
            _ => {
                return Err(anyhow::anyhow!(
                    "{} is unset or empty; set it to the root of a runvil-workshop checkout",
                    workshop::WORKSHOP_VARIABLE
                ));
            }
        };
        let configured_target = std::env::var_os("CARGO_TARGET_DIR");
        let target_root_from_env = configured_target.is_some();
        let target_root = match configured_target {
            Some(value) if value.is_empty() => {
                return Err(anyhow::anyhow!("CARGO_TARGET_DIR must not be empty"));
            }
            Some(value) => {
                let path = PathBuf::from(value);
                if path.is_absolute() {
                    path
                } else {
                    std::env::current_dir()
                        .map_err(|error| {
                            anyhow::anyhow!("cannot read the current directory: {error}")
                        })?
                        .join(path)
                }
            }
            None => workspace_root.join("target"),
        };
        let cargo = std::env::var_os("CARGO")
            .or_else(|| option_env!("CARGO").map(OsString::from))
            .map(PathBuf::from)
            .ok_or_else(|| {
                anyhow::anyhow!("Cargo did not provide the executable used for this command")
            })?;
        let signing_key =
            std::env::var_os("TAURI_SIGNING_PRIVATE_KEY").filter(|key| !key.is_empty());
        let gateway_sidecar = match std::env::var_os(gateway::GATEWAY_SIDECAR_VARIABLE) {
            Some(value) if value.is_empty() => {
                return Err(anyhow::anyhow!(
                    "{} must not be empty",
                    gateway::GATEWAY_SIDECAR_VARIABLE
                ));
            }
            Some(value) => Some(PathBuf::from(value)),
            None => None,
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| anyhow::anyhow!("the system clock is before 1970: {error}"))?;
        Ok(Self {
            workspace_root,
            workshop_root,
            target_root,
            target_root_from_env,
            cargo,
            signing_key,
            gateway_sidecar,
            qtifw_dir: std::env::var_os("QTIFWDIR"),
            path: std::env::var_os("PATH"),
            release_date: installer::ifw::utc_date(now.as_secs()),
        })
    }

    /// The target directory `cargo workshop bundle` writes to. That Cargo
    /// runs in the Workshop checkout, so without `CARGO_TARGET_DIR` it
    /// writes to the checkout's own `target`.
    fn workshop_target_root(&self) -> PathBuf {
        if self.target_root_from_env {
            self.target_root.clone()
        } else {
            self.workshop_root.join("target")
        }
    }

    /// Cargo resolves a relative `CARGO_TARGET_DIR` against its own working
    /// directory, and the bundle step runs Cargo from the Workshop
    /// checkout, so a child that runs Cargo gets the absolute root this
    /// process reads its outputs from.
    fn cargo_envs(&self) -> Vec<(OsString, OsString)> {
        if !self.target_root_from_env {
            return Vec::new();
        }
        vec![(
            OsString::from("CARGO_TARGET_DIR"),
            self.target_root.clone().into_os_string(),
        )]
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .iter()
        .any(|argument| argument == "-h" || argument == "--help")
    {
        print!("{}", args::USAGE);
        return ExitCode::SUCCESS;
    }
    let request = match parse_arguments(&args) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let environment = match BuildEnvironment::discover() {
        Ok(environment) => environment,
        Err(error) => {
            eprintln!("build-installer failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = match &request {
        Request::Installer(request) => run_installer(request, &environment),
        Request::Manifest(request) => run_manifest(request, &environment),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("build-installer failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_installer(request: &InstallerRequest, environment: &BuildEnvironment) -> Result<(), String> {
    let cancel = install_cancel_handler().map_err(|error| error.to_string())?;
    build_installer(request, environment, &mut ProcessRunner::new(cancel))
}

fn run_manifest(request: &ManifestRequest, environment: &BuildEnvironment) -> Result<(), String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("the system clock is before 1970: {error}"))?;
    manifest::write(request, &environment.workshop_root, now.as_secs())?;
    println!(
        "wrote {} and {} in {}",
        manifest::MANIFEST,
        manifest::CHECKSUMS,
        request.assets.display()
    );
    Ok(())
}

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
