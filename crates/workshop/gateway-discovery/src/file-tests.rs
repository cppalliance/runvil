//! The `gateway.json` reader and its golden fixtures: the six fields, the
//! tolerated unknown field, the boot identity, and validation.

use super::*;

/// A canonical valid file, matching `fixtures/gateway.json`.
fn valid_file() -> GatewayDiscoveryFile {
    GatewayDiscoveryFile {
        port: 8081,
        api_key: "0e1f8f0d8f8c4e0f9a1c2b3d4e5f6a7b".to_owned(),
        pid: 4242,
        epoch: 1_757_000_000,
        version: "0.2.0".to_owned(),
        started_at: "2026-09-03T12:00:00Z".to_owned(),
    }
}

/// One committed contract fixture's bytes.
fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read the contract fixture {}: {error}", path.display()))
}

/// Writes one fixture into a fresh run directory and parses it back.
fn parse_fixture(name: &str) -> Result<Option<GatewayDiscoveryFile>, SidecarError> {
    let dir = tempfile::TempDir::new().expect("tempdir");
    fs::write(
        crate::paths::gateway_discovery_file_path(dir.path()),
        fixture(name),
    )
    .expect("write the fixture");
    GatewayDiscoveryFile::read(dir.path())
}

#[test]
fn the_golden_fixture_parses_to_the_canonical_file() {
    let parsed = parse_fixture("gateway.json").expect("the golden fixture is valid");
    assert_eq!(parsed, Some(valid_file()));
}

#[test]
#[cfg(feature = "test-fixtures")]
fn the_writer_reproduces_the_golden_fixture_byte_for_byte() {
    // The committed bytes are the drift alarm for the serialization: a
    // change to the field order, the pretty-printing, or a field name
    // shows up here rather than in a Gateway that no longer attaches. The
    // writer emits no trailing newline and the committed file carries one
    // because a text file should end in a newline, so the comparison is
    // against the fixture without it.
    let dir = tempfile::TempDir::new().expect("tempdir");
    valid_file().write_to(dir.path()).expect("write");
    let written =
        fs::read_to_string(crate::paths::gateway_discovery_file_path(dir.path())).expect("read");
    let golden = fixture("gateway.json");
    assert_eq!(
        written,
        golden.trim_end_matches('\n'),
        "the writer's bytes are the committed contract bytes"
    );
}

#[test]
fn an_unknown_field_is_tolerated() {
    let parsed = parse_fixture("gateway-unknown-field.json")
        .expect("a newer Gateway's extra field must not break this reader");
    assert_eq!(
        parsed,
        Some(valid_file()),
        "the known fields read unchanged"
    );
}

#[test]
fn a_file_without_a_boot_identity_parses_but_carries_none() {
    let parsed = parse_fixture("gateway-no-boot-identity.json")
        .expect("valid fields parse")
        .expect("the file exists");
    assert!(
        !parsed.has_boot_identity(),
        "a zero epoch and a blank start time is no boot identity"
    );
}

#[test]
fn a_zero_port_is_a_validation_error() {
    let error =
        parse_fixture("gateway-invalid-port.json").expect_err("a zero port names no endpoint");
    let SidecarError::Invalid { reason, .. } = &error else {
        panic!("a zero port is a validation error, got {error}");
    };
    assert_eq!(reason, "port must not be 0");
}

#[test]
fn the_field_names_and_types_are_the_contract() {
    // Pins the wire names and types independently of the fixture file, so
    // a rename or a type change fails here even if nobody updates the
    // fixture. The future OpenAPI spec describes the same two fields for
    // the admin status route; this is the discovery file's six.
    let value = serde_json::to_value(valid_file()).expect("the file is representable");
    let object = value.as_object().expect("the file is a JSON object");
    let mut names: Vec<&str> = object.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["api_key", "epoch", "pid", "port", "started_at", "version"],
        "the contract names exactly these six fields"
    );
    assert!(object["port"].is_u64(), "port is an integer");
    assert!(object["api_key"].is_string(), "api_key is a string");
    assert!(object["pid"].is_u64(), "pid is an integer");
    assert!(object["epoch"].is_u64(), "epoch is an integer");
    assert!(object["version"].is_string(), "version is a string");
    assert!(object["started_at"].is_string(), "started_at is a string");
}

#[test]
fn validation_rejects_the_three_attach_fields() {
    let mut file = valid_file();
    assert_eq!(file.validation_error(), None);
    file.port = 0;
    assert_eq!(file.validation_error(), Some("port must not be 0"));
    file = valid_file();
    file.api_key.clear();
    assert_eq!(file.validation_error(), Some("api_key must not be empty"));
    file = valid_file();
    file.api_key = "key\r\ninjected: value".to_owned();
    assert_eq!(
        file.validation_error(),
        Some("api_key must not contain line breaks")
    );
    file = valid_file();
    file.pid = 0;
    assert_eq!(file.validation_error(), Some("pid must not be 0"));
}

#[test]
fn debug_redacts_bearer_and_untrusted_metadata() {
    let secret = "capability-secret";
    let mut file = valid_file();
    file.api_key = secret.to_owned();
    file.version = format!("version-{secret}\r\n");
    file.started_at = format!("started-{secret}\t");

    let debug = format!("{file:?}");
    assert!(!debug.contains(secret), "debug output redacts the bearer");
    assert!(
        !debug.contains(['\r', '\n', '\t']),
        "untrusted metadata cannot inject debug output"
    );
    assert!(
        debug.contains(&file.port.to_string()),
        "the endpoint remains visible"
    );
}

#[test]
fn read_returns_none_when_no_file_exists() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    assert_eq!(GatewayDiscoveryFile::read(dir.path()).expect("read"), None);
}

#[test]
fn read_rejects_corrupt_json() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    fs::write(
        crate::paths::gateway_discovery_file_path(dir.path()),
        b"not json",
    )
    .expect("write fixture");
    let error = GatewayDiscoveryFile::read(dir.path()).expect_err("corrupt JSON must fail");
    assert!(
        matches!(error, SidecarError::Parse { .. }),
        "a corrupt file is a parse error: {error}"
    );
}

#[test]
fn read_rejects_an_unreadable_path() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    fs::create_dir(crate::paths::gateway_discovery_file_path(dir.path()))
        .expect("a directory in the file's place");
    let error = GatewayDiscoveryFile::read(dir.path()).expect_err("a directory is not a file");
    assert!(
        matches!(error, SidecarError::Read { .. }),
        "an unreadable path is a read error, not staleness: {error}"
    );
}

#[test]
#[cfg(feature = "test-fixtures")]
fn write_refuses_an_invalid_file() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let mut file = valid_file();
    file.api_key.clear();
    let error = file
        .write_to(dir.path())
        .expect_err("an invalid file must not be written");
    assert!(matches!(error, SidecarError::Invalid { .. }));
    assert!(
        !crate::paths::gateway_discovery_file_path(dir.path()).exists(),
        "nothing was written"
    );
}

#[test]
#[cfg(feature = "test-fixtures")]
fn write_creates_a_missing_run_directory() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let run_dir = dir.path().join("nested").join("run");
    valid_file().write_to(&run_dir).expect("write");
    assert!(crate::paths::gateway_discovery_file_path(&run_dir).exists());
}

#[test]
#[cfg(feature = "test-fixtures")]
fn remove_if_mine_removes_only_the_owning_pid() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let file = valid_file();
    file.write_to(dir.path()).expect("write");
    let path = crate::paths::gateway_discovery_file_path(dir.path());

    remove_if_mine(dir.path(), 9999).expect("a foreign pid is tolerated");
    assert!(path.exists(), "a foreign pid's removal spares the file");

    remove_if_mine(dir.path(), file.pid).expect("the owning pid removes");
    assert!(!path.exists(), "the owning pid's removal deletes the file");
}

#[test]
#[cfg(feature = "test-fixtures")]
fn remove_if_mine_tolerates_a_missing_file() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    remove_if_mine(dir.path(), 4242).expect("a missing file is not an error");
}
