//! Conformance between the two halves of the Gateway discovery contract.
//!
//! While both halves still live in this workspace, run the Gateway's real
//! writer and Workshop's forked reader against each other. This catches a
//! disagreement in the field set, the serialization, the unknown-field
//! tolerance, or the validation rule - the drift the fork's own unit tests
//! cannot see, because each half only checks its own idea of the contract.
//!
//! This is the strongest check available and it disappears when the Gateway
//! moves to its own repository, at which point `CONTRACT.md` and the golden
//! fixtures are what is left. `build-xtask` is the one crate the product
//! rules let depend into both families, which is why the check lives here
//! rather than in either half.

#![expect(
    clippy::expect_used,
    reason = "test helpers panic on setup failure, which is the desired behavior"
)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use workshop_gateway_discovery::{
    GatewayDiscoveryFile, Resolution, StaleReason, launch_or_attach, resolve,
};

/// The environment variable naming the run directory the child serves in.
const CHILD_RUN_DIR_ENV: &str = "RUNVIL_DISCOVERY_CONFORMANCE_RUN_DIR";

/// The child's shutdown marker file, written when it observes the end of
/// its serving loop.
const CHILD_READY: &str = "gateway-ready";

/// The image name the reader accepts.
#[cfg(windows)]
const GATEWAY_IMAGE_NAME: &str = "runvil-gateway.exe";
/// The image name the reader accepts.
#[cfg(not(windows))]
const GATEWAY_IMAGE_NAME: &str = "runvil-gateway";

/// The writer's file, with the pid and port the fixture child supplies.
fn writer_file(pid: u32, port: u16) -> gateway_api_discovery::GatewayDiscoveryFile {
    gateway_api_discovery::GatewayDiscoveryFile {
        port,
        api_key: "0e1f8f0d8f8c4e0f9a1c2b3d4e5f6a7b".to_owned(),
        pid,
        epoch: 1_757_000_000,
        version: "0.2.0".to_owned(),
        started_at: "2026-09-03T12:00:00Z".to_owned(),
    }
}

/// The reader's view of the same file.
fn reader_file(pid: u32, port: u16) -> GatewayDiscoveryFile {
    GatewayDiscoveryFile {
        port,
        api_key: "0e1f8f0d8f8c4e0f9a1c2b3d4e5f6a7b".to_owned(),
        pid,
        epoch: 1_757_000_000,
        version: "0.2.0".to_owned(),
        started_at: "2026-09-03T12:00:00Z".to_owned(),
    }
}

/// A named fixture child: a copy of this test binary running under the
/// Gateway's image name, writing the Gateway writer's file and answering
/// the two probes.
struct WriterChild {
    child: std::process::Child,
    port: u16,
    _directory: tempfile::TempDir,
}

impl WriterChild {
    /// Starts the child in `run_dir` and waits for its readiness marker.
    fn spawn(run_dir: &Path) -> Self {
        let directory = tempfile::TempDir::new().expect("create the image directory");
        let executable = directory.path().join(GATEWAY_IMAGE_NAME);
        std::fs::copy(
            std::env::current_exe().expect("locate the test executable"),
            &executable,
        )
        .expect("copy the test executable under the Gateway image name");
        let mut child = std::process::Command::new(&executable)
            .args([
                "--exact",
                "the_writer_child_process",
                "--ignored",
                "--nocapture",
            ])
            .env(CHILD_RUN_DIR_ENV, run_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("start the named child");

        let ready = run_dir.join(CHILD_READY);
        let deadline = Instant::now() + Duration::from_secs(20);
        while !ready.is_file() {
            assert!(
                child.try_wait().expect("observe the child").is_none(),
                "the writer child exited before publishing a file"
            );
            assert!(
                Instant::now() < deadline,
                "the writer child did not publish a file within the deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let published = gateway_api_discovery::GatewayDiscoveryFile::read(run_dir)
            .expect("read the child's file")
            .expect("the child published a file");
        Self {
            child,
            port: published.port,
            _directory: directory,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Stops the child and waits for it to exit.
    fn stop(&mut self) {
        let _ = self.child.kill();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self
                .child
                .try_wait()
                .expect("observe the writer child")
                .is_some()
            {
                return;
            }
            assert!(Instant::now() < deadline, "the writer child did not stop");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for WriterChild {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// The reader, run against a real Gateway-written file from a real process
/// under the Gateway image name. This is the whole gauntlet: parse, process
/// identity, boot identity, one-connection health and bearer proof, and the
/// second process observation.
#[test]
fn the_workshop_reader_attaches_to_a_gateway_written_file_end_to_end() {
    let run = tempfile::TempDir::new().expect("tempdir");
    let child = WriterChild::spawn(run.path());

    let resolution = resolve(run.path()).expect("resolve");

    assert_eq!(
        resolution,
        Resolution::Attach(reader_file(child.pid(), child.port)),
        "the reader attaches to the process the Gateway writer described"
    );
    assert!(
        run.path().join("gateway.json").exists(),
        "a live file is left in place"
    );
}

#[test]
fn the_workshop_reader_rejects_a_gateway_written_file_whose_key_is_wrong() {
    // The child accepts one key; the file carries another. Everything but
    // the bearer proof passes, so this pins that step on real bytes.
    let run = tempfile::TempDir::new().expect("tempdir");
    let child = WriterChild::spawn(run.path());
    gateway_api_discovery::GatewayDiscoveryFile {
        api_key: "a-key-the-child-never-accepts".to_owned(),
        ..writer_file(child.pid(), child.port)
    }
    .write_to(run.path())
    .expect("rewrite the file with the wrong key");

    let resolution = resolve(run.path()).expect("resolve");

    assert_eq!(
        resolution,
        Resolution::Stale(StaleReason::KeyRejected),
        "a Gateway-written file with a stale key is stale on authority"
    );
    assert!(
        !run.path().join("gateway.json").exists(),
        "the reader cleaned the stale file"
    );
}

#[test]
fn the_workshop_reader_rejects_a_gateway_written_file_after_the_process_dies() {
    let run = tempfile::TempDir::new().expect("tempdir");
    let mut child = WriterChild::spawn(run.path());
    child.stop();

    let resolution = resolve(run.path()).expect("resolve");

    assert_eq!(
        resolution,
        Resolution::Stale(StaleReason::ProcessDead),
        "a dead pid is stale on process"
    );
}

#[test]
fn the_workshop_launch_election_attaches_to_a_gateway_written_file() {
    // The launch race's holder re-validates before launching; that is the
    // path a Workshop boot takes when a Gateway is already running.
    let run = tempfile::TempDir::new().expect("tempdir");
    let child = WriterChild::spawn(run.path());

    let decision = launch_or_attach(run.path(), Duration::from_secs(5)).expect("the race settles");
    match decision {
        workshop_gateway_discovery::LaunchDecision::Attach(file) => {
            assert_eq!(file, reader_file(child.pid(), child.port));
        }
        _ => panic!("a live Gateway-written file must attach, not launch: {decision:?}"),
    }
}

#[test]
fn both_halves_serialize_the_same_bytes() {
    // A field rename on either side, or a change to one side's serde
    // attributes, changes the bytes and fails here.
    let gateway_dir = tempfile::TempDir::new().expect("tempdir");
    writer_file(4242, 8081)
        .write_to(gateway_dir.path())
        .expect("the Gateway writer writes its file");
    let from_gateway = std::fs::read_to_string(gateway_dir.path().join("gateway.json"))
        .expect("read the Gateway's bytes");

    let workshop_dir = tempfile::TempDir::new().expect("tempdir");
    reader_file(4242, 8081)
        .write_to(workshop_dir.path())
        .expect("the Workshop test double writes its file");
    let from_workshop = std::fs::read_to_string(workshop_dir.path().join("gateway.json"))
        .expect("read the Workshop bytes");

    assert_eq!(
        from_gateway, from_workshop,
        "both halves of the contract produce the same file"
    );
}

#[test]
fn both_halves_agree_on_which_files_are_invalid() {
    // The validation rule is one rule, not two: a file one half refuses for
    // its fields must be refused by the other for the same reason.
    let dir = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(
        dir.path().join("gateway.json"),
        br#"{"port":0,"api_key":"k","pid":1,"epoch":1,"version":"0","started_at":"t"}"#,
    )
    .expect("write the zero-port file");

    let gateway = gateway_api_discovery::GatewayDiscoveryFile::read(dir.path())
        .expect_err("the Gateway writer's reader refuses a zero port");
    let workshop =
        GatewayDiscoveryFile::read(dir.path()).expect_err("the Workshop reader refuses it");

    let gateway_reason = match gateway {
        gateway_api_discovery::SidecarError::Invalid { reason, .. } => reason,
        other => panic!("the Gateway names an invalid file, got {other}"),
    };
    let workshop_reason = match workshop {
        workshop_gateway_discovery::SidecarError::Invalid { reason, .. } => reason,
        other => panic!("the Workshop reader names an invalid file, got {other}"),
    };
    assert_eq!(
        gateway_reason, workshop_reason,
        "both halves name the same broken invariant"
    );
}

#[test]
fn the_reader_tolerates_a_gateway_rewrite() {
    // The Gateway rewrites gateway.json as it moves through boot. A reader
    // that raced the rewrite must never see a truncated file: the atomic
    // write is what guarantees that, and this reads the result.
    let dir = tempfile::TempDir::new().expect("tempdir");
    for epoch in [1_757_000_000_u64, 1_757_000_001, 1_757_000_002] {
        gateway_api_discovery::GatewayDiscoveryFile {
            epoch,
            ..writer_file(4242, 8081)
        }
        .write_to(dir.path())
        .expect("the Gateway rewrites its file");
        let read = GatewayDiscoveryFile::read(dir.path())
            .expect("the reader accepts a rewritten file")
            .expect("the file exists");
        assert_eq!(read.epoch, epoch, "the newest write is what reads back");
    }
}

/// The child half: writes the Gateway writer's file and answers `GET
/// /health` and the bearer probe on `GET /v1/models` until it is killed.
#[test]
#[ignore = "runs only as a named child process"]
fn the_writer_child_process() {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    let Some(run_dir) = std::env::var_os(CHILD_RUN_DIR_ENV) else {
        return;
    };
    let run_dir = PathBuf::from(run_dir);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the fixture gateway");
    let port = listener.local_addr().expect("fixture address").port();
    let file = writer_file(std::process::id(), port);
    file.write_to(&run_dir)
        .expect("the Gateway writer publishes its file");
    std::fs::write(run_dir.join(CHILD_READY), b"ready").expect("announce readiness");

    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };
        let expected_key = file.api_key.clone();
        std::thread::spawn(move || {
            // The validation proof sends both requests on one connection,
            // so the fixture keeps answering on this one until the peer
            // closes.
            let mut buffer = [0_u8; 4096];
            loop {
                let Ok(read) = stream.read(&mut buffer) else {
                    return;
                };
                if read == 0 {
                    return;
                }
                let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let health = request.starts_with("GET /health ");
                let accepted = health
                    || request.contains(&format!("Authorization: Bearer {expected_key}\r\n"));
                let response = if accepted {
                    "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}"
                } else {
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n"
                };
                if stream.write_all(response.as_bytes()).is_err() {
                    return;
                }
            }
        });
    }
}
