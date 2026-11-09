//! The `manifest` mode's arguments, here rather than in `tests.rs`, which
//! holds the other modes' cases at the file ceiling.

use std::path::PathBuf;

use crate::args::{ManifestRequest, Request, parse_arguments};

const BASE_URL: &str = "https://example.invalid/releases/download/v1";

fn arguments(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn the_manifest_mode_parses() {
    let request = parse_arguments(&arguments(&[
        "manifest",
        "--assets",
        "dist",
        "--base-url",
        BASE_URL,
        "--public-key",
        "run.key.pub",
    ]))
    .expect("parses");
    assert_eq!(
        request,
        Request::Manifest(ManifestRequest {
            assets: PathBuf::from("dist"),
            base_url: BASE_URL.to_owned(),
            public_key: Some(PathBuf::from("run.key.pub")),
        })
    );
    let request = parse_arguments(&arguments(&[
        "manifest",
        "--base-url",
        BASE_URL,
        "--assets",
        "dist",
    ]))
    .expect("parses without a key file");
    assert!(
        matches!(
            request,
            Request::Manifest(ManifestRequest {
                public_key: None,
                ..
            })
        ),
        "{request:?}"
    );
}

#[test]
fn manifest_argument_errors_are_usage_errors() {
    for (args, message) in [
        (
            &["manifest", "--base-url", BASE_URL][..],
            "`manifest` needs `--assets <dir>`",
        ),
        (
            &["manifest", "--assets", "dist"][..],
            "`manifest` needs `--base-url <url>`",
        ),
        (
            &[
                "manifest",
                "--assets",
                "a",
                "--assets",
                "b",
                "--base-url",
                BASE_URL,
            ][..],
            "duplicate argument `--assets`",
        ),
        (
            &[
                "manifest",
                "--assets",
                "dist",
                "--base-url",
                BASE_URL,
                "--sign",
            ][..],
            "unsupported argument `--sign`",
        ),
        (
            &["manifest", "--assets", "--base-url", BASE_URL][..],
            "argument `--assets` needs a directory",
        ),
    ] {
        let error = parse_arguments(&arguments(args))
            .expect_err("a usage error")
            .to_string();
        assert!(error.contains(message), "{args:?}: {error}");
        assert!(error.contains("USAGE:"), "{args:?}: {error}");
    }
}
