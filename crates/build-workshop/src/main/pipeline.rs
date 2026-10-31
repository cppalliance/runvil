//! The build pipeline: host discovery, product builds, sidecar staging, and cleanup.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use super::{
    BuildEnvironment, BuildError, BuildRequest, CommandRunner, CommandSpec,
    GATEWAY_SIDECAR_VARIABLE, InterruptPolicy, OutputMode, Profile, StepError, valid_target_triple,
};

/// Where the staged Gateway sidecar comes from this run.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum GatewaySource {
    /// Stage the binary at this path: `--gateway-binary`, or the
    /// `RUNVIL_GATEWAY_SIDECAR` environment variable a CI job sets.
    Given(PathBuf),
    /// A sidecar file is already in place; use it untouched. Nothing is
    /// staged or removed, so a downloaded or hand-staged Gateway
    /// survives the run and the next `cargo workshop` reuses it too.
    ///
    /// The flag is true when that file is a compile-only stub, so the run
    /// says so instead of quietly producing an app with no Gateway.
    Staged { stub: bool },
    /// No sidecar is staged and none was named. The Gateway is its own
    /// product in `cppalliance/runvil-gateway`, so there is nothing here
    /// to build: the run stops and names the fetch script.
    Missing,
}

pub(super) fn build_workshop(
    request: &BuildRequest,
    environment: &BuildEnvironment,
    runner: &mut impl CommandRunner,
) -> Result<(), BuildError> {
    let target = match &request.target {
        Some(target) => target.clone(),
        None => discover_host_target(environment, runner).map_err(|error| BuildError {
            primary: error.message,
            cleanup: None,
        })?,
    };
    let source = gateway_source(request, environment, &target);
    if let GatewaySource::Staged { stub: true } = &source {
        eprintln!(
            "warning: reusing a compile-only stub Gateway at {}; it cannot run. \
             Run `node tools/fetch-gateway-sidecar.mjs --target {target}` for a real one, \
             or pass --gateway-binary.",
            sidecar_path(environment, &target).display()
        );
    }
    let mut staging_started = false;
    let primary = build_products(
        request,
        &target,
        &source,
        environment,
        runner,
        &mut staging_started,
    );
    let mut primary = match primary {
        Err(error) if error.interrupted && !staging_started => {
            return Err(BuildError {
                primary: error.message,
                cleanup: None,
            });
        }
        result => result,
    };
    if primary.is_ok() && runner.interruption_observed() {
        primary = Err(StepError {
            message: "Workshop build interrupted after child completion".to_owned(),
            interrupted: true,
            command_started: true,
        });
    }
    // Cleanup removes only what this run put there. A sidecar that was
    // already staged belongs to whoever staged it, and deleting it would
    // undo the download the next build is meant to reuse.
    let cleanup = if matches!(
        source,
        GatewaySource::Staged { .. } | GatewaySource::Missing
    ) {
        Ok(())
    } else {
        run_checked(
            runner,
            &sidecar_command(environment, "remove", &target, None),
            "Gateway sidecar cleanup",
            InterruptPolicy::AllowExisting,
        )
        .map(|_| ())
    };
    if primary.is_ok() && cleanup.is_ok() && runner.interruption_observed() {
        primary = Err(StepError {
            message: "Workshop build interrupted after child completion".to_owned(),
            interrupted: true,
            command_started: true,
        });
    }

    match (primary, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(BuildError {
            primary: error.message,
            cleanup: None,
        }),
        (Err(primary), Err(cleanup)) => Err(BuildError {
            primary: primary.message,
            cleanup: Some(cleanup.message),
        }),
    }
}

/// Decides where this run's sidecar comes from: the CLI flag, then the
/// environment variable, then a sidecar already in place. There is no
/// build fallback, because the Gateway is its own product.
pub(super) fn gateway_source(
    request: &BuildRequest,
    environment: &BuildEnvironment,
    target: &str,
) -> GatewaySource {
    if let Some(path) = &request.gateway_binary {
        return GatewaySource::Given(path.clone());
    }
    if let Some(path) = &environment.gateway_sidecar {
        return GatewaySource::Given(path.clone());
    }
    let staged = sidecar_path(environment, target);
    if staged.is_file() {
        // `tools/fetch-gateway-sidecar.mjs` records what it staged beside
        // the file. A stub is reused like any other, but the run says so:
        // a developer who left one behind would otherwise build an app
        // with no Gateway and no clue why.
        let stub = sidecar_pin_is_stub(environment, target);
        return GatewaySource::Staged { stub };
    }
    GatewaySource::Missing
}

/// Whether the staged sidecar's marker says it is a compile-only stub.
/// A missing or unreadable marker means it was not staged by the fetch
/// script, so it is treated as a real Gateway and nothing is said.
fn sidecar_pin_is_stub(environment: &BuildEnvironment, target: &str) -> bool {
    let mut marker = sidecar_path(environment, target).into_os_string();
    marker.push(".pinned");
    let Ok(text) = std::fs::read_to_string(PathBuf::from(marker)) else {
        return false;
    };
    // The marker is JSON written by the fetch script; a substring check is
    // enough here and keeps this crate to the dependencies it already has.
    text.contains("\"kind\": \"stub\"") || text.contains("\"kind\":\"stub\"")
}

/// The path `tauri.conf.json`'s `externalBin` entry resolves for `target`:
/// Tauri's sidecar naming rule applied under the desktop crate.
///
/// `tools/stage-gateway-sidecar.mjs` owns that rule; the
/// `sidecar_path_matches_the_staging_script` test runs the script and
/// compares, so the two cannot drift quietly.
pub(super) fn sidecar_path(environment: &BuildEnvironment, target: &str) -> PathBuf {
    let extension = if is_windows(target) { ".exe" } else { "" };
    environment
        .workspace_root
        .join("crates")
        .join("workshop")
        .join("desktop")
        .join("binaries")
        .join(format!("runvil-gateway-{target}{extension}"))
}

fn discover_host_target(
    environment: &BuildEnvironment,
    runner: &mut impl CommandRunner,
) -> Result<String, StepError> {
    let output = run_checked(
        runner,
        &CommandSpec {
            program: environment.cargo.clone(),
            args: vec![OsString::from("-vV")],
            current_dir: environment.workspace_root.clone(),
            env: Vec::new(),
            output_mode: OutputMode::Capture,
        },
        "Cargo host discovery",
        InterruptPolicy::RejectExisting,
    )?;
    output
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .filter(|target| valid_target_triple(target))
        .map(str::to_owned)
        .ok_or_else(|| StepError {
            message: "Cargo host triple was absent or malformed in `cargo -vV` output".to_owned(),
            interrupted: false,
            command_started: true,
        })
}

fn build_products(
    request: &BuildRequest,
    target: &str,
    source: &GatewaySource,
    environment: &BuildEnvironment,
    runner: &mut impl CommandRunner,
    staging_started: &mut bool,
) -> Result<(), StepError> {
    let staged_source = match source {
        GatewaySource::Staged { .. } => {
            return build_workshop_product(request, environment, runner, None);
        }
        GatewaySource::Given(path) => path.clone(),
        GatewaySource::Missing => {
            return Err(StepError {
                message: format!(
                    "no Gateway sidecar for {target}; the Gateway is a separate product, so run \
                     `node tools/fetch-gateway-sidecar.mjs --target {target}` to stage the release \
                     gateway-pin/pin.json names, or pass --gateway-binary"
                ),
                interrupted: false,
                command_started: false,
            });
        }
    };

    match run_checked(
        runner,
        &sidecar_command(environment, "stage", target, Some(staged_source)),
        "Gateway sidecar staging",
        InterruptPolicy::RejectExisting,
    ) {
        Ok(_) => *staging_started = true,
        Err(error) => {
            *staging_started = error.command_started;
            return Err(error);
        }
    }
    // The Workshop build is told where the sidecar is, so the desktop
    // build script copies exactly this file rather than guessing from
    // modification times. The path Tauri resolves is the same one.
    let staged = sidecar_path(environment, target);
    build_workshop_product(request, environment, runner, Some(staged))
}

fn build_workshop_product(
    request: &BuildRequest,
    environment: &BuildEnvironment,
    runner: &mut impl CommandRunner,
    gateway_sidecar: Option<PathBuf>,
) -> Result<(), StepError> {
    let env = match gateway_sidecar {
        Some(path) => vec![(
            OsString::from(GATEWAY_SIDECAR_VARIABLE),
            path.into_os_string(),
        )],
        None => Vec::new(),
    };
    run_checked(
        runner,
        &cargo_command(environment, request, "workshop", env),
        "Workshop build",
        InterruptPolicy::RejectExisting,
    )?;
    Ok(())
}

/// A `cargo build -p <package>` spec, with `env` passed to the child.
fn cargo_command(
    environment: &BuildEnvironment,
    request: &BuildRequest,
    package: &str,
    env: Vec<(OsString, OsString)>,
) -> CommandSpec {
    let mut args = vec![
        OsString::from("build"),
        OsString::from("-p"),
        OsString::from(package),
    ];
    if request.profile == Profile::Release {
        args.push(OsString::from("--release"));
    }
    if let Some(target) = &request.target {
        args.push(OsString::from("--target"));
        args.push(OsString::from(target));
    }
    CommandSpec {
        program: environment.cargo.clone(),
        args,
        current_dir: environment.workspace_root.clone(),
        env,
        output_mode: OutputMode::Inherit,
    }
}

fn sidecar_command(
    environment: &BuildEnvironment,
    action: &str,
    target: &str,
    source: Option<PathBuf>,
) -> CommandSpec {
    let mut args = vec![
        environment
            .workspace_root
            .join("tools")
            .join("stage-gateway-sidecar.mjs")
            .into_os_string(),
        OsString::from(action),
        OsString::from("--target"),
        OsString::from(target),
    ];
    if let Some(source) = source {
        args.push(OsString::from("--source"));
        args.push(source.into_os_string());
    }
    CommandSpec {
        program: environment.node.clone(),
        args,
        current_dir: environment.workspace_root.clone(),
        env: Vec::new(),
        output_mode: OutputMode::Inherit,
    }
}

fn is_windows(target: &str) -> bool {
    target.split('-').any(|part| part == "windows")
}

fn run_checked(
    runner: &mut impl CommandRunner,
    command: &CommandSpec,
    label: &str,
    interrupt_policy: InterruptPolicy,
) -> Result<String, StepError> {
    let result = runner.run(command, interrupt_policy);
    let command_started = runner.last_command_started();
    let result = result.map_err(|error| {
        if error.kind() == io::ErrorKind::Interrupted {
            StepError {
                message: format!("{label} interrupted: {error}"),
                interrupted: true,
                command_started,
            }
        } else {
            StepError {
                message: format!("{label} could not start: {error}"),
                interrupted: false,
                command_started,
            }
        }
    })?;
    if result.success {
        return Ok(result.stdout);
    }
    let detail = if result.stderr.trim().is_empty() {
        String::new()
    } else {
        format!(": {}", result.stderr.trim())
    };
    Err(StepError {
        message: format!("{label} failed{detail}"),
        interrupted: false,
        command_started,
    })
}
