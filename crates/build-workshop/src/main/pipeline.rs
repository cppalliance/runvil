//! The build pipeline: host discovery, product builds, sidecar staging, and cleanup.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::Path;

use super::args::valid_target_triple;
use super::installer::collect::copy_executable;
use super::sidecar::{self, GatewaySource};
use super::{
    BuildEnvironment, BuildError, BuildRequest, CommandRunner, CommandSpec, OutputMode, Profile,
    StepError, plugin,
};

pub(super) fn build_workshop(
    request: &BuildRequest,
    environment: &BuildEnvironment,
    runner: &mut impl CommandRunner,
) -> Result<(), BuildError> {
    let target = resolve_target(request.target.as_deref(), environment, runner)?;
    let given = request
        .gateway_binary
        .as_deref()
        .or(environment.gateway_sidecar.as_deref());
    let source = sidecar::gateway_source(given, &environment.workspace_root, &target)
        .map_err(StepError::failed)?;
    if source == (GatewaySource::Staged { stub: true }) {
        eprintln!(
            "warning: the Gateway staged at {} is a compile-only stub and cannot run; run \
             `node tools/fetch-gateway-sidecar.mjs --target {target}` for the pinned release, \
             or pass --gateway-binary",
            sidecar::sidecar_path(&environment.workspace_root, &target).display()
        );
    }
    with_staged_sidecar(
        environment,
        &target,
        runner,
        "Workshop build",
        &source,
        |runner| {
            let mut build = cargo_build_command(environment, request, "workshop");
            build.envs.extend(source.child_envs());
            run_checked(runner, &build, "Workshop build")?;
            let mut profile = environment.target_root.clone();
            if request.target.is_some() {
                profile.push(&target);
            }
            profile.push(request.profile.directory());
            // Workshop loads its Plugins from `plugins` beside its own
            // executable in a development tree.
            plugin::place(&plugin::built_libraries(&profile, &target), &profile).map_err(
                |reason| {
                    StepError::failed(format!(
                        "cannot place Workshop's Plugins beside it: {reason}"
                    ))
                },
            )?;
            if sidecar::bundles_sidecar(&target) {
                return Ok(());
            }
            // Workshop finds the Gateway beside its own executable in a
            // development tree, as tauri-build arranges on Windows.
            let beside = profile.join(sidecar::gateway_binary_name(&target));
            let gateway = source.binary(&environment.workspace_root, &target);
            // Copying a file onto itself would truncate it.
            if fs::canonicalize(&gateway).ok() == fs::canonicalize(&beside).ok() && beside.exists()
            {
                return Ok(());
            }
            // A Gateway from an earlier run may still be running from
            // `beside`, and writing into a running program's file fails on
            // Linux. A rename replaces the name and leaves that file alone.
            let fresh = beside.with_extension("new");
            // One left by a build that stopped mid-copy may be read-only.
            let _ = fs::remove_file(&fresh);
            copy_executable(&gateway, &fresh)
                .and_then(|()| {
                    fs::rename(&fresh, &beside).map_err(|error| {
                        format!(
                            "cannot move {} to {}: {error}",
                            fresh.display(),
                            beside.display()
                        )
                    })
                })
                .map_err(|reason| {
                    StepError::failed(format!(
                        "cannot place the Gateway beside Workshop: {reason}"
                    ))
                })
        },
    )
}

pub(super) fn resolve_target(
    target: Option<&str>,
    environment: &BuildEnvironment,
    runner: &mut impl CommandRunner,
) -> Result<String, BuildError> {
    match target {
        Some(target) => Ok(target.to_owned()),
        None => discover_host_target(environment, runner).map_err(BuildError::from),
    }
}

/// Runs `build_workshop` (named `workshop_label` in diagnostics). For a
/// target whose Workshop bundle carries the Gateway and a `source` that
/// names a binary, it stages that binary as the Tauri sidecar first and
/// removes it whatever happened, unless a cancel arrived before
/// anything was staged. A Gateway already staged is used in place and left
/// there.
pub(super) fn with_staged_sidecar<R: CommandRunner>(
    environment: &BuildEnvironment,
    target: &str,
    runner: &mut R,
    workshop_label: &str,
    source: &GatewaySource,
    build_workshop: impl FnOnce(&mut R) -> Result<(), StepError>,
) -> Result<(), BuildError> {
    let given = match source {
        GatewaySource::Given(path) if sidecar::bundles_sidecar(target) => path,
        _ => {
            build_workshop(runner)?;
            if runner.cancel_observed() {
                return Err(canceled_after_completion(workshop_label).into());
            }
            return Ok(());
        }
    };
    // Staging over a downloaded Gateway, then removing it, would lose the
    // download and leave its marker behind.
    let slot = sidecar::sidecar_path(&environment.workspace_root, target);
    if slot.exists() {
        return Err(StepError::failed(format!(
            "--gateway-binary or {} names a Gateway for {target}, but one is already staged \
             at {}; remove that file and its .pinned marker, or drop the named Gateway to use \
             the staged one",
            sidecar::GATEWAY_SIDECAR_VARIABLE,
            slot.display()
        ))
        .into());
    }
    let mut staged = false;
    let primary = build_around_sidecar(
        environment,
        target,
        runner,
        &mut staged,
        given,
        build_workshop,
    );
    let mut primary = match primary {
        Err(error) if error.canceled && !staged => return Err(error.into()),
        result => result,
    };
    if primary.is_ok() && runner.cancel_observed() {
        primary = Err(canceled_after_completion(workshop_label));
    }
    let cleanup = sidecar::remove(&environment.workspace_root, target)
        .map(|_| ())
        .map_err(|error| StepError::failed(format!("Gateway sidecar cleanup failed: {error}")));
    if primary.is_ok() && cleanup.is_ok() && runner.cancel_observed() {
        primary = Err(canceled_after_completion(workshop_label));
    }

    match (primary, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error.into()),
        (Err(primary), Err(cleanup)) => Err(BuildError {
            primary: primary.message,
            cleanup: Some(cleanup.message),
        }),
    }
}

fn build_around_sidecar<R: CommandRunner>(
    environment: &BuildEnvironment,
    target: &str,
    runner: &mut R,
    staged: &mut bool,
    source: &Path,
    build_workshop: impl FnOnce(&mut R) -> Result<(), StepError>,
) -> Result<(), StepError> {
    if runner.cancel_observed() {
        return Err(StepError {
            message: "Gateway sidecar staging canceled".to_owned(),
            canceled: true,
        });
    }
    // A copy that fails partway may leave a file behind, so cleanup counts
    // from the attempt.
    *staged = true;
    sidecar::stage(&environment.workspace_root, target, source)
        .map_err(|error| StepError::failed(format!("Gateway sidecar staging failed: {error}")))?;
    build_workshop(runner)
}

fn canceled_after_completion(workshop_label: &str) -> StepError {
    StepError {
        message: format!("{workshop_label} canceled after child completion"),
        canceled: true,
    }
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
            envs: Vec::new(),
            output_mode: OutputMode::Capture,
        },
        "Cargo host discovery",
    )?;
    output
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .filter(|target| valid_target_triple(target))
        .map(str::to_owned)
        .ok_or_else(|| {
            StepError::failed(
                "Cargo host triple was absent or malformed in `cargo -vV` output".to_owned(),
            )
        })
}

fn cargo_build_command(
    environment: &BuildEnvironment,
    request: &BuildRequest,
    package: &str,
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
        envs: environment.cargo_envs(),
        output_mode: OutputMode::Inherit,
    }
}

pub(super) fn run_checked(
    runner: &mut impl CommandRunner,
    command: &CommandSpec,
    label: &str,
) -> Result<String, StepError> {
    let result = runner.run(command).map_err(|error| {
        if error.kind() == io::ErrorKind::Interrupted {
            StepError {
                message: format!("{label} canceled: {error}"),
                canceled: true,
            }
        } else {
            StepError::failed(format!("{label} could not start: {error}"))
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
    Err(StepError::failed(format!("{label} failed{detail}")))
}
