//! Unit tests for the std Plugin's library: the name Cargo gives it for
//! each target, where a build places it, what a build that left none
//! says, and how an installer gets it into Workshop's bundle.

use super::ifw_tests::ifw_found;
use super::installer_tests::{bundled, node_found, request, version_printed};
use super::*;
use crate::installer::System;

#[test]
fn the_library_is_named_as_cargo_names_it_for_each_target() {
    for (target, name) in [
        ("x86_64-pc-windows-msvc", "plugin_std.dll"),
        ("aarch64-pc-windows-msvc", "plugin_std.dll"),
        ("x86_64-apple-darwin", "libplugin_std.dylib"),
        ("aarch64-apple-darwin", "libplugin_std.dylib"),
        ("x86_64-unknown-linux-gnu", "libplugin_std.so"),
        ("aarch64-unknown-linux-gnu", "libplugin_std.so"),
    ] {
        assert_eq!(plugin::library_name(target), name, "{target}");
        assert_eq!(
            plugin::built_library(Path::new("profile"), target),
            Path::new("profile").join("deps").join(name),
            "{target}"
        );
    }
}

#[test]
fn a_host_build_places_the_library_in_the_plugin_directory_beside_workshop() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-pc-windows-msvc";
    let staged = test_environment.stage_gateway(triple);
    let mut runner = FakeRunner::with_responses(vec![
        success("cargo 1.89.0\nhost: x86_64-pc-windows-msvc\n"),
        workshop_built_with_sidecar(staged, success("")),
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

    let plugins = environment.target_root.join("debug").join("plugins");
    assert_eq!(
        std::fs::read(plugins.join("plugin_std.dll")).expect("the placed library"),
        plugin_bytes(None, "debug"),
        "the library of the host's debug build, and of no other"
    );
    assert_eq!(
        std::fs::read_dir(&plugins).expect("plugins").count(),
        1,
        "the copy leaves no other file behind"
    );
}

#[test]
fn a_build_for_a_named_target_places_the_library_under_that_target_and_replaces_an_old_one() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "aarch64-unknown-linux-gnu";
    test_environment.stage_gateway(triple);
    let profile = environment.target_root.join(triple).join("release");
    let placed = profile.join("plugins").join("libplugin_std.so");
    write_file(&placed, b"a library from an earlier build");
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
        std::fs::read(&placed).expect("the placed library"),
        plugin_bytes(Some(triple), "release"),
        "the library of the named target's release build, and of no other"
    );
    assert!(
        !environment
            .target_root
            .join("release")
            .join("plugins")
            .exists(),
        "nothing is placed under the host's profile"
    );
}

#[test]
fn a_build_that_left_no_library_fails_naming_the_file_and_places_nothing() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "x86_64-unknown-linux-gnu";
    test_environment.stage_gateway(triple);
    test_environment.lose_plugin(triple);
    let profile = environment.target_root.join(triple).join("debug");
    let mut runner =
        FakeRunner::with_responses(vec![workshop_built_into(profile.clone(), success(""))]);

    let error = build_workshop(
        &BuildRequest {
            profile: Profile::Debug,
            target: Some(triple.to_owned()),
            gateway_binary: None,
        },
        environment,
        &mut runner,
    )
    .expect_err("the library is missing");

    let library = plugin::built_library(&profile, triple);
    assert_eq!(
        error.to_string(),
        format!(
            "cannot place the std Plugin beside Workshop: {} is missing; a Workshop build \
             writes it, because the server depends on workshop-plugin-std",
            library.display()
        )
    );
    assert!(!profile.join("plugins").exists());
    assert!(
        !profile.join("runvil-gateway").exists(),
        "the Gateway is placed only after the Plugin"
    );
}

#[test]
fn the_bundle_configuration_copies_the_library_into_each_platform_s_bundle() {
    let dylib = Path::new("release")
        .join("deps")
        .join("libplugin_std.dylib");
    assert_eq!(
        plugin::bundle_config(System::MacOs, &dylib),
        Ok(Some(
            serde_json::json!({
                "bundle": { "macOS": { "files": {
                    "Resources/plugins/libplugin_std.dylib": dylib
                } } }
            })
            .to_string()
        )),
        "a macOS path runs from the bundle's Contents"
    );
    let so = Path::new("release").join("deps").join("libplugin_std.so");
    assert_eq!(
        plugin::bundle_config(System::Linux, &so),
        Ok(Some(
            serde_json::json!({
                "bundle": { "linux": { "appimage": { "files": {
                    "/usr/lib/runvil/plugins/libplugin_std.so": so
                } } } }
            })
            .to_string()
        )),
        "an AppImage path runs from the root of the AppDir"
    );
    let dll = Path::new("release").join("deps").join("plugin_std.dll");
    assert_eq!(
        plugin::bundle_config(System::Windows, &dll),
        Ok(None),
        "the Windows setup installs the library itself"
    );
}

#[test]
fn a_bundle_tauri_left_without_the_library_fails_the_installer_and_collects_nothing() {
    for (target, directory, tree) in [
        ("aarch64-apple-darwin", "macos", "Runvil.app"),
        ("x86_64-unknown-linux-gnu", "appimage", "Runvil.AppDir"),
    ] {
        let test_environment = environment();
        let environment = &test_environment.environment;
        let staged = test_environment.stage_gateway(target);
        let bundle = environment
            .target_root
            .join(target)
            .join("release")
            .join("bundle")
            .join(directory);
        // The bundle is whole except for the library.
        let built = if directory == "macos" {
            bundle
                .join(tree)
                .join("Contents")
                .join("MacOS")
                .join("runvil-workshop")
        } else {
            bundle.join("Runvil_0.0.0_amd64.AppImage")
        };
        let mut runner = FakeRunner::with_responses(vec![
            ifw_found(),
            node_found(),
            version_printed(),
            bundled(staged, vec![built], success("")),
        ]);

        let error = build_installer(&request(target, false), environment, &mut runner)
            .expect_err("the bundle has no library");

        assert!(
            error
                .primary
                .starts_with("the Workshop bundle lacks the std Plugin's library: "),
            "{target}: {error}"
        );
        assert!(
            error.primary.contains(tree) && error.primary.contains("libplugin_std."),
            "{target}: the error names the missing file: {error}"
        );
        assert!(
            !environment.target_root.join("installer").exists(),
            "{target}: nothing is collected"
        );
    }
}

#[test]
fn a_windows_bundle_is_not_checked_for_a_library_inside_it() {
    let bundle = Path::new("release").join("bundle").join("nsis");
    assert_eq!(
        plugin::check_bundled(System::Windows, &bundle, "plugin_std.dll"),
        Ok(())
    );
}
