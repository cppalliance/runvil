//! The supervisor is the process an ordinary launch runs. It runs the serving
//! gateway as its `--supervised` child, copying the child's output to its own
//! and exiting with the child's code. It holds the launch lock for its whole
//! lifetime.
//!
//! The supervisor does not start the logging system, the child gateway process
//! owns the log file.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, ExitCode, ExitStatus, Stdio};
use std::thread::JoinHandle;

use gateway::AppOptions;
use gateway_api_discovery::LaunchLock;

/// Supervisor entry point: starts and manages the child gateway process,
/// returning the process exit code when it ends. Holds the launch lock while
/// the gateway process is running, so another launch attempt attaches to the
/// live child instead of launching a new gateway.
pub(crate) fn run(options: &AppOptions, _launch_lock: LaunchLock) -> ExitCode {
    let mut child = match build_child_command(options).spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("error: could not start the gateway: {error}");
            return ExitCode::FAILURE;
        }
    };

    // The child's stdin stays open, unused, for as long as this process
    // lives. When the pipe closes, the child's watchdog stops it.
    let _child_stdin = child.stdin.take();

    let child_stdout = child.stdout.take();
    let child_stderr = child.stderr.take();

    // Forward the child's stdout and std_error to this thread's output streams
    let output_handles = [
        child_stdout.map(|pipe| OutputStream::Stdout.forward(pipe)),
        child_stderr.map(|pipe| OutputStream::Stderr.forward(pipe)),
    ];

    let status = child.wait();

    // The pipes close when the child exits; joining flushes its last lines.
    for handle in output_handles.into_iter().flatten() {
        let _ = handle.join();
    }

    match status {
        Ok(status) => exit_code(status),
        Err(error) => {
            eprintln!("error: could not wait for the gateway: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Builds the command to launch the child Gateway: this binary with `--supervised`
/// flag set, passing currently set app options on to the child.
fn build_child_command(options: &AppOptions) -> Command {
    // A missing executable path makes `spawn` fail with a clear error.
    let exe = std::env::current_exe().unwrap_or_else(|_| "runvil-gateway".into());
    let mut command = Command::new(exe);

    command.arg("--supervised");

    if !options.tray {
        command.arg("--no-tray");
    }

    // Already resolved against RUNVIL_GATEWAY_CONFIG, so the child needs no
    // second lookup.
    if let Some(config) = &options.serve.config_path {
        command.arg("--config").arg(config);
    }

    if let Some(profile) = &options.serve.profile {
        command.arg("--profile").arg(profile.as_str());
    }

    if options.serve.browser {
        command.arg("--browser");
    }

    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    command
}

/// Control forwarding of the child's output to the supervisor
#[derive(Clone, Copy)]
enum OutputStream {
    Stdout,
    Stderr,
}

impl OutputStream {
    /// Copies every line from one of the child's pipes to the matching stream
    /// of this process, as bytes, so output that is not UTF-8 passes through.
    fn forward(self, pipe: impl Read + Send + 'static) -> JoinHandle<()> {
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).split(b'\n') {
                let Ok(mut line) = line else { break };
                line.push(b'\n');

                let _ = match self {
                    OutputStream::Stdout => std::io::stdout().write_all(&line),
                    OutputStream::Stderr => std::io::stderr().write_all(&line),
                };
            }
        })
    }
}

/// Builds the supervisor's exit code from the child's `ExitStatus`:
/// the child processes i16 `ExitStatus::code` is translated to a u8 `ExitCode`,
/// or `ExitCode::FAILURE` for a code outside the range of u8.
fn exit_code(status: ExitStatus) -> ExitCode {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from)
}
