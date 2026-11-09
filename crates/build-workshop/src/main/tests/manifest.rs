//! The `manifest` mode: the release file set, the signature checks,
//! `latest.json`, `SHA256SUMS`, and the date. Its arguments are tested in
//! `manifest-args.rs`.

use std::fmt::Write as _;
use std::path::PathBuf;

use tempfile::TempDir;

use super::*;

#[path = "manifest-args.rs"]
mod args;

const BASE_URL: &str = "https://example.invalid/releases/download/v1";

fn key_pair() -> minisign::KeyPair {
    minisign::KeyPair::generate_unencrypted_keypair().expect("generate a key pair")
}

/// The public key as `tauri.conf.json` and `tauri signer generate` write it.
fn public(pair: &minisign::KeyPair) -> String {
    STANDARD.encode(pair.pk.to_box().expect("box the key").into_string())
}

/// A signature as the Tauri CLI writes a `.sig` file.
fn sign(pair: &minisign::KeyPair, data: &[u8], trusted: &str) -> String {
    let signature =
        minisign::sign(Some(&pair.pk), &pair.sk, data, Some(trusted), None).expect("sign");
    STANDARD.encode(signature.into_string())
}

fn release_comment(file: &str) -> String {
    format!("timestamp:1\tfile:{file}\tversion:{VERSION}")
}

/// A complete release signed with `pair`, beside a workspace whose
/// `tauri.conf.json` trusts `pair`.
struct Release {
    _temp: TempDir,
    assets: PathBuf,
    workspace: PathBuf,
    pair: minisign::KeyPair,
}

impl Release {
    fn new() -> Release {
        let temp = tempfile::tempdir().expect("temp dir");
        let assets = temp.path().join("assets");
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&assets).expect("assets dir");
        let pair = key_pair();
        let release = Release {
            _temp: temp,
            assets,
            workspace,
            pair,
        };
        release.trust(&public(&release.pair));
        let updaters = updaters();
        for file in checksummed_files(&updaters) {
            fs::write(release.assets.join(&file), format!("contents of {file}")).expect("file");
        }
        for updater in &updaters {
            release.resign(
                &updater.file,
                &release_comment(&updater.file),
                &release.pair,
            );
        }
        // One `.sig` ends in a newline, as an editor may leave it; both forms
        // are trimmed.
        let first = release.assets.join(format!("{}.sig", updaters[0].file));
        let text = fs::read_to_string(&first).expect("signature");
        fs::write(&first, text + "\n").expect("signature");
        release
    }

    /// Writes `key` as the workspace's `plugins.updater.pubkey`.
    fn trust(&self, key: &str) {
        let desktop = self.workspace.join("crates/workshop/desktop");
        fs::create_dir_all(&desktop).expect("desktop dir");
        let config = serde_json::json!({ "plugins": { "updater": { "pubkey": key } } });
        fs::write(desktop.join("tauri.conf.json"), config.to_string()).expect("config");
    }

    /// Signs `file` with `trusted` as the trusted comment, with no trailing
    /// newline, as the Tauri CLI's `sign_file` writes it.
    fn resign(&self, file: &str, trusted: &str, pair: &minisign::KeyPair) {
        let data = fs::read(self.assets.join(file)).expect("read file");
        let signature = sign(pair, &data, trusted);
        fs::write(self.assets.join(format!("{file}.sig")), signature).expect("signature");
    }

    fn request(&self) -> ManifestRequest {
        ManifestRequest {
            assets: self.assets.clone(),
            base_url: format!("{BASE_URL}/"),
            public_key: None,
        }
    }

    fn write(&self) -> Result<(), String> {
        write(&self.request(), &self.workspace, 1_709_210_096)
    }

    fn manifest(&self) -> serde_json::Value {
        let text = fs::read_to_string(self.assets.join(MANIFEST)).expect("latest.json");
        serde_json::from_str(&text).expect("latest.json is JSON")
    }
}

#[test]
fn the_complete_set_for_the_workspace_version_passes() {
    let release = Release::new();
    release.write().expect("a complete release");
    assert!(release.assets.join(MANIFEST).is_file());
    assert!(release.assets.join(CHECKSUMS).is_file());
    // A second run over its own output rewrites it.
    release.write().expect("a rerun");
}

#[test]
fn the_release_is_the_fourteen_planned_files() {
    let updaters = updaters();
    assert_eq!(updaters.len(), 5);
    assert_eq!(checksummed_files(&updaters).len(), 9);
    let mut planned = BTreeSet::new();
    for name in [
        "Runvil_{v}_x64-setup.exe",
        "Runvil_{v}_aarch64-setup.dmg",
        "Runvil_{v}_x86_64-setup.dmg",
        "Runvil_{v}_amd64-setup.run",
        "Runvil_{v}_aarch64-setup.run",
        "Runvil_{v}_aarch64.app.tar.gz",
        "Runvil_{v}_x86_64.app.tar.gz",
        "Runvil_{v}_amd64.AppImage",
        "Runvil_{v}_aarch64.AppImage",
    ] {
        let name = name.replace("{v}", VERSION);
        if !name.ends_with("-setup.dmg") && !name.ends_with("-setup.run") {
            planned.insert(format!("{name}.sig"));
        }
        planned.insert(name);
    }
    assert_eq!(release_files(&updaters), planned);
}

#[test]
fn a_missing_file_fails_naming_it() {
    let release = Release::new();
    let dmg = installer_name(RELEASE_PLATFORMS[1]);
    fs::remove_file(release.assets.join(&dmg)).expect("remove");
    let error = release.write().expect_err("a missing installer");
    assert!(
        error.contains(&format!("missing 1 of 14: {dmg}")),
        "{error}"
    );
    assert!(
        !release.assets.join(MANIFEST).exists(),
        "nothing is written"
    );
}

#[test]
fn an_extra_file_fails_naming_it() {
    let release = Release::new();
    fs::write(release.assets.join("notes.txt"), "stray").expect("stray file");
    let error = release.write().expect_err("an extra file");
    assert!(error.contains("unexpected: notes.txt"), "{error}");
}

#[test]
fn a_release_name_that_is_not_a_regular_file_fails_naming_it() {
    let release = Release::new();
    let dmg = installer_name(RELEASE_PLATFORMS[2]);
    fs::remove_file(release.assets.join(&dmg)).expect("remove");
    fs::create_dir(release.assets.join(&dmg)).expect("directory");
    let error = release.write().expect_err("a directory");
    assert!(
        error.contains(&format!("not a regular file: {dmg}")),
        "{error}"
    );
    assert!(!error.contains("missing"), "{error}");
    #[cfg(unix)]
    {
        let run = installer_name(RELEASE_PLATFORMS[3]);
        let target = release.workspace.join(&run);
        fs::rename(release.assets.join(&run), &target).expect("move");
        std::os::unix::fs::symlink(&target, release.assets.join(&run)).expect("symlink");
        let error = release.write().expect_err("a symlink");
        assert!(error.contains(&format!("{run}, {dmg}")), "{error}");
    }
}

#[test]
fn a_missing_release_directory_fails_naming_it() {
    let release = Release::new();
    let request = ManifestRequest {
        assets: release.workspace.join("absent"),
        ..release.request()
    };
    let error = write(&request, &release.workspace, 0).expect_err("no directory");
    assert!(
        error.contains("cannot read the release directory"),
        "{error}"
    );
    assert!(error.contains("absent"), "{error}");
}

#[test]
fn a_file_named_for_another_version_fails_naming_it() {
    let release = Release::new();
    let setup = installer_name(RELEASE_PLATFORMS[0]);
    let other = setup.replace(VERSION, "9.9.9");
    fs::rename(release.assets.join(&setup), release.assets.join(&other)).expect("rename");
    let error = release.write().expect_err("another version's file");
    assert!(
        error.contains(&format!("missing 1 of 14: {setup}")),
        "{error}"
    );
    assert!(error.contains(&format!("unexpected: {other}")), "{error}");
}

#[test]
fn a_wrong_key_fails_naming_every_platform_key_and_file() {
    let release = Release::new();
    release.trust(&public(&key_pair()));
    let error = release.write().expect_err("signed with another key");
    assert!(error.contains("5 of 5 updater signatures fail"), "{error}");
    assert!(
        error.contains("which every installed copy trusts"),
        "{error}"
    );
    assert!(
        !release.assets.join(MANIFEST).exists() && !release.assets.join(CHECKSUMS).exists(),
        "nothing is written"
    );
    for updater in updaters() {
        assert!(
            error.contains(&format!(
                "{} ({}): the signature was made with another key",
                updater.key, updater.file
            )),
            "{error}"
        );
    }
}

#[test]
fn a_tampered_file_fails_naming_its_platform_key() {
    let release = Release::new();
    let appimage = workshop_updater_name(RELEASE_PLATFORMS[4]);
    fs::write(release.assets.join(&appimage), "tampered").expect("tamper");
    let error = release.write().expect_err("a tampered file");
    assert!(error.contains("1 of 5 updater signatures fail"), "{error}");
    assert!(
        error.contains(&format!(
            "linux-aarch64 ({appimage}): the signature does not match the file"
        )),
        "{error}"
    );
}

#[test]
fn a_signature_that_is_not_base64_minisign_text_fails_naming_its_platform_key() {
    let release = Release::new();
    let setup = installer_name(RELEASE_PLATFORMS[0]);
    let raw = minisign::sign(None, &release.pair.sk, &b"x"[..], None, None).expect("sign");
    for (contents, reason) in [
        (raw.into_string(), "the signature is not standard base64"),
        (
            STANDARD.encode("junk"),
            "the signature is not a minisign signature",
        ),
    ] {
        fs::write(release.assets.join(format!("{setup}.sig")), contents).expect("signature");
        let error = release.write().expect_err("an unusable signature");
        assert!(
            error.contains(&format!("windows-x86_64 ({setup}): {reason}")),
            "{error}"
        );
    }
}

#[test]
fn a_trusted_comment_without_the_version_fails_naming_its_platform_key() {
    let release = Release::new();
    let archive = workshop_updater_name(RELEASE_PLATFORMS[1]);
    release.resign(
        &archive,
        &format!("timestamp:1\tfile:{archive}"),
        &release.pair,
    );
    let error = release.write().expect_err("no version field");
    assert!(
        error.contains(&format!(
            "darwin-aarch64 ({archive}): the signature's trusted comment must carry \
             \"version:{VERSION}\""
        )),
        "{error}"
    );
}

#[test]
fn a_trusted_comment_with_another_version_fails_naming_its_platform_key() {
    let release = Release::new();
    let setup = installer_name(RELEASE_PLATFORMS[0]);
    release.resign(
        &setup,
        &format!("timestamp:1\tfile:{setup}\tversion:9.9.9"),
        &release.pair,
    );
    let error = release.write().expect_err("another version");
    assert!(
        error.contains(&format!(
            "windows-x86_64 ({setup}): the signature's trusted comment"
        )),
        "{error}"
    );
    assert!(error.contains("version:9.9.9"), "{error}");
}

#[test]
fn the_public_key_option_replaces_the_tauri_config_key() {
    let release = Release::new();
    let key_file = release.workspace.join("run.key.pub");
    fs::write(&key_file, public(&release.pair)).expect("key file");
    release.trust(&public(&key_pair()));
    let request = ManifestRequest {
        public_key: Some(key_file.clone()),
        ..release.request()
    };
    write(&request, &release.workspace, 0).expect("the key file's key verifies");

    let other = release.workspace.join("other.key.pub");
    fs::write(&other, public(&key_pair())).expect("other key file");
    let request = ManifestRequest {
        public_key: Some(other.clone()),
        ..release.request()
    };
    let error = write(&request, &release.workspace, 0).expect_err("the file's key is used");
    assert!(error.contains(&other.display().to_string()), "{error}");
    assert!(!error.contains("every installed copy"), "{error}");

    let request = ManifestRequest {
        public_key: Some(release.workspace.join("absent.key.pub")),
        ..release.request()
    };
    let error = write(&request, &release.workspace, 0).expect_err("no key file");
    assert!(error.contains("cannot read the public key"), "{error}");
}

#[test]
fn a_tauri_config_without_a_key_fails_naming_the_field() {
    let release = Release::new();
    let config = release
        .workspace
        .join("crates/workshop/desktop/tauri.conf.json");
    fs::write(&config, r#"{"plugins":{}}"#).expect("config");
    let error = release.write().expect_err("no key in the config");
    assert!(
        error.contains("has no string at plugins.updater.pubkey"),
        "{error}"
    );
}

#[test]
fn latest_json_carries_the_version_and_workshop_s_five_entries() {
    let release = Release::new();
    release.write().expect("a complete release");
    let manifest = release.manifest();
    assert_eq!(manifest["version"], VERSION);
    assert_eq!(manifest["notes"], format!("Runvil {VERSION}"));
    assert_eq!(manifest["pub_date"], "2024-02-29T12:34:56Z");
    let platforms = manifest["platforms"].as_object().expect("platforms");
    let keys: Vec<&str> = platforms.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "darwin-aarch64",
            "darwin-x86_64",
            "linux-aarch64",
            "linux-x86_64",
            "windows-x86_64",
        ]
    );
    for updater in updaters() {
        let entry = &platforms[&updater.key];
        assert_eq!(entry["url"], format!("{BASE_URL}/{}", updater.file));
        let signature = fs::read_to_string(release.assets.join(format!("{}.sig", updater.file)))
            .expect("signature");
        assert_eq!(entry["signature"], signature.trim());
    }
}

#[test]
fn sha256sums_lists_nine_sorted_files_with_their_digests() {
    let release = Release::new();
    release.write().expect("a complete release");
    let text = fs::read_to_string(release.assets.join(CHECKSUMS)).expect("SHA256SUMS");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 9);
    let mut names = Vec::new();
    for line in lines {
        let (digest, name) = line.split_once("  ").expect("two-space separator");
        let data = fs::read(release.assets.join(name)).expect("listed file");
        let expected = Sha256::digest(&data)
            .iter()
            .fold(String::new(), |mut hex, byte| {
                write!(hex, "{byte:02x}").expect("write to a String");
                hex
            });
        assert_eq!(digest, expected, "{name}");
        names.push(name.to_owned());
    }
    // The installers and the other updater files, sorted, with no signature.
    assert_eq!(names, checksummed_files(&updaters()));
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn every_release_name_is_url_safe() {
    for name in release_files(&updaters()) {
        assert!(
            name.chars()
                .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character)),
            "{name}"
        );
    }
}

#[test]
fn a_base_url_that_cannot_take_a_file_name_is_refused() {
    for value in [
        "releases/v1",
        "https://",
        "https:///v1",
        "https://h/v1?x=1",
        "https://h/v1#x",
        "https://h/a b",
    ] {
        let error = base_url(value).expect_err(value);
        assert!(
            error.contains("must be an http or https URL"),
            "{value}: {error}"
        );
    }
    assert_eq!(base_url("http://host/"), Ok("http://host"));
}

#[test]
fn pub_date_is_rfc_3339_in_utc() {
    assert_eq!(pub_date(0), "1970-01-01T00:00:00Z");
    assert_eq!(pub_date(1_709_210_096), "2024-02-29T12:34:56Z");
    assert_eq!(pub_date(1_767_225_599), "2025-12-31T23:59:59Z");
    assert_eq!(pub_date(1_767_225_600), "2026-01-01T00:00:00Z");
}
