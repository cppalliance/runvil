//! Unit tests for the macOS Gateway bundle and the macOS installer
//! sequences.

use super::ifw_tests::{ifw_built, ifw_commands, ifw_found, installer_path};
use super::installer_tests::{
    bundle, bundled, collect_signature, expected_commands, output, request, version_printed,
};
use super::*;
use crate::installer::bundle::{self, BundleSources};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[test]
fn the_gateway_bundle_holds_the_versioned_plist_the_executable_and_the_icon() {
    let test_environment = environment();
    let temp = test_environment.temp.path();
    let gateway = temp.join("runvil-gateway");
    write_file(&gateway, b"gateway");
    let app = temp.join("Runvil Gateway.app");
    let sources = BundleSources::from_environment(&test_environment.environment);
    // The guard in `assemble` only checks that the placeholder appears, so
    // anywhere but the two version keys would defeat it.
    let template = std::fs::read_to_string(&sources.info_plist).expect("template");
    assert_eq!(template.matches("@VERSION@").count(), 2, "{template}");

    bundle::assemble(&sources, &gateway, GATEWAY_VERSION, &app).expect("assemble");

    let mut files = Vec::new();
    collect_files(&app, &app, &mut files);
    files.sort();
    assert_eq!(
        files,
        [
            "Contents/Info.plist",
            "Contents/MacOS/runvil-gateway",
            "Contents/Resources/icon.icns",
        ]
    );
    let contents = app.join("Contents");
    assert_eq!(
        std::fs::read(contents.join("Resources").join("icon.icns")).expect("icon"),
        std::fs::read(&sources.icon).expect("Workshop's icon"),
        "the icon is the Workshop checkout's"
    );
    let plist = std::fs::read_to_string(contents.join("Info.plist")).expect("Info.plist");
    assert!(
        plist.contains(&format!(
            "<key>CFBundleShortVersionString</key>\n    <string>{GATEWAY_VERSION}</string>"
        )),
        "{plist}"
    );
    assert!(
        plist.contains(&format!(
            "<key>CFBundleVersion</key>\n    <string>{GATEWAY_VERSION}</string>"
        )),
        "{plist}"
    );
    assert!(!plist.contains("@VERSION@"), "{plist}");
    assert!(
        plist.contains("<key>CFBundleExecutable</key>\n    <string>runvil-gateway</string>"),
        "{plist}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let executable = contents.join("MacOS").join("runvil-gateway");
        let mode = std::fs::metadata(executable)
            .expect("executable")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}

#[test]
fn a_template_without_the_placeholder_or_a_missing_icon_fails_naming_the_file() {
    let test_environment = environment();
    let temp = test_environment.temp.path();
    let gateway = temp.join("runvil-gateway");
    write_file(&gateway, b"gateway");
    let real = BundleSources::from_environment(&test_environment.environment);
    let fixed = temp.join("Info.plist");
    write_file(&fixed, b"<plist><string>1.0.0</string></plist>");

    let error = bundle::assemble(
        &BundleSources {
            info_plist: fixed.clone(),
            icon: real.icon.clone(),
        },
        &gateway,
        GATEWAY_VERSION,
        &temp.join("a.app"),
    )
    .expect_err("no placeholder");
    assert!(error.contains(&fixed.display().to_string()), "{error}");
    assert!(error.contains("@VERSION@"), "{error}");

    let missing = temp.join("icon.icns");
    let error = bundle::assemble(
        &BundleSources {
            info_plist: real.info_plist,
            icon: missing.clone(),
        },
        &gateway,
        GATEWAY_VERSION,
        &temp.join("b.app"),
    )
    .expect_err("no icon");
    assert!(error.contains(&missing.display().to_string()), "{error}");
}

pub(super) fn collect_files(root: &Path, directory: &Path, files: &mut Vec<String>) {
    for entry in std::fs::read_dir(directory).expect("read directory") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            collect_files(root, &path, files);
        } else {
            let relative = path.strip_prefix(root).expect("under root");
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// The payload Gateway bundle's `Info.plist`.
fn payload_plist(payload: &Path) -> String {
    std::fs::read_to_string(
        payload
            .join("Runvil Gateway.app")
            .join("Contents")
            .join("Info.plist"),
    )
    .expect("Info.plist")
}

#[test]
fn macos_signed_collects_the_payload_and_publishes_workshop_s_updater() {
    let mut test_environment = environment();
    set_signing_key(&mut test_environment.environment);
    let environment = &test_environment.environment;
    let target = "aarch64-apple-darwin";
    let staged = test_environment.stage_gateway(target);
    let macos = bundle(environment, target, "macos");
    let updater = macos.join("Runvil.app.tar.gz");
    let output_root = output(environment, target);
    let mut responses = vec![
        ifw_found(),
        bundled(
            vec![
                macos
                    .join("Runvil.app")
                    .join("Contents")
                    .join("MacOS")
                    .join("runvil-workshop"),
                collect_signature(&updater),
                updater,
            ],
            success(""),
        ),
        version_printed(),
    ];
    responses.extend(ifw_built(environment, target));
    let mut runner = FakeRunner::with_responses(responses);

    build_installer(&request(target, true), environment, &mut runner).expect("installer");

    let mut expected = expected_commands(environment, target, true, None);
    expected.extend(ifw_commands(environment, target));
    assert_eq!(runner.commands, expected);
    assert_eq!(
        runner.commands[1].args,
        strings(&["workshop", "bundle", "--target", target, "--sign"])
    );
    assert!(installer_path(environment, target).is_file());
    let payload = output_root.join("payload");
    assert!(
        payload
            .join("Runvil.app")
            .join("Contents")
            .join("MacOS")
            .join("runvil-workshop")
            .is_file()
    );
    assert_eq!(
        std::fs::read(
            payload
                .join("Runvil Gateway.app")
                .join("Contents")
                .join("MacOS")
                .join("runvil-gateway")
        )
        .expect("gateway payload"),
        b"gateway"
    );
    assert!(!payload.join("runvil-gateway").exists());
    let publish = output_root.join("publish");
    let app_updater = publish.join(format!("Runvil_{VERSION}_aarch64.app.tar.gz"));
    assert!(app_updater.is_file());
    assert!(collect_signature(&app_updater).is_file());
    assert_eq!(std::fs::read_dir(&publish).expect("publish").count(), 3);
    assert!(staged.is_file(), "a staged Gateway must outlive the build");
}

#[test]
fn macos_unsigned_publishes_only_the_dmg_around_the_pinned_gateway() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let target = "x86_64-apple-darwin";
    test_environment.stage_gateway(target);
    let contents = Path::new("Runvil.app").join("Contents");
    let executable = contents.join("MacOS").join("runvil-workshop");
    // Workshop's bundle arrives with the Plugins' libraries inside it.
    let library = contents
        .join("Resources")
        .join("plugins")
        .join("libplugin_std.dylib");
    let macos = bundle(environment, target, "macos");
    let mut responses = vec![
        ifw_found(),
        bundled(
            vec![macos.join(&executable), macos.join(&library)],
            success(""),
        ),
        version_printed(),
    ];
    responses.extend(ifw_built(environment, target));
    let mut runner = FakeRunner::with_responses(responses);

    build_installer(&request(target, false), environment, &mut runner).expect("installer");

    let mut expected = expected_commands(environment, target, false, None);
    expected.extend(ifw_commands(environment, target));
    assert_eq!(runner.commands, expected);
    assert_eq!(
        runner.commands[1].args,
        strings(&["workshop", "bundle", "--target", target])
    );
    let payload = output(environment, target).join("payload");
    // The payload holds Workshop's bundle whole, and the bundle has left
    // Tauri's directory.
    assert!(payload.join(&executable).is_file());
    assert!(payload.join(&library).is_file());
    assert!(!macos.join("Runvil.app").exists());
    // The Gateway bundle states the pin's version, not Workshop's.
    let plist = payload_plist(&payload);
    assert!(
        plist.contains(&format!("<string>{GATEWAY_VERSION}</string>")),
        "{plist}"
    );
    assert!(
        !plist.contains(&format!("<string>{VERSION}</string>")),
        "{plist}"
    );
    assert!(!payload.join("runvil-gateway").exists());
    let installer = installer_path(environment, target);
    assert_eq!(
        installer.file_name().expect("name"),
        format!("Runvil_{VERSION}_x86_64-setup.dmg").as_str()
    );
    assert!(installer.is_file());
    assert_eq!(
        std::fs::read_dir(output(environment, target).join("publish"))
            .expect("publish")
            .count(),
        1
    );
}
