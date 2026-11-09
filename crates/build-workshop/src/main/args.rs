//! Command-line parsing for the build, installer, and manifest modes.

use std::path::PathBuf;

use super::{BuildRequest, Profile};

pub(super) const USAGE: &str = "\
Build Workshop, build a platform installer that carries the Gateway, or
write a release's latest.json and SHA256SUMS. Workshop bundles the Gateway
as a sidecar only for Windows targets; macOS and Linux install it beside
Workshop. On macOS and Linux, `installer` needs Qt IFW 4.11.0, found through
QTIFWDIR (the directory holding bin/) or else binarycreator on PATH.

The Gateway is a separate product, so this never builds it. It comes from
--gateway-binary, then the RUNVIL_GATEWAY_SIDECAR environment variable, then
the file `node tools/fetch-gateway-sidecar.mjs --target <triple>` stages
under crates/workshop/desktop/binaries/: the release gateway-pin/pin.json
names. A staged Gateway is left in place, so the next build reuses it.

USAGE:
    cargo workshop [--release] [--target <triple>] [--gateway-binary <path>]
    cargo workshop installer [--target <triple>] [--sign] [--gateway-binary <path>]
    cargo workshop manifest --assets <dir> --base-url <url> [--public-key <file>]

OPTIONS:
    --release           Build Workshop with Cargo's release profile
    --target <triple>   Build for this target triple instead of the host
    --gateway-binary <path>
                        The Gateway executable to ship instead of the staged one
    --sign              Sign the updater files with TAURI_SIGNING_PRIVATE_KEY
                        and TAURI_SIGNING_PRIVATE_KEY_PASSWORD (unset is empty)
    --assets <dir>      The directory holding every platform's published
                        files, exactly one release's; latest.json and
                        SHA256SUMS are written into it
    --base-url <url>    The URL the release's files are downloaded from
    --public-key <file> The updater public key to verify against, instead
                        of plugins.updater.pubkey in tauri.conf.json
    -h, --help          Print this help
";

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Request {
    Build(BuildRequest),
    Installer(InstallerRequest),
    Manifest(ManifestRequest),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct InstallerRequest {
    pub(super) target: Option<String>,
    pub(super) sign: bool,
    pub(super) gateway_binary: Option<PathBuf>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ManifestRequest {
    pub(super) assets: PathBuf,
    pub(super) base_url: String,
    pub(super) public_key: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Flag {
    Release,
    Target,
    Sign,
    GatewayBinary,
    Assets,
    BaseUrl,
    PublicKey,
}

impl Flag {
    const ALL: [Self; 7] = [
        Self::Release,
        Self::Target,
        Self::Sign,
        Self::GatewayBinary,
        Self::Assets,
        Self::BaseUrl,
        Self::PublicKey,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Release => "--release",
            Self::Target => "--target",
            Self::Sign => "--sign",
            Self::GatewayBinary => "--gateway-binary",
            Self::Assets => "--assets",
            Self::BaseUrl => "--base-url",
            Self::PublicKey => "--public-key",
        }
    }
}

#[derive(Debug, Default)]
struct Options {
    release: bool,
    target: Option<String>,
    sign: bool,
    gateway_binary: Option<String>,
    assets: Option<String>,
    base_url: Option<String>,
    public_key: Option<String>,
}

impl Options {
    fn parse(args: &[String], allowed: &[Flag]) -> Result<Self, anyhow::Error> {
        let mut options = Self::default();
        let mut index = 0;
        while index < args.len() {
            let argument = args[index].as_str();
            let flag = Flag::ALL
                .into_iter()
                .find(|flag| flag.name() == argument && allowed.contains(flag))
                .ok_or_else(|| usage_error(&format!("unsupported argument `{argument}`")))?;
            if options.seen(flag) {
                return Err(usage_error(&format!("duplicate argument `{argument}`")));
            }
            match flag {
                Flag::Release => options.release = true,
                Flag::Sign => options.sign = true,
                Flag::Target => {
                    let value = value_after(args, index, "a target triple")?;
                    if !valid_target_triple(value) {
                        return Err(usage_error(&format!(
                            "argument `--target` needs a valid target triple, got `{value}`"
                        )));
                    }
                    options.target = Some(value.to_owned());
                    index += 1;
                }
                Flag::GatewayBinary => {
                    options.gateway_binary = Some(value_after(args, index, "a path")?.to_owned());
                    index += 1;
                }
                Flag::Assets => {
                    options.assets = Some(value_after(args, index, "a directory")?.to_owned());
                    index += 1;
                }
                Flag::BaseUrl => {
                    options.base_url = Some(value_after(args, index, "a URL")?.to_owned());
                    index += 1;
                }
                Flag::PublicKey => {
                    options.public_key = Some(value_after(args, index, "a path")?.to_owned());
                    index += 1;
                }
            }
            index += 1;
        }
        Ok(options)
    }

    fn seen(&self, flag: Flag) -> bool {
        match flag {
            Flag::Release => self.release,
            Flag::Target => self.target.is_some(),
            Flag::Sign => self.sign,
            Flag::GatewayBinary => self.gateway_binary.is_some(),
            Flag::Assets => self.assets.is_some(),
            Flag::BaseUrl => self.base_url.is_some(),
            Flag::PublicKey => self.public_key.is_some(),
        }
    }
}

pub(super) fn parse_arguments(args: &[String]) -> Result<Request, anyhow::Error> {
    match args.first().map(String::as_str) {
        Some("installer") => {
            if args[1..].iter().any(|argument| argument == "--release") {
                return Err(usage_error(
                    "`installer` always builds the release profile; drop `--release`",
                ));
            }
            let options =
                Options::parse(&args[1..], &[Flag::Target, Flag::Sign, Flag::GatewayBinary])?;
            Ok(Request::Installer(InstallerRequest {
                target: options.target,
                sign: options.sign,
                gateway_binary: options.gateway_binary.map(PathBuf::from),
            }))
        }
        Some("manifest") => parse_manifest(&args[1..]),
        _ => {
            let options =
                Options::parse(args, &[Flag::Release, Flag::Target, Flag::GatewayBinary])?;
            Ok(Request::Build(BuildRequest {
                profile: if options.release {
                    Profile::Release
                } else {
                    Profile::Debug
                },
                target: options.target,
                gateway_binary: options.gateway_binary.map(PathBuf::from),
            }))
        }
    }
}

fn parse_manifest(args: &[String]) -> Result<Request, anyhow::Error> {
    let options = Options::parse(args, &[Flag::Assets, Flag::BaseUrl, Flag::PublicKey])?;
    let assets = options
        .assets
        .ok_or_else(|| usage_error("`manifest` needs `--assets <dir>`"))?;
    let base_url = options
        .base_url
        .ok_or_else(|| usage_error("`manifest` needs `--base-url <url>`"))?;
    Ok(Request::Manifest(ManifestRequest {
        assets: PathBuf::from(assets),
        base_url,
        public_key: options.public_key.map(PathBuf::from),
    }))
}

fn value_after<'a>(
    args: &'a [String],
    index: usize,
    expected: &str,
) -> Result<&'a str, anyhow::Error> {
    let name = &args[index];
    match args.get(index + 1) {
        Some(value) if !value.starts_with('-') => Ok(value),
        _ => Err(usage_error(&format!("argument `{name}` needs {expected}"))),
    }
}

fn usage_error(message: &str) -> anyhow::Error {
    anyhow::anyhow!("{message}\n\n{USAGE}")
}

pub(super) fn valid_target_triple(target: &str) -> bool {
    let parts: Vec<&str> = target.split('-').collect();
    parts.len() >= 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
}
