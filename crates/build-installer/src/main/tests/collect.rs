//! Unit tests for the installer's platform table, bundle collection, and the
//! installed names Workshop's Gateway lookup shares.

use super::*;
use crate::installer::bundle::BundleSources;
use crate::installer::collect;
use crate::installer::{Arch, Gateway, Platform, System};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const WINDOWS: Platform = Platform {
    system: System::Windows,
    arch: Arch::X86_64,
};

#[test]
fn maps_each_supported_triple_to_its_platform() {
    for (target, system, arch) in [
        ("x86_64-pc-windows-msvc", System::Windows, Arch::X86_64),
        ("aarch64-pc-windows-msvc", System::Windows, Arch::Aarch64),
        ("aarch64-apple-darwin", System::MacOs, Arch::Aarch64),
        ("x86_64-apple-darwin", System::MacOs, Arch::X86_64),
        ("x86_64-unknown-linux-gnu", System::Linux, Arch::X86_64),
        ("aarch64-unknown-linux-gnu", System::Linux, Arch::Aarch64),
    ] {
        assert_eq!(
            Platform::from_triple(target),
            Ok(Platform { system, arch }),
            "{target}"
        );
    }
    for target in [
        "x86_64-pc-windows-gnu",
        "x86_64-unknown-linux-musl",
        "i686-pc-windows-msvc",
        "x86_64-unknown-freebsd",
        "x86_64-uwp-windows-msvc",
        "aarch64-apple-ios",
        "x86_64-unknown-linux-gnux32",
    ] {
        let error = Platform::from_triple(target).expect_err(target);
        assert!(error.contains(target), "{error}");
    }
}

#[test]
fn a_missing_or_duplicated_bundle_file_names_the_glob_and_directory() {
    let temp = tempfile::tempdir().expect("temporary root");
    let release = temp.path().join("release");
    let nsis = release.join("bundle").join("nsis");
    std::fs::create_dir_all(&nsis).expect("bundle directory");

    let error = collect::collect(&release, WINDOWS, false, &temp.path().join("out"))
        .expect_err("missing setup");
    assert!(error.contains("`*-setup.exe`"), "{error}");
    assert!(error.contains(&nsis.display().to_string()), "{error}");
    assert!(error.contains("found 0"), "{error}");

    write_file(&nsis.join("a-setup.exe"), b"a");
    write_file(&nsis.join("b-setup.exe"), b"b");
    let error = collect::collect(&release, WINDOWS, false, &temp.path().join("out"))
        .expect_err("duplicate setup");
    assert!(error.contains("found 2"), "{error}");
}

#[test]
fn a_missing_signature_under_sign_names_the_file() {
    let temp = tempfile::tempdir().expect("temporary root");
    let release = temp.path().join("release");
    let setup = release
        .join("bundle")
        .join("nsis")
        .join(format!("Runvil_{VERSION}_x64-setup.exe"));
    write_file(&setup, b"setup");

    let error = collect::collect(&release, WINDOWS, true, &temp.path().join("out"))
        .expect_err("missing signature");

    assert!(
        error.contains(&collect::signature_path(&setup).display().to_string()),
        "{error}"
    );
    assert!(
        error.contains("missing from the Tauri bundle output"),
        "{error}"
    );
}

#[test]
fn collection_replaces_an_earlier_output() {
    let temp = tempfile::tempdir().expect("temporary root");
    let release = temp.path().join("release");
    let output = temp.path().join("out");
    write_file(&output.join("publish").join("stale.exe"), b"stale");
    write_file(
        &release
            .join("bundle")
            .join("nsis")
            .join(format!("Runvil_{VERSION}_x64-setup.exe")),
        b"setup",
    );

    collect::collect(&release, WINDOWS, false, &output).expect("collect");

    assert!(!output.join("publish").join("stale.exe").exists());
}

/// A Tauri macOS bundle output; with `sign`, also Workshop's updater
/// archive and its signature.
fn macos_release(release: &Path, sign: bool) {
    let bundle = release.join("bundle").join("macos");
    write_file(
        &bundle
            .join("Runvil.app")
            .join("Contents")
            .join("MacOS")
            .join("runvil-workshop"),
        b"workshop",
    );
    if sign {
        write_file(&bundle.join("Runvil.app.tar.gz"), b"app archive");
        write_file(&bundle.join("Runvil.app.tar.gz.sig"), b"app signature");
    }
}

/// Collects `release` for a macOS `arch` into `output` and places a
/// Gateway with the test pin's version in the payload, as the installer
/// build does.
fn collect_macos(test_environment: &TestEnvironment, release: &Path, arch: Arch, sign: bool) {
    let platform = Platform {
        system: System::MacOs,
        arch,
    };
    let output = test_environment.temp.path().join("out");
    collect::collect(release, platform, sign, &output).expect("collect");
    collect::place_gateway(
        &BundleSources::from_environment(&test_environment.environment),
        &Gateway {
            binary: test_environment.given_gateway(),
            version: GATEWAY_VERSION.to_owned(),
        },
        platform,
        &output.join("payload"),
    )
    .expect("place the Gateway");
}

#[test]
fn macos_payload_holds_the_two_bundles_with_the_gateway_s_version() {
    let test_environment = environment();
    let release = test_environment.temp.path().join("release");
    let output = test_environment.temp.path().join("out");
    macos_release(&release, true);

    collect_macos(&test_environment, &release, Arch::Aarch64, true);

    let payload = output.join("payload");
    let mut names: Vec<_> = std::fs::read_dir(&payload)
        .expect("payload")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["Runvil Gateway.app", "Runvil.app"]);
    let contents = payload.join("Runvil Gateway.app").join("Contents");
    assert_eq!(
        std::fs::read(contents.join("MacOS").join("runvil-gateway")).expect("executable"),
        b"given gateway"
    );
    let plist = std::fs::read_to_string(contents.join("Info.plist")).expect("Info.plist");
    assert!(
        plist.contains(&format!(
            "<key>CFBundleVersion</key>\n    <string>{GATEWAY_VERSION}</string>"
        )),
        "{plist}"
    );
    // Only Workshop's updater is published; the Gateway has its own releases.
    let mut published: Vec<_> = std::fs::read_dir(output.join("publish"))
        .expect("publish")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    published.sort();
    let updater = format!("Runvil_{VERSION}_aarch64.app.tar.gz");
    assert_eq!(published, [updater.clone(), format!("{updater}.sig")]);
}

#[test]
fn macos_unsigned_payload_publishes_nothing() {
    let test_environment = environment();
    let release = test_environment.temp.path().join("release");
    let output = test_environment.temp.path().join("out");
    macos_release(&release, false);

    collect_macos(&test_environment, &release, Arch::X86_64, false);

    let payload = output.join("payload");
    assert!(payload.join("Runvil Gateway.app").is_dir());
    assert!(!payload.join("runvil-gateway").exists());
    assert_eq!(
        std::fs::read_dir(output.join("publish"))
            .expect("publish")
            .count(),
        0
    );
}

/// The string constant `name` declares in Rust source `text`.
fn declared(text: &str, name: &str) -> String {
    let start = text
        .find(&format!("pub const {name}: &str = \""))
        .unwrap_or_else(|| panic!("no `{name}` constant"));
    let value = &text[start..];
    let value = &value[value.find('"').expect("open quote") + 1..];
    value[..value.find('"').expect("close quote")].to_owned()
}

/// The one test that reads the real Workshop checkout, the one
/// `RUNVIL_WORKSHOP` names when the test runs. Cargo's configuration in
/// this repository sets the variable to the checkout next to the
/// repository unless it is already set.
#[test]
fn the_installed_names_match_workshop_s_gateway_lookup() {
    let variable = workshop::WORKSHOP_VARIABLE;
    let checkout = std::env::var_os(variable).unwrap_or_else(|| {
        panic!("{variable} is unset; set it to the root of a runvil-workshop checkout")
    });
    let peer = PathBuf::from(checkout)
        .join("crates")
        .join("workshop")
        .join("gateway-discovery")
        .join("src")
        .join("peer.rs");
    let text = std::fs::read_to_string(&peer).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}; set {variable} to the root of a runvil-workshop checkout",
            peer.display()
        )
    });
    for (name, ours) in [
        ("WORKSHOP_BUNDLE_NAME", collect::WORKSHOP_BUNDLE_NAME),
        ("GATEWAY_BUNDLE_NAME", collect::GATEWAY_BUNDLE_NAME),
        ("WORKSHOP_APPIMAGE_NAME", collect::WORKSHOP_APPIMAGE_NAME),
    ] {
        assert_eq!(declared(&text, name), ours, "{name} in {}", peer.display());
    }
    // The lookup joins these onto the directories above; the payload must
    // use the same Gateway file name and bundle folders.
    for needle in [
        format!("Self::MacOs | Self::Linux => \"{}\"", collect::GATEWAY),
        "join(\"Contents\").join(\"MacOS\")".to_owned(),
    ] {
        assert!(text.contains(&needle), "`{needle}` in {}", peer.display());
    }
}
