//! Builds Workshop and stages its Gateway sidecar for Tauri.
//!
//! The sidecar comes from one of three places, in this order: the
//! `--gateway-binary` flag, the `RUNVIL_GATEWAY_SIDECAR` environment
//! variable, or a sidecar already staged under
//! `crates/workshop/desktop/binaries/`. The Gateway is its own product in
//! `cppalliance/runvil-gateway`, so nothing here compiles it: with no
//! sidecar anywhere, the run stops and names
//! `node tools/fetch-gateway-sidecar.mjs`, which downloads the release
//! `gateway-pin/pin.json` names.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::{Child, ExitCode};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

#[path = "main/pipeline.rs"]
mod pipeline;
#[path = "main/runner.rs"]
mod runner;

use pipeline::build_workshop;
use runner::install_interrupt_handler;

/// The environment variable naming a Gateway binary to stage, so CI can
/// point at a staged file without passing a flag through Tauri.
const GATEWAY_SIDECAR_VARIABLE: &str = "RUNVIL_GATEWAY_SIDECAR";

const USAGE: &str = "\
Build Workshop, staging its Gateway sidecar.

USAGE:
    cargo workshop [--release] [--target <triple>] [--gateway-binary <path>]

OPTIONS:
    --release                 Build Workshop with Cargo's release profile
    --target <triple>         Build for this target triple
    --gateway-binary <path>   Stage this Gateway binary
    -h, --help                Print this help

The Gateway sidecar comes from --gateway-binary, then the
RUNVIL_GATEWAY_SIDECAR environment variable, then a sidecar already staged
under crates/workshop/desktop/binaries/. The Gateway is a separate product,
so this never builds it: run `node tools/fetch-gateway-sidecar.mjs` to stage
the release gateway-pin/pin.json names. A sidecar that was already staged is
left in place, so the next build reuses it.
";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Profile {
    Debug,
    Release,
}

#[derive(Debug, Eq, PartialEq)]
struct BuildRequest {
    profile: Profile,
    target: Option<String>,
    gateway_binary: Option<PathBuf>,
}

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
    env: Vec<(OsString, OsString)>,
    output_mode: OutputMode,
}

#[derive(Debug, Eq, PartialEq)]
struct CommandResult {
    success: bool,
    stdout: String,
    stderr: String,
}

trait CommandRunner {
    fn run(
        &mut self,
        command: &CommandSpec,
        interrupt_policy: InterruptPolicy,
    ) -> io::Result<CommandResult>;

    fn last_command_started(&self) -> bool;

    fn interruption_observed(&self) -> bool;
}

#[derive(Debug)]
struct ProcessRunner {
    interrupt: InterruptController,
    last_command_started: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InterruptPolicy {
    RejectExisting,
    AllowExisting,
}

#[derive(Clone, Debug)]
struct InterruptController {
    state: Arc<InterruptState>,
}

#[derive(Debug)]
struct InterruptState {
    generation: AtomicUsize,
    active_child: Mutex<Option<Child>>,
    termination_error: Mutex<Option<String>>,
}

#[derive(Debug)]
struct BuildEnvironment {
    workspace_root: PathBuf,
    cargo: PathBuf,
    node: PathBuf,
    /// The Gateway binary named by `RUNVIL_GATEWAY_SIDECAR`, when set: the
    /// path a CI job stages and points the build at, so no flag has to pass
    /// through Tauri.
    gateway_sidecar: Option<PathBuf>,
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
        let cargo = std::env::var_os("CARGO")
            .or_else(|| option_env!("CARGO").map(OsString::from))
            .map(PathBuf::from)
            .ok_or_else(|| {
                anyhow::anyhow!("Cargo did not provide the executable used for this command")
            })?;
        let gateway_sidecar = match std::env::var_os(GATEWAY_SIDECAR_VARIABLE) {
            Some(value) if value.is_empty() => {
                return Err(anyhow::anyhow!(
                    "{GATEWAY_SIDECAR_VARIABLE} must not be empty"
                ));
            }
            Some(value) => Some(PathBuf::from(value)),
            None => None,
        };
        Ok(Self {
            workspace_root,
            cargo,
            node: PathBuf::from("node"),
            gateway_sidecar,
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
struct BuildError {
    primary: String,
    cleanup: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct StepError {
    message: String,
    interrupted: bool,
    command_started: bool,
}

impl fmt::Display for BuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.primary)?;
        if let Some(cleanup) = &self.cleanup {
            write!(formatter, "\ncleanup also failed: {cleanup}")?;
        }
        Ok(())
    }
}

fn parse_arguments(args: &[String]) -> Result<BuildRequest, anyhow::Error> {
    let mut profile = Profile::Debug;
    let mut release_seen = false;
    let mut target = None;
    let mut gateway_binary = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--release" if release_seen => {
                return Err(anyhow::anyhow!("duplicate argument `--release`\n\n{USAGE}"));
            }
            "--release" => {
                profile = Profile::Release;
                release_seen = true;
                index += 1;
            }
            "--target" if target.is_some() => {
                return Err(anyhow::anyhow!("duplicate argument `--target`\n\n{USAGE}"));
            }
            "--target" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    anyhow::anyhow!("argument `--target` needs a target triple\n\n{USAGE}")
                })?;
                if value.starts_with('-') || !valid_target_triple(value) {
                    return Err(anyhow::anyhow!(
                        "argument `--target` needs a valid target triple, got `{value}`\n\n{USAGE}"
                    ));
                }
                target = Some(value.clone());
                index += 2;
            }
            "--gateway-binary" if gateway_binary.is_some() => {
                return Err(anyhow::anyhow!(
                    "duplicate argument `--gateway-binary`\n\n{USAGE}"
                ));
            }
            "--gateway-binary" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    anyhow::anyhow!("argument `--gateway-binary` needs a path\n\n{USAGE}")
                })?;
                if value.starts_with('-') || value.trim().is_empty() {
                    return Err(anyhow::anyhow!(
                        "argument `--gateway-binary` needs a path, got `{value}`\n\n{USAGE}"
                    ));
                }
                gateway_binary = Some(PathBuf::from(value));
                index += 2;
            }
            argument => {
                return Err(anyhow::anyhow!(
                    "unsupported argument `{argument}`\n\n{USAGE}"
                ));
            }
        }
    }
    Ok(BuildRequest {
        profile,
        target,
        gateway_binary,
    })
}

fn valid_target_triple(target: &str) -> bool {
    let parts: Vec<&str> = target.split('-').collect();
    parts.len() >= 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.as_slice(), [argument] if argument == "-h" || argument == "--help") {
        print!("{USAGE}");
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
            eprintln!("build-workshop failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let interrupt = match install_interrupt_handler() {
        Ok(interrupt) => interrupt,
        Err(error) => {
            eprintln!("build-workshop failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    match build_workshop(&request, &environment, &mut ProcessRunner::new(interrupt)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("build-workshop failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
