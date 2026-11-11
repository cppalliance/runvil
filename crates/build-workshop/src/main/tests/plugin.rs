//! Unit tests for the std Plugin's library: the name Cargo gives it for
//! each target, where a build places it, and what a build that left none
//! says.

use super::*;
use crate::installer::Platform;
use crate::installer::ifw::{self, Sources};

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
fn a_payload_without_a_plugin_directory_is_refused_by_the_ifw_stage() {
    let temp = tempfile::tempdir().expect("temporary root");
    let payload = temp.path().join("payload");
    write_file(&payload.join("Runvil.AppImage"), b"program");
    write_file(&payload.join("runvil-gateway"), b"program");
    let platform = Platform::from_triple("x86_64-unknown-linux-gnu").expect("platform");

    let error = ifw::stage(
        &Sources::in_workspace(&repository_root()),
        &payload,
        platform,
        "2026-10-11",
        GATEWAY_VERSION,
        &temp.path().join("ifw"),
    )
    .expect_err("the payload has no plugins directory");

    assert!(
        error.contains(&payload.join("plugins").display().to_string()),
        "the error names the missing directory: {error}"
    );
}
