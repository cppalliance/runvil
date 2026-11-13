//! Unit tests for the installer mode's arguments. The `manifest` mode's are
//! in `manifest-args.rs`.

use super::*;

pub(super) fn arguments(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn parses_the_installer_mode_with_its_gateway_binary() {
    assert_eq!(
        parse_arguments(&[]).expect("installer"),
        Request::Installer(InstallerRequest {
            target: None,
            sign: false,
            gateway_binary: None,
        })
    );
    assert_eq!(
        parse_arguments(&arguments(&[
            "--gateway-binary",
            "gateway/runvil-gateway.exe",
            "--sign",
            "--target",
            "x86_64-pc-windows-msvc",
        ]))
        .expect("signed installer"),
        Request::Installer(InstallerRequest {
            target: Some("x86_64-pc-windows-msvc".to_owned()),
            sign: true,
            gateway_binary: Some(PathBuf::from("gateway/runvil-gateway.exe")),
        })
    );
}

/// The installer is the default mode, so the word that once chose it is
/// an argument like any other.
#[test]
fn the_installer_word_is_an_unsupported_argument() {
    for args in [
        arguments(&["installer"]),
        arguments(&["installer", "--target", "x86_64-pc-windows-msvc"]),
    ] {
        let error = parse_arguments(&args)
            .expect_err("the installer word")
            .to_string();
        assert!(
            error.starts_with("unsupported argument `installer`"),
            "{args:?}: {error}"
        );
        assert!(error.contains(args::USAGE), "{error}");
    }
}

#[test]
fn rejects_product_features_and_other_unsupported_arguments() {
    for args in [
        arguments(&["--features", "local"]),
        arguments(&["--profile", "dist"]),
        arguments(&["gateway"]),
        arguments(&["--source", "gateway"]),
        arguments(&["bundle", "--target", "x86_64-pc-windows-msvc"]),
        arguments(&["sidecar", "stage", "--target", "x86_64-pc-windows-msvc"]),
        arguments(&["--assets", "dist"]),
        arguments(&[
            "manifest",
            "--assets",
            "dist",
            "--base-url",
            "https://example.invalid",
            "--gateway-binary",
            "g",
        ]),
    ] {
        let error = parse_arguments(&args)
            .expect_err("unsupported argument")
            .to_string();
        assert!(error.contains("unsupported argument"), "{args:?}: {error}");
        assert!(error.contains(args::USAGE), "{error}");
    }
}

#[test]
fn rejects_duplicate_incomplete_or_malformed_options() {
    for args in [
        arguments(&["--sign", "--sign"]),
        arguments(&["--target"]),
        arguments(&[
            "--target",
            "x86_64-pc-windows-msvc",
            "--target",
            "x86_64-unknown-linux-gnu",
        ]),
        arguments(&["--target", "../outside"]),
        arguments(&["--target", "--sign"]),
        arguments(&["--gateway-binary"]),
        arguments(&["--gateway-binary", "a", "--gateway-binary", "b"]),
        arguments(&["--gateway-binary", "--sign"]),
    ] {
        assert!(parse_arguments(&args).is_err(), "{args:?}");
    }
}

#[test]
fn installer_release_says_the_installer_always_builds_release() {
    for args in [
        arguments(&["--release"]),
        arguments(&["--target", "x86_64-pc-windows-msvc", "--release"]),
    ] {
        let error = parse_arguments(&args)
            .expect_err("installer --release")
            .to_string();
        assert!(
            error.starts_with(
                "`cargo installer` always builds the release profile; drop `--release`"
            ),
            "{args:?}: {error}"
        );
        assert!(error.contains(args::USAGE), "{error}");
    }
}
