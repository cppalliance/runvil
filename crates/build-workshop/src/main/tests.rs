//! Unit tests for argument parsing and the build command sequence.

use std::collections::VecDeque;

use tempfile::TempDir;

use super::*;

#[path = "tests-failures.rs"]
mod failures;
#[path = "tests-gateway-source.rs"]
mod source;

#[derive(Debug)]
enum FakeResponse {
    Completed(CommandResult),
    CompletedAndInterrupted,
    InterruptedBeforeStart,
    InterruptedAfterStart,
    SpawnFailed(io::ErrorKind, &'static str),
}

#[derive(Debug, Default)]
struct FakeRunner {
    responses: VecDeque<FakeResponse>,
    commands: Vec<CommandSpec>,
    last_command_started: bool,
    interruption_observed: bool,
}

impl FakeRunner {
    fn with_responses(responses: Vec<FakeResponse>) -> Self {
        Self {
            responses: responses.into(),
            commands: Vec::new(),
            last_command_started: false,
            interruption_observed: false,
        }
    }
}

impl CommandRunner for FakeRunner {
    fn run(
        &mut self,
        command: &CommandSpec,
        interrupt_policy: InterruptPolicy,
    ) -> io::Result<CommandResult> {
        self.commands.push(command.clone());
        self.last_command_started = false;
        if interrupt_policy == InterruptPolicy::RejectExisting && self.interruption_observed {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "interrupted before child start",
            ));
        }
        match self.responses.pop_front().expect("unexpected command") {
            FakeResponse::Completed(result) => {
                self.last_command_started = true;
                Ok(result)
            }
            FakeResponse::CompletedAndInterrupted => {
                self.last_command_started = true;
                self.interruption_observed = true;
                Ok(CommandResult {
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                })
            }
            FakeResponse::InterruptedBeforeStart => {
                self.interruption_observed = true;
                Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted"))
            }
            FakeResponse::InterruptedAfterStart => {
                self.last_command_started = true;
                self.interruption_observed = true;
                Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted"))
            }
            FakeResponse::SpawnFailed(kind, message) => Err(io::Error::new(kind, message)),
        }
    }

    fn last_command_started(&self) -> bool {
        self.last_command_started
    }

    fn interruption_observed(&self) -> bool {
        self.interruption_observed
    }
}

fn success(stdout: &str) -> FakeResponse {
    FakeResponse::Completed(CommandResult {
        success: true,
        stdout: stdout.to_owned(),
        stderr: String::new(),
    })
}

fn failure(stderr: &str) -> FakeResponse {
    FakeResponse::Completed(CommandResult {
        success: false,
        stdout: String::new(),
        stderr: stderr.to_owned(),
    })
}

struct TestEnvironment {
    _temp: TempDir,
    environment: BuildEnvironment,
}

fn environment() -> TestEnvironment {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let workspace_root = temp.path().join("repo");
    let target_root = temp.path().join("cargo-target");
    std::fs::create_dir_all(&workspace_root).expect("workspace root");
    TestEnvironment {
        _temp: temp,
        environment: BuildEnvironment {
            workspace_root,
            target_root,
            cargo: PathBuf::from("selected-cargo"),
            node: PathBuf::from("selected-node"),
            gateway_sidecar: None,
        },
    }
}

fn strings(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn command(
    environment: &BuildEnvironment,
    program: &str,
    args: &[&str],
    output_mode: OutputMode,
) -> CommandSpec {
    CommandSpec {
        program: PathBuf::from(program),
        args: strings(args),
        current_dir: environment.workspace_root.clone(),
        env: Vec::new(),
        output_mode,
    }
}

/// The staging script's absolute path, as the pipeline passes it.
fn staging_script(environment: &BuildEnvironment) -> String {
    environment
        .workspace_root
        .join("tools")
        .join("stage-gateway-sidecar.mjs")
        .to_str()
        .expect("UTF-8 script")
        .to_owned()
}

/// The `stage` command's arguments, spelled once for both order tests.
fn stage_arguments(environment: &BuildEnvironment, target: &str, source: &str) -> Vec<String> {
    vec![
        staging_script(environment),
        "stage".to_owned(),
        "--target".to_owned(),
        target.to_owned(),
        "--source".to_owned(),
        source.to_owned(),
    ]
}

/// The `remove` command's arguments.
fn remove_arguments(environment: &BuildEnvironment, target: &str) -> Vec<String> {
    vec![
        staging_script(environment),
        "remove".to_owned(),
        "--target".to_owned(),
        target.to_owned(),
    ]
}

/// A command spec from owned argument strings.
fn owned_command(
    environment: &BuildEnvironment,
    program: &str,
    args: &[String],
    output_mode: OutputMode,
) -> CommandSpec {
    CommandSpec {
        program: PathBuf::from(program),
        args: args.iter().map(OsString::from).collect(),
        current_dir: environment.workspace_root.clone(),
        env: Vec::new(),
        output_mode,
    }
}

/// The Workshop build's spec: the one command that carries the sidecar
/// path in its environment.
fn workshop_command(environment: &BuildEnvironment, target: &str, args: &[&str]) -> CommandSpec {
    let mut spec = command(environment, "selected-cargo", args, OutputMode::Inherit);
    spec.env = vec![(
        OsString::from(GATEWAY_SIDECAR_VARIABLE),
        super::pipeline::sidecar_path(environment, target).into_os_string(),
    )];
    spec
}

#[test]
fn parses_only_the_documented_profile_and_target_options() {
    assert_eq!(
        parse_arguments(&[]).expect("debug request"),
        BuildRequest {
            profile: Profile::Debug,
            target: None,
            gateway_binary: None,
        }
    );
    assert_eq!(
        parse_arguments(&[
            "--target".to_owned(),
            "aarch64-apple-darwin".to_owned(),
            "--release".to_owned(),
            "--gateway-binary".to_owned(),
            "../gateway/target/debug/runvil-gateway".to_owned(),
        ])
        .expect("release target request"),
        BuildRequest {
            profile: Profile::Release,
            target: Some("aarch64-apple-darwin".to_owned()),
            gateway_binary: Some(PathBuf::from("../gateway/target/debug/runvil-gateway")),
        }
    );
}

#[test]
fn rejects_product_features_and_other_unsupported_arguments() {
    for args in [
        vec!["--features".to_owned(), "local".to_owned()],
        vec!["--profile".to_owned(), "dist".to_owned()],
        vec!["gateway".to_owned()],
    ] {
        let error = parse_arguments(&args)
            .expect_err("unsupported argument")
            .to_string();
        assert!(error.contains("unsupported argument"), "{error}");
        assert!(error.contains(USAGE), "{error}");
    }
}

#[test]
fn rejects_duplicate_or_incomplete_options() {
    for args in [
        vec!["--release".to_owned(), "--release".to_owned()],
        vec!["--target".to_owned()],
        vec![
            "--target".to_owned(),
            "x86_64-pc-windows-msvc".to_owned(),
            "--target".to_owned(),
            "x86_64-unknown-linux-gnu".to_owned(),
        ],
        vec!["--gateway-binary".to_owned()],
        vec![
            "--gateway-binary".to_owned(),
            "a/runvil-gateway".to_owned(),
            "--gateway-binary".to_owned(),
            "b/runvil-gateway".to_owned(),
        ],
    ] {
        assert!(parse_arguments(&args).is_err(), "{args:?}");
    }
}

#[test]
fn default_build_derives_host_and_cleans_up_in_order() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let mut runner = FakeRunner::with_responses(vec![
        success("cargo 1.89.0\nhost: x86_64-pc-windows-msvc\n"),
        success(""),
        success(""),
        success(""),
        success(""),
    ]);

    build_workshop(
        &BuildRequest {
            profile: Profile::Debug,
            target: None,
            gateway_binary: None,
        },
        environment,
        &mut runner,
    )
    .expect("Workshop build");

    let source = environment
        .target_root
        .join("debug")
        .join("runvil-gateway.exe");
    assert_eq!(
        runner.commands,
        vec![
            command(environment, "selected-cargo", &["-vV"], OutputMode::Capture),
            command(
                environment,
                "selected-cargo",
                &["build", "-p", "gateway"],
                OutputMode::Inherit,
            ),
            owned_command(
                environment,
                "selected-node",
                &stage_arguments(
                    environment,
                    "x86_64-pc-windows-msvc",
                    source.to_str().expect("UTF-8 source"),
                ),
                OutputMode::Inherit,
            ),
            workshop_command(
                environment,
                "x86_64-pc-windows-msvc",
                &["build", "-p", "workshop"],
            ),
            owned_command(
                environment,
                "selected-node",
                &remove_arguments(environment, "x86_64-pc-windows-msvc"),
                OutputMode::Inherit,
            ),
        ]
    );
}

#[test]
fn explicit_release_target_uses_target_output_without_a_host_probe() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "aarch64-unknown-linux-gnu";
    let mut runner =
        FakeRunner::with_responses(vec![success(""), success(""), success(""), success("")]);

    build_workshop(
        &BuildRequest {
            profile: Profile::Release,
            target: Some(triple.to_owned()),
            gateway_binary: None,
        },
        environment,
        &mut runner,
    )
    .expect("Workshop build");

    let source = environment
        .target_root
        .join(triple)
        .join("release")
        .join("runvil-gateway");
    assert_eq!(
        runner.commands,
        vec![
            command(
                environment,
                "selected-cargo",
                &["build", "-p", "gateway", "--release", "--target", triple,],
                OutputMode::Inherit,
            ),
            owned_command(
                environment,
                "selected-node",
                &stage_arguments(environment, triple, source.to_str().expect("UTF-8 source")),
                OutputMode::Inherit,
            ),
            workshop_command(
                environment,
                triple,
                &["build", "-p", "workshop", "--release", "--target", triple,],
            ),
            owned_command(
                environment,
                "selected-node",
                &remove_arguments(environment, triple),
                OutputMode::Inherit,
            ),
        ]
    );
}
