//! Unit tests for the sidecar's name, staging, and removal, the Gateway's
//! source, and the pinned version.

use super::*;
use sidecar::GatewaySource;

#[test]
fn names_the_sidecar_with_tauri_s_target_suffix() {
    let root = Path::new("repo");
    let binaries = root
        .join("crates")
        .join("workshop")
        .join("desktop")
        .join("binaries");
    for (target, binary, staged) in [
        (
            "x86_64-pc-windows-msvc",
            "runvil-gateway.exe",
            "runvil-gateway-x86_64-pc-windows-msvc.exe",
        ),
        (
            "x86_64-pc-windows-gnu",
            "runvil-gateway.exe",
            "runvil-gateway-x86_64-pc-windows-gnu.exe",
        ),
        (
            "x86_64-unknown-linux-gnu",
            "runvil-gateway",
            "runvil-gateway-x86_64-unknown-linux-gnu",
        ),
        (
            "aarch64-unknown-linux-gnu",
            "runvil-gateway",
            "runvil-gateway-aarch64-unknown-linux-gnu",
        ),
        (
            "aarch64-apple-darwin",
            "runvil-gateway",
            "runvil-gateway-aarch64-apple-darwin",
        ),
        (
            "x86_64-apple-darwin",
            "runvil-gateway",
            "runvil-gateway-x86_64-apple-darwin",
        ),
    ] {
        assert_eq!(sidecar::gateway_binary_name(target), binary, "{target}");
        assert_eq!(
            sidecar::sidecar_path(root, target),
            binaries.join(staged),
            "{target}"
        );
    }
}

#[test]
fn stages_a_copy_and_removes_it() {
    let temp = tempfile::tempdir().expect("temporary root");
    let root = temp.path().join("repo");
    let source = temp.path().join("build").join("runvil-gateway");
    write_file(&source, b"gateway bytes");
    let target = "x86_64-unknown-linux-gnu";

    let staged = sidecar::stage(&root, target, &source).expect("stage");

    assert_eq!(staged, sidecar::sidecar_path(&root, target));
    assert_eq!(std::fs::read(&staged).expect("staged"), b"gateway bytes");
    assert!(source.is_file(), "staging moved the source");
    assert_eq!(sidecar::remove(&root, target).expect("remove"), staged);
    assert!(!staged.exists());
}

#[cfg(unix)]
#[test]
fn staging_keeps_the_execute_bit() {
    use std::os::unix::fs::PermissionsExt as _;

    let temp = tempfile::tempdir().expect("temporary root");
    let source = temp.path().join("runvil-gateway");
    write_file(&source, b"gateway");
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o755))
        .expect("make executable");

    let staged =
        sidecar::stage(&temp.path().join("repo"), "aarch64-apple-darwin", &source).expect("stage");

    let mode = std::fs::metadata(staged)
        .expect("staged")
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "{mode:o}");
}

#[test]
fn removing_an_absent_sidecar_succeeds() {
    let temp = tempfile::tempdir().expect("temporary root");
    let target = "x86_64-pc-windows-msvc";

    let removed = sidecar::remove(temp.path(), target).expect("remove");

    assert_eq!(removed, sidecar::sidecar_path(temp.path(), target));
}

#[test]
fn refuses_a_missing_source_a_directory_and_a_wrong_name() {
    let temp = tempfile::tempdir().expect("temporary root");
    let root = temp.path().join("repo");
    let target = "x86_64-pc-windows-msvc";
    let missing = temp.path().join("runvil-gateway.exe");
    let directory = temp.path().join("dir").join("runvil-gateway.exe");
    std::fs::create_dir_all(&directory).expect("directory");
    let misnamed = temp.path().join("runvil-gateway");
    write_file(&misnamed, b"gateway");

    for (source, expected) in [
        (&missing, "does not exist"),
        (&directory, "is not a file"),
        (
            &misnamed,
            "must be named runvil-gateway.exe for x86_64-pc-windows-msvc",
        ),
    ] {
        let error = sidecar::stage(&root, target, source).expect_err("refused source");
        assert!(error.contains(expected), "{error}");
        assert!(error.contains(&source.display().to_string()), "{error}");
    }
    assert!(!sidecar::sidecar_path(&root, target).exists());
}

#[test]
fn only_windows_targets_bundle_the_sidecar() {
    assert!(sidecar::bundles_sidecar("x86_64-pc-windows-msvc"));
    assert!(sidecar::bundles_sidecar("aarch64-pc-windows-msvc"));
    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ] {
        assert!(!sidecar::bundles_sidecar(target), "{target}");
    }
}

#[test]
fn a_given_gateway_wins_over_the_slot_and_the_marker_says_whether_it_is_a_stub() {
    let test_environment = environment();
    let root = &test_environment.environment.workspace_root;
    let target = "aarch64-apple-darwin";
    let given = Path::new("elsewhere").join("runvil-gateway");

    assert_eq!(
        sidecar::gateway_source(Some(&given), root, target),
        Ok(GatewaySource::Given(given.clone()))
    );
    let error = sidecar::gateway_source(None, root, target).expect_err("nothing staged");
    assert!(
        error.contains(&format!(
            "node tools/fetch-gateway-sidecar.mjs --target {target}"
        )),
        "{error}"
    );

    let slot = test_environment.stage_gateway(target);
    assert_eq!(
        sidecar::gateway_source(None, root, target),
        Ok(GatewaySource::Staged { stub: false })
    );
    assert_eq!(
        GatewaySource::Staged { stub: false }.binary(root, target),
        slot
    );
    assert_eq!(
        sidecar::gateway_source(Some(&given), root, target),
        Ok(GatewaySource::Given(given))
    );
    test_environment.stage_stub(target);
    assert_eq!(
        sidecar::gateway_source(None, root, target),
        Ok(GatewaySource::Staged { stub: true })
    );
    // A file some other step staged, with no marker, is a real Gateway.
    let mut marker = slot.into_os_string();
    marker.push(".pinned");
    std::fs::remove_file(marker).expect("remove the marker");
    assert_eq!(
        sidecar::gateway_source(None, root, target),
        Ok(GatewaySource::Staged { stub: false })
    );
}

#[test]
fn the_pinned_version_is_the_pin_s_version_and_a_placeholder_or_missing_pin_fails() {
    let test_environment = environment();
    let root = &test_environment.environment.workspace_root;
    assert_eq!(
        sidecar::pinned_version(root),
        Ok(GATEWAY_VERSION.to_owned())
    );

    let pin = root.join("gateway-pin").join("pin.json");
    for (contents, expected) in [
        (r#"{"version": "PLACEHOLDER"}"#, "names no version"),
        (r#"{"version": ""}"#, "names no version"),
        (r#"{"tag": "gateway-v1.0.0"}"#, "names no version"),
        ("not json", "is not JSON"),
    ] {
        write_file(&pin, contents.as_bytes());
        let error = sidecar::pinned_version(root).expect_err(contents);
        assert!(error.contains(expected), "{contents}: {error}");
        assert!(error.contains(&pin.display().to_string()), "{error}");
    }
    std::fs::remove_file(&pin).expect("remove the pin");
    let error = sidecar::pinned_version(root).expect_err("missing pin");
    assert!(error.contains("cannot read the Gateway pin"), "{error}");
}
