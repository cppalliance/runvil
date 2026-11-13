//! The checked command run every installer step goes through, and the
//! discovery of the target a build without `--target` is for.

use std::ffi::OsString;
use std::io;

use super::args::valid_target_triple;
use super::{BuildEnvironment, CommandRunner, CommandSpec, OutputMode};

pub(super) fn resolve_target(
    target: Option<&str>,
    environment: &BuildEnvironment,
    runner: &mut impl CommandRunner,
) -> Result<String, String> {
    match target {
        Some(target) => Ok(target.to_owned()),
        None => discover_host_target(environment, runner),
    }
}

fn discover_host_target(
    environment: &BuildEnvironment,
    runner: &mut impl CommandRunner,
) -> Result<String, String> {
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
        .ok_or_else(|| "Cargo host triple was absent or malformed in `cargo -vV` output".to_owned())
}

pub(super) fn run_checked(
    runner: &mut impl CommandRunner,
    command: &CommandSpec,
    label: &str,
) -> Result<String, String> {
    let result = runner.run(command).map_err(|error| {
        if error.kind() == io::ErrorKind::Interrupted {
            format!("{label} canceled: {error}")
        } else {
            format!("{label} could not start: {error}")
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
    Err(format!("{label} failed{detail}"))
}
