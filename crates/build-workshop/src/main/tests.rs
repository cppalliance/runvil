//! Unit tests for the build command sequence, with the fake runner and
//! test workspace every mode's tests share.

use std::collections::VecDeque;
use std::path::Path;

use tempfile::TempDir;

use super::*;
use args::InstallerRequest;

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
#[path = "tests/sidecar.rs"]
mod sidecar_tests;
#[path = "tests/stage.rs"]
mod stage_tests;

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

/// A Workshop build that asserts the sidecar is staged while it runs.
fn workshop_built_with_sidecar(sidecar: PathBuf, response: FakeResponse) -> FakeResponse {
    act(
        move || assert!(sidecar.is_file(), "sidecar not staged during the build"),
        response,
    )
}

/// A Workshop build that asserts no sidecar is staged while it runs.
fn workshop_built_without_sidecar(sidecar: PathBuf, response: FakeResponse) -> FakeResponse {
    act(
        move || assert!(!sidecar.exists(), "sidecar staged for a target without one"),
        response,
    )
}

/// A Workshop build that creates its profile directory, as Cargo does, so
/// a macOS or Linux build can copy the Gateway beside Workshop.
fn workshop_built_into(profile: PathBuf, response: FakeResponse) -> FakeResponse {
    act(
        move || write_file(&profile.join("runvil-workshop"), b"workshop"),
        response,
    )
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

struct TestEnvironment {
    temp: TempDir,
    environment: BuildEnvironment,
}

impl TestEnvironment {
    fn sidecar(&self, target: &str) -> PathBuf {
        sidecar::sidecar_path(&self.environment.workspace_root, target)
    }

    /// Stages a release Gateway in `target`'s slot with the fetch script's
    /// marker, as `tools/fetch-gateway-sidecar.mjs` leaves it.
    fn stage_gateway(&self, target: &str) -> PathBuf {
        self.stage_gateway_kind(target, "release")
    }

    /// Stages `tools/fetch-gateway-sidecar.mjs --stub`'s compile-only file.
    fn stage_stub(&self, target: &str) -> PathBuf {
        self.stage_gateway_kind(target, "stub")
    }

    fn stage_gateway_kind(&self, target: &str, kind: &str) -> PathBuf {
        let slot = self.sidecar(target);
        write_file(&slot, b"gateway");
        let mut marker = slot.clone().into_os_string();
        marker.push(".pinned");
        write_file(
            Path::new(&marker),
            format!("{{\n  \"kind\": \"{kind}\"\n}}\n").as_bytes(),
        );
        slot
    }

    /// A Gateway binary outside the slot, as `--gateway-binary` or
    /// `RUNVIL_GATEWAY_SIDECAR` names one.
    fn given_gateway(&self, target: &str) -> PathBuf {
        let path = self
            .temp
            .path()
            .join("given")
            .join(sidecar::gateway_binary_name(target));
        write_file(&path, b"given gateway");
        path
    }

    /// Writes `gateway-pin/pin.json` naming `version`.
    fn pin(&self, version: &str) {
        let pin = self
            .environment
            .workspace_root
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

/// The Gateway version the test workspace's pin names, apart from
/// Workshop's own so a test can tell the two apart.
const GATEWAY_VERSION: &str = "0.9.3";

fn environment() -> TestEnvironment {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let workspace_root = temp.path().join("repo");
    let target_root = temp.path().join("cargo-target");
    let tauri_cli = temp.path().join("tauri-cli").join("tauri.js");
    std::fs::create_dir_all(&workspace_root).expect("workspace root");
    write_file(&tauri_cli, b"");
    let sources = installer::bundle::BundleSources::in_workspace(&repository_root());
    let copies = installer::bundle::BundleSources::in_workspace(&workspace_root);
    for (source, copy) in [
        (sources.info_plist, copies.info_plist),
        (sources.icon, copies.icon),
    ] {
        write_file(&copy, &std::fs::read(source).expect("bundle source"));
    }
    ifw_tests::copy_sources(&repository_root(), &workspace_root);
    let qtifw_dir = Some(ifw_tests::fake_ifw(temp.path()).into_os_string());
    let test_environment = TestEnvironment {
        temp,
        environment: BuildEnvironment {
            workspace_root,
            target_root,
            target_root_from_env: true,
            cargo: PathBuf::from("selected-cargo"),
            node: PathBuf::from("selected-node"),
            tauri_cli,
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

/// A Cargo build, which runs with the absolute target root.
fn cargo_build(environment: &BuildEnvironment, args: &[&str]) -> CommandSpec {
    CommandSpec {
        envs: target_root_env(environment),
        ..command(environment, "selected-cargo", args, OutputMode::Inherit)
    }
}

/// Sets the key the release secrets set.
fn set_signing_key(environment: &mut BuildEnvironment) {
    environment.signing_key = Some(OsString::from("dW50cnVzdGVkIGNvbW1lbnQ6"));
}

#[test]
fn default_build_derives_host_and_uses_the_staged_gateway_in_place() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-pc-windows-msvc";
    let staged = test_environment.stage_gateway(triple);
    let mut runner = FakeRunner::with_responses(vec![
        success("cargo 1.89.0\nhost: x86_64-pc-windows-msvc\n"),
        workshop_built_with_sidecar(staged.clone(), success("")),
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

    assert_eq!(
        runner.commands,
        vec![
            command(environment, "selected-cargo", &["-vV"], OutputMode::Capture),
            cargo_build(environment, &["build", "-p", "workshop"]),
        ]
    );
    assert!(staged.is_file(), "a staged Gateway must outlive the build");
    // Windows carries the Gateway inside Workshop, never beside it.
    assert!(
        !environment
            .target_root
            .join("debug")
            .join("runvil-gateway.exe")
            .exists()
    );
}

#[test]
fn explicit_linux_release_target_copies_the_gateway_beside_workshop() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "aarch64-unknown-linux-gnu";
    let staged = test_environment.stage_gateway(triple);
    let profile = environment.target_root.join(triple).join("release");
    let mut runner =
        FakeRunner::with_responses(vec![workshop_built_into(profile.clone(), success(""))]);

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

    assert_eq!(
        runner.commands,
        vec![cargo_build(
            environment,
            &["build", "-p", "workshop", "--release", "--target", triple]
        )]
    );
    assert_eq!(
        std::fs::read(profile.join("runvil-gateway")).expect("Gateway beside Workshop"),
        b"gateway"
    );
    assert!(staged.is_file(), "a staged Gateway must outlive the build");
}

#[test]
fn a_target_root_from_cargo_configuration_is_left_to_cargo() {
    let mut test_environment = environment();
    test_environment.environment.target_root_from_env = false;
    let environment = &test_environment.environment;
    let triple = "aarch64-unknown-linux-gnu";
    test_environment.stage_gateway(triple);
    let profile = environment.target_root.join(triple).join("release");
    let mut runner = FakeRunner::with_responses(vec![workshop_built_into(profile, success(""))]);

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

    assert_eq!(runner.commands.len(), 1);
    assert!(
        runner
            .commands
            .iter()
            .all(|command| command.envs.is_empty())
    );
}
