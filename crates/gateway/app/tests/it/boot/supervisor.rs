//! Test he supervisor through the production binary: an ordinary launch serves
//! through a `--supervised` child, ends with it, stops it when killed, and
//! hands a second launch off to it.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gateway_api_discovery::{GatewayDiscoveryFile, ValidatedConnection};

use super::race_config;
use crate::support::{GatewayProcess, wait_for_connection};

const BOOT_TIMEOUT: Duration = Duration::from_secs(30);
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);

fn run_dir(temp: &tempfile::TempDir) -> PathBuf {
    temp.path().join(".runvil").join("run")
}

/// Polls `condition` until it holds, failing with `what` after `timeout`.
fn wait_until(timeout: Duration, what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;

    while !condition() {
        assert!(Instant::now() < deadline, "{what} within {timeout:?}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Whether `pid` has exited. A zombie counts: it has exited and only waits
/// for whoever inherited it to reap it.
fn process_exited(pid: u32) -> bool {
    let pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);

    system
        .process(pid)
        .is_none_or(|process| process.status() == sysinfo::ProcessStatus::Zombie)
}

/// `POST /shutdown` stops the child, and the supervisor exits with the
/// child's 0.
#[test]
fn a_shutdown_request_ends_the_child_and_the_supervisor_with_success() {
    let temp = tempfile::tempdir().unwrap();
    let config = race_config(&temp);
    let mut supervisor = GatewayProcess::spawn_headless_launch(&config, temp.path(), &[]);

    let connection = wait_for_connection(&run_dir(&temp), BOOT_TIMEOUT);
    let child_pid = connection.pid;
    assert_ne!(
        child_pid,
        supervisor.id(),
        "the supervisor's child should serve, not the supervisor"
    );

    let validated_connection =
        ValidatedConnection::validate_before(connection, Instant::now() + EXIT_TIMEOUT)
            .expect("the published connection should validate");

    gateway_api_discovery::request_shutdown(&validated_connection)
        .expect("the Gateway should accept a shutdown");

    let status = supervisor.wait_for_exit(EXIT_TIMEOUT);

    assert!(
        status.success(),
        "a stopped child should end the supervisor with 0, got {status}"
    );
    assert!(
        process_exited(child_pid),
        "the child exited before its supervisor"
    );
}

/// Killing the supervisor closes the child's stdin, and the child's
/// watchdog stops it cleanly: it removes its discovery file, which a
/// killed process would leave behind.
#[test]
fn killing_the_supervisor_stops_its_child() {
    let temp = tempfile::tempdir().unwrap();
    let config = race_config(&temp);
    let mut supervisor = GatewayProcess::spawn_headless_launch(&config, temp.path(), &[]);

    let connection = wait_for_connection(&run_dir(&temp), BOOT_TIMEOUT);
    let child_pid = connection.pid;

    supervisor.stop(EXIT_TIMEOUT);

    let file = gateway_api_discovery::gateway_discovery_file_path(&run_dir(&temp));
    wait_until(
        EXIT_TIMEOUT,
        "an orphaned child should remove its discovery file",
        || !file.exists(),
    );
    wait_until(EXIT_TIMEOUT, "an orphaned child should exit", || {
        process_exited(child_pid)
    });
}

/// A second ordinary launch finds the first supervisor's lock held and its
/// child live, hands off, and exits 0, leaving the first child serving.
/// `--login` keeps the handoff from opening a browser.
#[test]
fn a_second_launch_attaches_to_the_supervised_child() {
    let temp = tempfile::tempdir().unwrap();
    let config = race_config(&temp);

    let mut first = GatewayProcess::spawn_headless_launch(&config, temp.path(), &[]);

    let connection = wait_for_connection(&run_dir(&temp), BOOT_TIMEOUT);

    let mut second = GatewayProcess::spawn_headless_launch(&config, temp.path(), &["--login"]);

    let status = second.wait_for_exit(EXIT_TIMEOUT);
    assert!(
        status.success(),
        "a second launch should hand off with 0, got {status}"
    );

    let after = GatewayDiscoveryFile::read(&run_dir(&temp))
        .expect("read the gateway discovery file should not fail")
        .expect("the first child should still publish");

    assert_eq!(
        after.pid, connection.pid,
        "the first supervisor's child should still serve"
    );
    assert!(
        first.try_wait().is_none(),
        "the first supervisor should keep running"
    );

    first.stop(EXIT_TIMEOUT);
}
