//! The fake runner and the test environment the installer's tests share.

use std::collections::VecDeque;
use std::fmt;
use std::path::Path;

use tempfile::TempDir;

use super::*;

#[path = "tests/args.rs"]
mod args_tests;
#[path = "tests/bundle.rs"]
mod bundle_tests;
#[path = "tests/collect.rs"]
mod collect_tests;
#[path = "tests/failures.rs"]
mod failures;
#[path = "tests/gateway.rs"]
mod gateway_tests;
#[path = "tests/ifw.rs"]
mod ifw_tests;
#[path = "tests/installer.rs"]
mod installer_tests;
#[path = "tests/stage.rs"]
mod stage_tests;
#[path = "tests/workshop.rs"]
mod workshop_tests;

enum FakeResponse {
    Completed(CommandResult),
    CompletedAndCanceled,
    CanceledBeforeStart,
    CanceledAfterStart,
    SpawnFailed(io::ErrorKind, &'static str),
    /// Runs the effect, standing in for what the real child does to the
    /// filesystem, then answers with the inner response.
    Act(Box<dyn FnOnce()>, Box<FakeResponse>),
}

impl fmt::Debug for FakeResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Completed(result) => write!(formatter, "Completed({result:?})"),
            Self::CompletedAndCanceled => formatter.write_str("CompletedAndCanceled"),
            Self::CanceledBeforeStart => formatter.write_str("CanceledBeforeStart"),
            Self::CanceledAfterStart => formatter.write_str("CanceledAfterStart"),
            Self::SpawnFailed(kind, message) => {
                write!(formatter, "SpawnFailed({kind:?}, {message})")
            }
            Self::Act(_, response) => write!(formatter, "Act({response:?})"),
        }
    }
}

#[derive(Debug, Default)]
struct FakeRunner {
    responses: VecDeque<FakeResponse>,
    commands: Vec<CommandSpec>,
    cancel_observed: bool,
}

impl FakeRunner {
    fn with_responses(responses: Vec<FakeResponse>) -> Self {
        Self {
            responses: responses.into(),
            commands: Vec::new(),
            cancel_observed: false,
        }
    }

    fn answer(&mut self, response: FakeResponse) -> io::Result<CommandResult> {
        match response {
            FakeResponse::Completed(result) => Ok(result),
            FakeResponse::CompletedAndCanceled => {
                self.cancel_observed = true;
                Ok(CommandResult {
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                })
            }
            FakeResponse::CanceledBeforeStart | FakeResponse::CanceledAfterStart => {
                self.cancel_observed = true;
                Err(io::Error::new(io::ErrorKind::Interrupted, "canceled"))
            }
            FakeResponse::SpawnFailed(kind, message) => Err(io::Error::new(kind, message)),
            FakeResponse::Act(effect, response) => {
                effect();
                self.answer(*response)
            }
        }
    }
}

impl CommandRunner for FakeRunner {
    fn run(&mut self, command: &CommandSpec) -> io::Result<CommandResult> {
        self.commands.push(command.clone());
        if self.cancel_observed {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "canceled before child start",
            ));
        }
        let response = self.responses.pop_front().expect("unexpected command");
        self.answer(response)
    }

    fn cancel_observed(&self) -> bool {
        self.cancel_observed
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

fn act(effect: impl FnOnce() + 'static, response: FakeResponse) -> FakeResponse {
    FakeResponse::Act(Box::new(effect), Box::new(response))
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn write_file(path: &Path, contents: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("parent directory");
    std::fs::write(path, contents).expect("write file");
}

/// The Gateway version the test Workshop checkout's pin names, apart from
/// Workshop's own so a test can tell the two apart.
const GATEWAY_VERSION: &str = "0.9.3";

/// The Cargo manifest of a Workshop checkout at `version`.
fn workshop_manifest(version: &str) -> String {
    format!("[workspace.package]\nversion = \"{version}\"\n")
}

/// A build environment whose Workshop checkout and target directory are
/// in a temporary directory. The workspace root is this repository, whose
/// IFW definition, Gateway bundle template, `LICENSE`, and toolchain file
/// the build reads and never writes.
struct TestEnvironment {
    temp: TempDir,
    environment: BuildEnvironment,
}

impl TestEnvironment {
    /// Stages a Gateway for `target` in the Workshop checkout, where its
    /// `tools/fetch-gateway-sidecar.mjs` leaves one.
    fn stage_gateway(&self, target: &str) -> PathBuf {
        let staged = gateway::staged_path(&self.environment.workshop_root, target);
        write_file(&staged, b"gateway");
        staged
    }

    /// A Gateway binary outside the Workshop checkout, as
    /// `--gateway-binary` or `RUNVIL_GATEWAY_SIDECAR` names one.
    fn given_gateway(&self) -> PathBuf {
        let path = self.temp.path().join("given").join("runvil-gateway");
        write_file(&path, b"given gateway");
        path
    }

    /// Writes the Workshop checkout's `gateway-pin/pin.json` naming
    /// `version`.
    fn pin(&self, version: &str) {
        let pin = self
            .environment
            .workshop_root
            .join("gateway-pin")
            .join("pin.json");
        write_file(
            &pin,
            serde_json::json!({ "version": version })
                .to_string()
                .as_bytes(),
        );
    }
}

/// The environment of a build whose Workshop checkout agrees with this
/// workspace: its version, this repository's toolchain file, the Gateway
/// pin, and placeholders for the two icons the installer reads.
fn environment() -> TestEnvironment {
    let temp = tempfile::tempdir().expect("temporary root");
    let workspace_root = repository_root();
    let workshop_root = temp.path().join("runvil-workshop");
    let target_root = temp.path().join("cargo-target");
    write_file(
        &workshop_root.join("Cargo.toml"),
        workshop_manifest(installer::VERSION).as_bytes(),
    );
    write_file(
        &workshop_root.join("rust-toolchain.toml"),
        &std::fs::read(workspace_root.join("rust-toolchain.toml")).expect("toolchain file"),
    );
    for icon in ["icon.icns", "icon.png"] {
        write_file(
            &workshop::desktop(&workshop_root).join("icons").join(icon),
            format!("placeholder {icon}").as_bytes(),
        );
    }
    let qtifw_dir = Some(ifw_tests::fake_ifw(temp.path()).into_os_string());
    let test_environment = TestEnvironment {
        temp,
        environment: BuildEnvironment {
            workspace_root,
            workshop_root,
            target_root,
            target_root_from_env: true,
            cargo: PathBuf::from("selected-cargo"),
            signing_key: None,
            gateway_sidecar: None,
            qtifw_dir,
            path: None,
            release_date: "2026-10-11".to_owned(),
        },
    };
    test_environment.pin(GATEWAY_VERSION);
    test_environment
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
        envs: Vec::new(),
        output_mode,
    }
}

/// The environment a child that runs Cargo gets: the test target root
/// stands in for an absolute `CARGO_TARGET_DIR`.
fn target_root_env(environment: &BuildEnvironment) -> Vec<(OsString, OsString)> {
    vec![(
        OsString::from("CARGO_TARGET_DIR"),
        environment.target_root.clone().into_os_string(),
    )]
}

/// Sets the key the release secrets set.
fn set_signing_key(environment: &mut BuildEnvironment) {
    environment.signing_key = Some(OsString::from("dW50cnVzdGVkIGNvbW1lbnQ6"));
}
