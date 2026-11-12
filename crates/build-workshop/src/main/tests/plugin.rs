//! Unit tests for the libraries of the Plugins Workshop ships: the name
//! Cargo gives each for each target, where a build places them, what a
//! build that left one out says, and how an installer gets them into
//! Workshop's bundle.

use super::ifw_tests::ifw_found;
use super::installer_tests::{bundled, bundled_plugins, node_found, request, version_printed};
use super::*;
use crate::installer::System;

#[test]
fn each_library_is_named_as_cargo_names_it_for_each_target() {
    for (target, names) in [
        (
            "x86_64-pc-windows-msvc",
            ["plugin_std.dll", "plugin_bashkit.dll"],
        ),
        (
            "aarch64-pc-windows-msvc",
            ["plugin_std.dll", "plugin_bashkit.dll"],
        ),
        (
            "x86_64-apple-darwin",
            ["libplugin_std.dylib", "libplugin_bashkit.dylib"],
        ),
        (
            "aarch64-apple-darwin",
            ["libplugin_std.dylib", "libplugin_bashkit.dylib"],
        ),
        (
            "x86_64-unknown-linux-gnu",
            ["libplugin_std.so", "libplugin_bashkit.so"],
        ),
        (
            "aarch64-unknown-linux-gnu",
            ["libplugin_std.so", "libplugin_bashkit.so"],
        ),
    ] {
        assert_eq!(plugin::library_names(target), names, "{target}");
        assert_eq!(
            plugin::built_libraries(Path::new("profile"), target),
            names.map(|name| Path::new("profile").join("deps").join(name)),
            "{target}"
        );
    }
}

#[test]
fn a_host_build_places_each_library_in_the_plugin_directory_beside_workshop() {
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
    for library in ["plugin_std", "plugin_bashkit"] {
        assert_eq!(
            std::fs::read(plugins.join(format!("{library}.dll"))).expect("the placed library"),
            plugin_bytes(library, None, "debug"),
            "the library of the host's debug build, and of no other"
        );
    }
    assert_eq!(
        std::fs::read_dir(&plugins).expect("plugins").count(),
        2,
        "the copies leave no other file behind"
    );
}

#[test]
fn a_build_for_a_named_target_places_each_library_under_that_target_and_replaces_an_old_one() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let triple = "aarch64-unknown-linux-gnu";
    test_environment.stage_gateway(triple);
    let profile = environment.target_root.join(triple).join("release");
    let plugins = profile.join("plugins");
    write_file(
        &plugins.join("libplugin_std.so"),
        b"a library from an earlier build",
    );
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

    for library in ["plugin_std", "plugin_bashkit"] {
        assert_eq!(
            std::fs::read(plugins.join(format!("lib{library}.so"))).expect("the placed library"),
            plugin_bytes(library, Some(triple), "release"),
            "the library of the named target's release build, and of no other"
        );
    }
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
fn a_build_that_left_a_library_out_fails_naming_the_file_and_places_nothing() {
    for (lost, file) in [
        ("plugin_std", "libplugin_std.so"),
        ("plugin_bashkit", "libplugin_bashkit.so"),
    ] {
        let test_environment = environment();
        let environment = &test_environment.environment;
        let triple = "x86_64-unknown-linux-gnu";
        test_environment.stage_gateway(triple);
        test_environment.lose_plugin(triple, lost);
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
        .expect_err("a library is missing");

        assert_eq!(
            error.to_string(),
            format!(
                "cannot place Workshop's Plugins beside it: {} is missing; a Workshop build \
                 writes it, because the server depends on the `workshop-plugin-*` crate that \
                 exports it",
                profile.join("deps").join(file).display()
            ),
            "{lost}"
        );
        assert!(
            !profile.join("plugins").exists(),
            "{lost}: the library that was built is not placed alone"
        );
        assert!(
            !profile.join("runvil-gateway").exists(),
            "{lost}: the Gateway is placed only after the Plugins"
        );
    }
}

#[test]
fn the_bundle_configuration_copies_each_library_into_each_platform_s_bundle() {
    let deps = Path::new("release").join("deps");
    let dylibs = [
        deps.join("libplugin_std.dylib"),
        deps.join("libplugin_bashkit.dylib"),
    ];
    assert_eq!(
        plugin::bundle_config(System::MacOs, &dylibs),
        Ok(Some(
            serde_json::json!({
                "bundle": { "macOS": { "files": {
                    "Resources/plugins/libplugin_std.dylib": dylibs[0],
                    "Resources/plugins/libplugin_bashkit.dylib": dylibs[1],
                } } }
            })
            .to_string()
        )),
        "a macOS path runs from the bundle's Contents"
    );
    let sos = [
        deps.join("libplugin_std.so"),
        deps.join("libplugin_bashkit.so"),
    ];
    assert_eq!(
        plugin::bundle_config(System::Linux, &sos),
        Ok(Some(
            serde_json::json!({
                "bundle": { "linux": { "appimage": { "files": {
                    "/usr/lib/runvil/plugins/libplugin_std.so": sos[0],
                    "/usr/lib/runvil/plugins/libplugin_bashkit.so": sos[1],
                } } } }
            })
            .to_string()
        )),
        "an AppImage path runs from the root of the AppDir"
    );
    let dlls = [deps.join("plugin_std.dll"), deps.join("plugin_bashkit.dll")];
    assert_eq!(
        plugin::bundle_config(System::Windows, &dlls),
        Ok(None),
        "the Windows setup installs the libraries itself"
    );
}

#[test]
fn a_bundle_tauri_left_without_a_library_fails_the_installer_and_collects_nothing() {
    for (target, directory, tree) in [
        ("aarch64-apple-darwin", "macos", "Runvil.app"),
        ("x86_64-unknown-linux-gnu", "appimage", "Runvil.AppDir"),
    ] {
        // The bundle is whole except for the library `lacks` names: both
        // of them, and then the second alone.
        for (kept, lacks) in [(0, "libplugin_std."), (1, "libplugin_bashkit.")] {
            let test_environment = environment();
            let environment = &test_environment.environment;
            let staged = test_environment.stage_gateway(target);
            let bundle = environment
                .target_root
                .join(target)
                .join("release")
                .join("bundle")
                .join(directory);
            let built = if directory == "macos" {
                bundle
                    .join(tree)
                    .join("Contents")
                    .join("MacOS")
                    .join("runvil-workshop")
            } else {
                bundle.join("Runvil_0.0.0_amd64.AppImage")
            };
            let mut files = vec![built];
            files.extend(bundled_plugins(environment, target).into_iter().take(kept));
            let mut runner = FakeRunner::with_responses(vec![
                ifw_found(),
                node_found(),
                version_printed(),
                bundled(staged, files, success("")),
            ]);

            let error = build_installer(&request(target, false), environment, &mut runner)
                .expect_err("the bundle lacks a library");

            assert!(
                error
                    .primary
                    .starts_with("the Workshop bundle lacks a Plugin's library: "),
                "{target}: {error}"
            );
            assert!(
                error.primary.contains(tree) && error.primary.contains(lacks),
                "{target}: the error names the missing file: {error}"
            );
            assert!(
                !environment.target_root.join("installer").exists(),
                "{target}: nothing is collected"
            );
        }
    }
}

#[test]
fn a_windows_bundle_is_not_checked_for_a_library_inside_it() {
    let bundle = Path::new("release").join("bundle").join("nsis");
    assert_eq!(
        plugin::check_bundled(
            System::Windows,
            &bundle,
            &plugin::library_names("x86_64-pc-windows-msvc")
        ),
        Ok(())
    );
}
