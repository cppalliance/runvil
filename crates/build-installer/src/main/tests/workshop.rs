//! Unit tests for the Workshop checkout check, which runs before any
//! command.

use super::installer_tests::VERSION;
use super::*;

const TOOLCHAIN_FILE: &str = "rust-toolchain.toml";

/// The toolchain file of a checkout on `channel`.
fn toolchain_file(channel: &str) -> String {
    format!("[toolchain]\nchannel = \"{channel}\"\n")
}

/// The error of a build that names no target, after requiring that it ran
/// nothing: past the check, such a build first runs `cargo -vV`.
fn refusal(environment: &BuildEnvironment) -> String {
    let mut runner = FakeRunner::default();
    let error = build_installer(
        &InstallerRequest {
            target: None,
            sign: false,
            gateway_binary: None,
        },
        environment,
        &mut runner,
    )
    .expect_err("a refused Workshop checkout");
    assert!(runner.commands.is_empty(), "{error}");
    error
}

#[test]
fn a_workshop_checkout_that_is_not_a_directory_fails_naming_it_and_the_variable() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let root = &environment.workshop_root;
    let expected = format!(
        "the Workshop checkout {} is not a directory; set RUNVIL_WORKSHOP to the root of a \
         runvil-workshop checkout",
        root.display()
    );

    std::fs::remove_dir_all(root).expect("remove the checkout");
    assert_eq!(refusal(environment), expected, "a missing directory");

    write_file(root, b"not a checkout");
    assert_eq!(refusal(environment), expected, "a file");
}

#[test]
fn a_workshop_version_that_differs_fails_naming_both_manifests() {
    let test_environment = environment();
    let environment = &test_environment.environment;
    let manifest = environment.workshop_root.join("Cargo.toml");
    write_file(&manifest, workshop_manifest("9.9.9").as_bytes());

    assert_eq!(
        refusal(environment),
        format!(
            "the Workshop checkout is not at this workspace's version: {} has \
             workspace.package.version `9.9.9`, required `{VERSION}`, the version in {}; the \
             release file names use this workspace's version and Tauri names its files with \
             Workshop's",
            manifest.display(),
            environment.workspace_root.join("Cargo.toml").display()
        )
    );
}

#[test]
fn a_workshop_toolchain_channel_that_differs_fails_naming_both_toolchain_files() {
    let mut test_environment = environment();
    // The check reads nothing else of this repository, so a workspace root
    // of the test's own lets it name both channels.
    let workspace_root = test_environment.temp.path().join("runvil");
    let ours = workspace_root.join(TOOLCHAIN_FILE);
    write_file(&ours, toolchain_file("1.99").as_bytes());
    test_environment.environment.workspace_root = workspace_root;
    let environment = &test_environment.environment;
    let theirs = environment.workshop_root.join(TOOLCHAIN_FILE);
    write_file(&theirs, toolchain_file("1.90").as_bytes());

    assert_eq!(
        refusal(environment),
        format!(
            "the Workshop checkout is not on this workspace's toolchain: {} has \
             toolchain.channel `1.90`, required `1.99`, the channel in {}; the Cargo that runs \
             the installer builds Workshop",
            theirs.display(),
            ours.display()
        )
    );
}

#[test]
fn a_checkout_file_that_is_missing_malformed_or_lacks_the_value_fails_naming_it() {
    for (file, key) in [
        ("Cargo.toml", "workspace.package.version"),
        (TOOLCHAIN_FILE, "toolchain.channel"),
    ] {
        let section = key.rsplit_once('.').expect("a dotted key").0;
        for (contents, expected) in [
            (None, "cannot read ".to_owned()),
            (Some("= not TOML".to_owned()), " is not TOML: ".to_owned()),
            (
                Some(format!("[{section}]\n")),
                format!(" has no string at {key}"),
            ),
            (
                Some(format!("[{section}]\n{} = 3\n", &key[section.len() + 1..])),
                format!(" has no string at {key}"),
            ),
        ] {
            let test_environment = environment();
            let environment = &test_environment.environment;
            let path = environment.workshop_root.join(file);
            match &contents {
                Some(contents) => write_file(&path, contents.as_bytes()),
                None => std::fs::remove_file(&path).expect("remove the file"),
            }

            let error = refusal(environment);

            assert!(error.contains(&expected), "{file} {contents:?}: {error}");
            assert!(
                error.contains(&path.display().to_string()),
                "{file} {contents:?}: {error}"
            );
        }
    }
}
