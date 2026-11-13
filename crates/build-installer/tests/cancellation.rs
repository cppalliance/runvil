//! Subprocess coverage for a cancel during the Workshop bundle step.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

const WAIT_BOUND: Duration = Duration::from_secs(15);

// A Windows installer needs no Gateway and no Qt IFW, so the bundle is its
// first command. The fake Cargo builds nothing, so the Windows target
// serves on every machine.
const TARGET: &str = "x86_64-pc-windows-msvc";

#[test]
fn cancel_signal_during_the_bundle_kills_the_child_fails_and_leaves_no_output() {
    let temp = tempfile::tempdir().expect("temporary test root");
    let workshop_root = write_workshop_checkout(&temp);
    let target_root = temp.path().join("target");
    let fake_cargo = write_fake_cargo(&temp);
    let blocked_marker = temp.path().join("bundle-blocked");
    let stdout_path = temp.path().join("orchestrator.stdout");
    let stderr_path = temp.path().join("orchestrator.stderr");
    let stdout = File::create(&stdout_path).expect("stdout file");
    let stderr = File::create(&stderr_path).expect("stderr file");

    let mut command = Command::new(env!("CARGO_BIN_EXE_build-installer"));
    command
        .arg("--target")
        .arg(TARGET)
        .env("CARGO", fake_cargo)
        .env("RUNVIL_WORKSHOP", &workshop_root)
        .env("CARGO_TARGET_DIR", &target_root)
        .env_remove("RUNVIL_GATEWAY_SIDECAR")
        .current_dir(temp.path())
        .env("BUILD_INSTALLER_BLOCKED_MARKER", &blocked_marker)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    configure_signal_process_group(&mut command);
    let mut orchestrator = command.spawn().expect("start build orchestrator");

    wait_for_path(&blocked_marker, &mut orchestrator, &stderr_path);
    send_cancel_signal(orchestrator.id(), &temp);

    let status = wait_for_exit(&mut orchestrator, &stderr_path);
    assert!(!status.success(), "canceled orchestrator succeeded");
    let stderr = fs::read_to_string(stderr_path).expect("orchestrator stderr");
    assert!(
        stderr.contains("build-installer failed: Workshop bundle canceled"),
        "missing cancellation diagnostic: {stderr}"
    );
    let output = target_root.join("installer");
    assert!(
        !output.exists(),
        "canceled orchestrator left installer output at {}",
        output.display()
    );
}

/// Writes the two files the orchestrator's Workshop checkout check reads:
/// a manifest at this workspace's version, and this repository's toolchain
/// file.
fn write_workshop_checkout(temp: &TempDir) -> PathBuf {
    let workshop_root = temp.path().join("runvil-workshop");
    let manifest = format!(
        "[workspace.package]\nversion = \"{}\"\n",
        env!("CARGO_PKG_VERSION")
    );
    let toolchain = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("rust-toolchain.toml");
    let written = fs::create_dir(&workshop_root)
        .and_then(|()| fs::write(workshop_root.join("Cargo.toml"), manifest))
        .and_then(|()| fs::copy(toolchain, workshop_root.join("rust-toolchain.toml")));
    if let Err(error) = written {
        panic!("could not write the Workshop checkout: {error}");
    }
    workshop_root
}

fn wait_for_path(path: &Path, child: &mut Child, stderr_path: &Path) {
    let deadline = Instant::now() + WAIT_BOUND;
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => panic!("could not poll orchestrator: {error}"),
        };
        if let Some(status) = status {
            let stderr = fs::read_to_string(stderr_path).unwrap_or_default();
            panic!("orchestrator exited before blocking: {status}\n{stderr}");
        }
        thread::sleep(Duration::from_millis(20));
    }
    terminate_after_timeout(child);
    let stderr = fs::read_to_string(stderr_path).unwrap_or_default();
    panic!("orchestrator did not reach the blocked bundle child\n{stderr}");
}

fn wait_for_exit(child: &mut Child, stderr_path: &Path) -> ExitStatus {
    let deadline = Instant::now() + WAIT_BOUND;
    while Instant::now() < deadline {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => panic!("could not poll orchestrator: {error}"),
        };
        if let Some(status) = status {
            return status;
        }
        thread::sleep(Duration::from_millis(20));
    }
    terminate_after_timeout(child);
    let stderr = fs::read_to_string(stderr_path).unwrap_or_default();
    panic!("orchestrator did not exit after cancellation\n{stderr}");
}

fn terminate_after_timeout(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn write_fake_cargo(temp: &TempDir) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;

    let path = temp.path().join("fake-cargo");
    if let Err(error) = fs::write(
        &path,
        r#"#!/bin/sh
if [ "$1" = "workshop" ] && [ "$2" = "bundle" ]; then
  printf blocked > "$BUILD_INSTALLER_BLOCKED_MARKER"
  while :; do sleep 1; done
fi
exit 91
"#,
    ) {
        panic!("could not write fake Cargo: {error}");
    }
    if let Err(error) = fs::set_permissions(&path, fs::Permissions::from_mode(0o755)) {
        panic!("could not make fake Cargo executable: {error}");
    }
    path
}

#[cfg(windows)]
fn write_fake_cargo(temp: &TempDir) -> PathBuf {
    let path = temp.path().join("fake-cargo.cmd");
    if let Err(error) = fs::write(
        &path,
        r#"@echo off
if "%1"=="workshop" if "%2"=="bundle" goto bundle
exit /b 91
:bundle
> "%BUILD_INSTALLER_BLOCKED_MARKER%" echo blocked
:blocked
ping -n 2 127.0.0.1 >nul
goto blocked
"#,
    ) {
        panic!("could not write fake Cargo: {error}");
    }
    path
}

#[cfg(unix)]
fn configure_signal_process_group(_command: &mut Command) {}

#[cfg(windows)]
fn configure_signal_process_group(command: &mut Command) {
    use std::os::windows::process::CommandExt as _;

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

#[cfg(unix)]
fn send_cancel_signal(process_id: u32, _temp: &TempDir) {
    let status = Command::new("kill")
        .arg("-INT")
        .arg(process_id.to_string())
        .status();
    let status = match status {
        Ok(status) => status,
        Err(error) => panic!("could not send SIGINT: {error}"),
    };
    assert!(status.success(), "kill could not send SIGINT");
}

#[cfg(windows)]
fn send_cancel_signal(process_id: u32, temp: &TempDir) {
    let sender = temp.path().join("send-cancel-signal.ps1");
    if let Err(error) = fs::write(
        &sender,
        r#"Add-Type -TypeDefinition @"
using System.Runtime.InteropServices;
public static class ConsoleSignal {
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool GenerateConsoleCtrlEvent(uint signal, uint processGroupId);
}
"@
if (-not [ConsoleSignal]::GenerateConsoleCtrlEvent(1, [uint32]$args[0])) {
    exit 1
}
"#,
    ) {
        panic!("could not write the cancel signal sender: {error}");
    }
    let status = Command::new("powershell.exe")
        .arg("-NoLogo")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(sender)
        .arg(process_id.to_string())
        .status();
    let status = match status {
        Ok(status) => status,
        Err(error) => panic!("could not send the cancel signal: {error}"),
    };
    assert!(status.success(), "could not send the cancel signal");
}
