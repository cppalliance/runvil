//! `cargo workshop manifest`: checks that a directory holds exactly one
//! release's files for every platform, verifies each updater file's
//! signature against the updater public key, and writes the release's
//! `latest.json` and `SHA256SUMS` into it.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read as _;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest as _, Sha256};

use super::args::ManifestRequest;
use super::installer::collect::{arch_name, installer_name, workshop_updater_name};
use super::installer::ifw::utc_date;
use super::installer::{Arch, Platform, System, VERSION};

pub(crate) const MANIFEST: &str = "latest.json";
pub(crate) const CHECKSUMS: &str = "SHA256SUMS";
const SIGNATURE_SUFFIX: &str = ".sig";
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Every platform a release ships an installer for.
const RELEASE_PLATFORMS: [Platform; 5] = [
    Platform {
        system: System::Windows,
        arch: Arch::X86_64,
    },
    Platform {
        system: System::MacOs,
        arch: Arch::Aarch64,
    },
    Platform {
        system: System::MacOs,
        arch: Arch::X86_64,
    },
    Platform {
        system: System::Linux,
        arch: Arch::X86_64,
    },
    Platform {
        system: System::Linux,
        arch: Arch::Aarch64,
    },
];

/// One signed updater file and the `latest.json` key it is published under.
#[derive(Debug, Eq, PartialEq)]
struct Updater {
    key: String,
    file: String,
}

/// The release's five signed updater files: Workshop's on each platform,
/// under the Tauri updater's `{os}-{arch}` key. The Gateway is its own
/// product with its own releases, so this release signs no Gateway file.
fn updaters() -> Vec<Updater> {
    RELEASE_PLATFORMS
        .into_iter()
        .map(|platform| {
            let os = match platform.system {
                System::Windows => "windows",
                System::MacOs => "darwin",
                System::Linux => "linux",
            };
            Updater {
                key: format!("{os}-{}", arch_name(platform.arch)),
                file: workshop_updater_name(platform),
            }
        })
        .collect()
}

/// The files `SHA256SUMS` lists, sorted: the five installers and the other
/// updater files. The Windows setup is both.
fn checksummed_files(updaters: &[Updater]) -> Vec<String> {
    let files: BTreeSet<String> = RELEASE_PLATFORMS
        .into_iter()
        .map(installer_name)
        .chain(updaters.iter().map(|updater| updater.file.clone()))
        .collect();
    files.into_iter().collect()
}

/// Every file the release publishes besides the two this mode writes.
fn release_files(updaters: &[Updater]) -> BTreeSet<String> {
    checksummed_files(updaters)
        .into_iter()
        .chain(
            updaters
                .iter()
                .map(|updater| format!("{}{SIGNATURE_SUFFIX}", updater.file)),
        )
        .collect()
}

/// Verifies `request.assets` as this version's release and writes
/// `latest.json` and `SHA256SUMS` into it, dated `now` (seconds since the
/// Unix epoch). The public key is `request.public_key`'s contents, or else
/// `plugins.updater.pubkey` in `workspace_root`'s Workshop `tauri.conf.json`.
pub(crate) fn write(
    request: &ManifestRequest,
    workspace_root: &Path,
    now: u64,
) -> Result<(), String> {
    let base_url = base_url(&request.base_url)?;
    let updaters = updaters();
    check_file_set(&request.assets, &release_files(&updaters))?;
    let (public_key, against) = public_key(request, workspace_root)?;
    let mut platforms = serde_json::Map::new();
    let mut failures = Vec::new();
    for updater in &updaters {
        match verify(&public_key, &request.assets, &updater.file) {
            Ok(signature) => {
                platforms.insert(
                    updater.key.clone(),
                    serde_json::json!({
                        "url": format!("{base_url}/{}", updater.file),
                        "signature": signature,
                    }),
                );
            }
            Err(reason) => failures.push(format!("{} ({}): {reason}", updater.key, updater.file)),
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "{} of {} updater signatures fail against {against}; each must verify and \
             carry \"version:{VERSION}\":\n  {}",
            failures.len(),
            updaters.len(),
            failures.join("\n  ")
        ));
    }
    let manifest = serde_json::json!({
        "version": VERSION,
        "notes": format!("Runvil {VERSION}"),
        "pub_date": pub_date(now),
        "platforms": platforms,
    });
    let manifest = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("cannot render {MANIFEST}: {error}"))?;
    let checksums = checksummed_files(&updaters)
        .into_iter()
        .map(|file| {
            sha256_hex(&request.assets.join(&file)).map(|digest| format!("{digest}  {file}\n"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    write_file(&request.assets.join(MANIFEST), &(manifest + "\n"))?;
    write_file(&request.assets.join(CHECKSUMS), &checksums.concat())
}

/// `value` without a trailing `/`, once it is an http or https URL with a
/// host and nothing a file name appended after it would break: no query,
/// fragment, or whitespace.
fn base_url(value: &str) -> Result<&str, String> {
    let trimmed = value.trim_end_matches('/');
    let has_host = ["https://", "http://"].iter().any(|scheme| {
        trimmed
            .strip_prefix(scheme)
            .and_then(|rest| rest.split('/').next())
            .is_some_and(|host| !host.is_empty())
    });
    let appendable = !trimmed
        .chars()
        .any(|character| character.is_whitespace() || character == '?' || character == '#');
    if has_host && appendable {
        Ok(trimmed)
    } else {
        Err(format!(
            "`--base-url` must be an http or https URL with a host and no query, fragment, \
             or whitespace; got `{value}`"
        ))
    }
}

/// Requires `assets` to hold `expected` as regular files (not symlinks) and
/// nothing else, except an earlier run's `latest.json` and `SHA256SUMS`,
/// which this run rewrites.
fn check_file_set(assets: &Path, expected: &BTreeSet<String>) -> Result<(), String> {
    let entries = fs::read_dir(assets).map_err(|error| {
        format!(
            "cannot read the release directory {}: {error}",
            assets.display()
        )
    })?;
    let mut present = BTreeSet::new();
    let mut irregular = BTreeSet::new();
    let mut unexpected = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot read {}: {error}", assets.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == MANIFEST || name == CHECKSUMS {
            continue;
        }
        if !expected.contains(&name) {
            unexpected.insert(name);
        } else if entry.file_type().is_ok_and(|kind| kind.is_file()) {
            present.insert(name);
        } else {
            irregular.insert(name);
        }
    }
    let missing: Vec<&str> = expected
        .iter()
        .filter(|name| !present.contains(*name) && !irregular.contains(*name))
        .map(String::as_str)
        .collect();
    if missing.is_empty() && irregular.is_empty() && unexpected.is_empty() {
        return Ok(());
    }
    let mut problems = Vec::new();
    if !missing.is_empty() {
        problems.push(format!(
            "missing {} of {}: {}",
            missing.len(),
            expected.len(),
            missing.join(", ")
        ));
    }
    if !irregular.is_empty() {
        problems.push(format!(
            "not a regular file: {}",
            irregular.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    if !unexpected.is_empty() {
        problems.push(format!(
            "unexpected: {}",
            unexpected.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    Err(format!(
        "{} must hold exactly the {} release files of Runvil {VERSION} as regular files; \
         {}",
        assets.display(),
        expected.len(),
        problems.join("; ")
    ))
}

/// The key to verify against and how a signature failure names it: only
/// the release key in `tauri.conf.json` is the one installed copies trust.
fn public_key(
    request: &ManifestRequest,
    workspace_root: &Path,
) -> Result<(minisign_verify::PublicKey, String), String> {
    let (encoded, source, against) = if let Some(path) = &request.public_key {
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("cannot read the public key {}: {error}", path.display()))?;
        let source = path.display().to_string();
        let against = format!("the key in {source}");
        (contents, source, against)
    } else {
        let config = workspace_root
            .join("crates")
            .join("workshop")
            .join("desktop")
            .join("tauri.conf.json");
        let text = fs::read_to_string(&config)
            .map_err(|error| format!("cannot read {}: {error}", config.display()))?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| format!("{} is not JSON: {error}", config.display()))?;
        let key = value
            .pointer("/plugins/updater/pubkey")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                format!(
                    "{} has no string at plugins.updater.pubkey",
                    config.display()
                )
            })?;
        let source = format!("{} (plugins.updater.pubkey)", config.display());
        let against = format!("the release key in {source}, which every installed copy trusts");
        (key.to_owned(), source, against)
    };
    let text =
        base64_text(&encoded, "public key").map_err(|reason| format!("{source}: {reason}"))?;
    let key = minisign_verify::PublicKey::decode(&text)
        .map_err(|error| format!("{source}: the public key is not a minisign key: {error}"))?;
    Ok((key, against))
}

/// Checks `file`'s signature as the Gateway does before it applies an
/// update, and returns the signature as `latest.json` carries it.
fn verify(key: &minisign_verify::PublicKey, assets: &Path, file: &str) -> Result<String, String> {
    let signature_path = assets.join(format!("{file}{SIGNATURE_SUFFIX}"));
    let encoded = fs::read_to_string(&signature_path)
        .map_err(|error| format!("cannot read {}: {error}", signature_path.display()))?;
    let encoded = encoded.trim().to_owned();
    let signature = minisign_verify::Signature::decode(&base64_text(&encoded, "signature")?)
        .map_err(|error| format!("the signature is not a minisign signature: {error}"))?;
    let required = format!("version:{VERSION}");
    if !signature
        .trusted_comment()
        .split('\t')
        .any(|field| field == required)
    {
        return Err(format!(
            "the signature's trusted comment must carry \"{required}\"; it reads \"{}\"",
            signature.trusted_comment()
        ));
    }
    let mut stream = key
        .verify_stream(&signature)
        .map_err(|error| reason(&error))?;
    read_chunks(&assets.join(file), |chunk| stream.update(chunk))?;
    stream.finalize().map_err(|error| reason(&error))?;
    Ok(encoded)
}

fn reason(error: &minisign_verify::Error) -> String {
    match error {
        minisign_verify::Error::UnexpectedKeyId => {
            "the signature was made with another key".to_owned()
        }
        minisign_verify::Error::InvalidSignature => {
            "the signature does not match the file".to_owned()
        }
        other => format!("the signature cannot be checked: {other}"),
    }
}

fn base64_text(encoded: &str, what: &str) -> Result<String, String> {
    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|error| format!("the {what} is not standard base64: {error}"))?;
    String::from_utf8(bytes).map_err(|_| format!("the {what} does not decode to text"))
}

fn sha256_hex(path: &Path) -> Result<String, String> {
    let mut hasher = Sha256::new();
    read_chunks(path, |chunk| hasher.update(chunk))?;
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hex.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(hex)
}

fn read_chunks(path: &Path, mut consume: impl FnMut(&[u8])) -> Result<(), String> {
    let mut file =
        fs::File::open(path).map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if read == 0 {
            return Ok(());
        }
        consume(&buffer[..read]);
    }
}

fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// RFC 3339 in UTC, to the second, for `seconds` since the Unix epoch.
fn pub_date(seconds: u64) -> String {
    let of_day = seconds % 86_400;
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        utc_date(seconds),
        of_day / 3_600,
        of_day % 3_600 / 60,
        of_day % 60
    )
}

#[cfg(test)]
#[path = "tests/manifest.rs"]
mod tests;
