//! The Qt Installer Framework stage of the macOS and Linux installers:
//! locates IFW, checks its version, stages the definition in
//! `crates/build-workshop/ifw/` around the collected payload, and builds
//! the installer with `binarycreator`, which on macOS `hdiutil` then wraps
//! in a DMG.
//!
//! The definition's text files are templates whose `{{NAME}}` placeholders
//! are filled here; IFW's own `@Variable@` placeholders are expanded at
//! install time and left alone.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use super::collect::{
    GATEWAY, GATEWAY_BUNDLE_NAME, WORKSHOP_APPIMAGE_NAME, WORKSHOP_BUNDLE_NAME, create_directory,
    installer_name,
};
use super::{Platform, System, VERSION};
use crate::{CommandSpec, OutputMode};

/// The one IFW release local and CI builds use; any other is refused.
pub(crate) const IFW_VERSION: &str = "4.11.0";

const WORKSHOP_PACKAGE: &str = "com.runvil.workshop";
const GATEWAY_PACKAGE: &str = "com.runvil.gateway";
const INSTALLER_APP: &str = "Runvil Installer.app";

/// The two IFW programs the build runs: `binarycreator`, and
/// `installerbase`, the installer template it embeds, whose version is the
/// one the installer carries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ifw {
    pub(crate) binarycreator: PathBuf,
    pub(crate) installerbase: PathBuf,
}

/// Finds IFW from `QTIFWDIR` (`qtifw_dir`) when it is set and non-empty,
/// with no fallback, so a stale value cannot silently pick another IFW;
/// otherwise from the first `binarycreator` on `PATH` (`path`), with
/// `installerbase` beside it. Relative values resolve against `cwd`, and
/// the paths returned are absolute, since the children that run them each
/// start in their own working directory.
pub(crate) fn locate(
    qtifw_dir: Option<&OsStr>,
    path: Option<&OsStr>,
    cwd: &Path,
) -> Result<Ifw, String> {
    if let Some(dir) = qtifw_dir.filter(|dir| !dir.is_empty()) {
        let bin = cwd.join(dir).join("bin");
        let binarycreator = bin.join("binarycreator");
        if !is_regular_file(&binarycreator) {
            return Err(format!(
                "Qt IFW {IFW_VERSION} not found: QTIFWDIR is {}, which holds no {}; point \
                 QTIFWDIR at the IFW {IFW_VERSION} directory that holds bin/, or unset it to \
                 search PATH",
                Path::new(dir).display(),
                binarycreator.display()
            ));
        }
        return beside(binarycreator, &bin);
    }
    let found = path
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|entry| cwd.join(entry).join("binarycreator"))
        .find(|candidate| is_regular_file(candidate));
    match found {
        Some(binarycreator) => {
            let bin = binarycreator
                .parent()
                .map_or_else(PathBuf::new, Path::to_path_buf);
            beside(binarycreator, &bin)
        }
        None => Err(format!(
            "Qt IFW {IFW_VERSION} not found: QTIFWDIR is unset or empty and no binarycreator \
             is on PATH; \
             set QTIFWDIR to the IFW {IFW_VERSION} directory that holds bin/"
        )),
    }
}

fn beside(binarycreator: PathBuf, bin: &Path) -> Result<Ifw, String> {
    let installerbase = bin.join("installerbase");
    if is_regular_file(&installerbase) {
        Ok(Ifw {
            binarycreator,
            installerbase,
        })
    } else {
        Err(format!(
            "Qt IFW is incomplete: {} has no installerbase beside it at {}",
            binarycreator.display(),
            installerbase.display()
        ))
    }
}

fn is_regular_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

pub(crate) fn version_command(ifw: &Ifw, workspace_root: &Path) -> CommandSpec {
    CommandSpec {
        program: ifw.installerbase.clone(),
        args: vec![OsString::from("--version")],
        current_dir: workspace_root.to_path_buf(),
        envs: Vec::new(),
        output_mode: OutputMode::Capture,
    }
}

/// Checks `installerbase --version` output, whose first line reads
/// `IFW Version: 4.11.0, built with Qt 6.9.3.`.
pub(crate) fn check_version(ifw: &Ifw, printed: &str) -> Result<(), String> {
    let found = printed
        .lines()
        .find_map(|line| line.trim().strip_prefix("IFW Version: "))
        .map(|rest| rest.split([',', ' ']).next().unwrap_or_default());
    match found {
        Some(IFW_VERSION) => Ok(()),
        Some(found) => Err(format!(
            "Qt IFW {found} found at {}, {IFW_VERSION} required; install IFW {IFW_VERSION} \
             and point QTIFWDIR at it",
            ifw.installerbase.display()
        )),
        None => Err(format!(
            "Qt IFW version check: `{} --version` printed no `IFW Version:` line; IFW \
             {IFW_VERSION} required",
            ifw.installerbase.display()
        )),
    }
}

/// What the definition's placeholders become for one platform.
fn placeholders(
    platform: Platform,
    release_date: &str,
    gateway_version: &str,
) -> [(&'static str, String); 6] {
    let (target_dir, gateway, workshop) = match platform.system {
        System::MacOs => (
            "@ApplicationsDir@/Runvil",
            format!("{GATEWAY_BUNDLE_NAME}/Contents/MacOS/{GATEWAY}"),
            // Fills the template only: the Workshop script reads it on Linux.
            format!("{WORKSHOP_BUNDLE_NAME}/Contents/MacOS/runvil-workshop"),
        ),
        System::Linux | System::Windows => (
            "@HomeDir@/Runvil",
            GATEWAY.to_owned(),
            WORKSHOP_APPIMAGE_NAME.to_owned(),
        ),
    };
    [
        ("VERSION", VERSION.to_owned()),
        // The Gateway is its own product with its own version, the pin's.
        ("GATEWAY_VERSION", ifw_version(gateway_version).to_owned()),
        ("RELEASE_DATE", release_date.to_owned()),
        ("TARGET_DIR", target_dir.to_owned()),
        ("GATEWAY_EXECUTABLE", gateway),
        ("WORKSHOP_EXECUTABLE", workshop),
    ]
}

/// The part of `version` an IFW `<Version>` accepts. IFW takes digits
/// joined by `.` or `-` and refuses anything else, so a prerelease or build
/// suffix such as `-rc.1` is left off: `0.4.0-rc.1` becomes `0.4.0`.
pub(crate) fn ifw_version(version: &str) -> &str {
    let core = version
        .find(|character: char| !(character.is_ascii_digit() || character == '.'))
        .map_or(version, |end| &version[..end]);
    core.trim_end_matches('.')
}

/// The checked-in files the stage is built from.
#[derive(Debug)]
pub(crate) struct Sources {
    pub(crate) definition: PathBuf,
    pub(crate) license: PathBuf,
    pub(crate) icons: PathBuf,
}

impl Sources {
    pub(crate) fn in_workspace(workspace_root: &Path) -> Self {
        let crates = workspace_root.join("crates");
        Self {
            definition: crates.join("build-workshop").join("ifw"),
            license: workspace_root.join("LICENSE"),
            icons: crates.join("workshop").join("desktop").join("icons"),
        }
    }
}

/// Writes the definition, filled for `platform`, with the icons, the
/// license, and the `payload` items in their packages' `data/`, into
/// `stage`, which `collect` has already cleared with the rest of the
/// output directory.
pub(crate) fn stage(
    sources: &Sources,
    payload: &Path,
    platform: Platform,
    release_date: &str,
    gateway_version: &str,
    stage: &Path,
) -> Result<(), String> {
    let values = placeholders(platform, release_date, gateway_version);
    copy_definition(&sources.definition, stage, &values)?;
    let config = stage.join("config");
    for icon in ["icon.icns", "icon.png"] {
        copy_file(&sources.icons.join(icon), &config.join(icon))?;
    }
    let packages = stage.join("packages");
    for package in [WORKSHOP_PACKAGE, GATEWAY_PACKAGE] {
        copy_file(
            &sources.license,
            &packages.join(package).join("meta").join("LICENSE.txt"),
        )?;
    }
    let (workshop, gateway, icons) = match platform.system {
        System::MacOs => (WORKSHOP_BUNDLE_NAME, GATEWAY_BUNDLE_NAME, false),
        System::Linux | System::Windows => (WORKSHOP_APPIMAGE_NAME, GATEWAY, true),
    };
    for (package, item, icon) in [
        (WORKSHOP_PACKAGE, workshop, "runvil-workshop.png"),
        (GATEWAY_PACKAGE, gateway, "runvil-gateway.png"),
    ] {
        let data = packages.join(package).join("data");
        create_directory(&data)?;
        copy_tree(&payload.join(item), &data.join(item))?;
        // Each package carries its own copy for its desktop entry: a shared
        // file would go with whichever component is uninstalled first.
        if icons {
            copy_file(&sources.icons.join("icon.png"), &data.join(icon))?;
        }
    }
    Ok(())
}

/// Copies the definition tree, filling the placeholders in its `.xml` and
/// `.qs` templates and failing on any placeholder left over.
fn copy_definition(
    source: &Path,
    destination: &Path,
    values: &[(&'static str, String)],
) -> Result<(), String> {
    create_directory(destination)?;
    for child in sorted_children(source)? {
        let from = source.join(&child);
        let to = destination.join(&child);
        if from.is_dir() {
            copy_definition(&from, &to, values)?;
        } else if matches!(from.extension().and_then(OsStr::to_str), Some("xml" | "qs")) {
            let template = fs::read_to_string(&from)
                .map_err(|error| format!("cannot read {}: {error}", from.display()))?;
            let filled = fill(&template, values).map_err(|placeholder| {
                format!("{} has no value for {placeholder}", from.display())
            })?;
            fs::write(&to, filled)
                .map_err(|error| format!("cannot write {}: {error}", to.display()))?;
        } else {
            copy_file(&from, &to)?;
        }
    }
    Ok(())
}

/// Fills every known placeholder; the error is the first one left over.
fn fill(template: &str, values: &[(&'static str, String)]) -> Result<String, String> {
    let mut filled = template.to_owned();
    for (name, value) in values {
        filled = filled.replace(&format!("{{{{{name}}}}}"), value);
    }
    match filled.find("{{") {
        None => Ok(filled),
        Some(start) => {
            let rest = &filled[start..];
            let end = rest.find("}}").map_or(rest.len(), |end| end + 2);
            Err(rest[..end].to_owned())
        }
    }
}

/// Copies a payload item, a file or a directory tree, keeping each file's
/// mode; a symlink is refused naming its path, as the archive writer does.
fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
    if metadata.is_dir() {
        create_directory(destination)?;
        for child in sorted_children(source)? {
            copy_tree(&source.join(&child), &destination.join(&child))?;
        }
        Ok(())
    } else if metadata.is_file() {
        copy_file(source, destination)
    } else {
        Err(format!(
            "{} is neither a file nor a directory and cannot be packaged",
            source.display()
        ))
    }
}

fn sorted_children(directory: &Path) -> Result<Vec<OsString>, String> {
    let mut children = fs::read_dir(directory)
        .and_then(|entries| {
            entries
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    children.sort();
    Ok(children)
}

/// `fs::copy` keeps the source's permission bits, so executables stay
/// `0755`.
fn copy_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::copy(source, destination).map(|_| ()).map_err(|error| {
        format!(
            "cannot copy {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })
}

/// Where binarycreator writes: on macOS the installer app, which `hdiutil`
/// wraps from its own folder; on Linux the published `.run`.
pub(crate) fn binarycreator_output(platform: Platform, stage: &Path, publish: &Path) -> PathBuf {
    match platform.system {
        System::MacOs => stage.join("dmg").join(INSTALLER_APP),
        System::Linux | System::Windows => publish.join(installer_name(platform)),
    }
}

pub(crate) fn dmg_path(platform: Platform, publish: &Path) -> PathBuf {
    publish.join(installer_name(platform))
}

/// binarycreator can write a DMG itself, but it ignores `hdiutil`'s exit
/// status and then deletes the app either way, so a failed wrap would
/// leave neither file; the build wraps it instead.
/// <https://github.com/qtproject/installer-framework/blob/4.11.0/src/libs/ifwtools/binarycreator.cpp#L464-L492>
pub(crate) fn binarycreator_command(ifw: &Ifw, stage: &Path, output: &Path) -> CommandSpec {
    let mut args = vec![OsString::from("--offline-only"), OsString::from("-c")];
    args.push(stage.join("config").join("config.xml").into_os_string());
    args.push(OsString::from("-p"));
    args.push(stage.join("packages").into_os_string());
    args.push(output.as_os_str().to_owned());
    CommandSpec {
        program: ifw.binarycreator.clone(),
        args,
        current_dir: stage.to_path_buf(),
        envs: Vec::new(),
        output_mode: OutputMode::Inherit,
    }
}

/// The app binarycreator assembles on macOS (`installerbase` copied
/// unchanged, a new `Info.plist`, the resources in
/// `Contents/Resources/installer.dat`) fails signature validation, and
/// macOS kills it at launch. binarycreator's own `--sign` refuses `-`,
/// the ad hoc identity, so the build signs the app ad hoc itself.
/// <https://github.com/qtproject/installer-framework/blob/4.11.0/src/libs/ifwtools/binarycreator.cpp#L355-L388>
/// <https://github.com/qtproject/installer-framework/blob/4.11.0/tools/binarycreator/main.cpp#L217-L221>
pub(crate) fn adhoc_sign_command(app: &Path) -> CommandSpec {
    let mut args = ["--force", "--sign", "-"].map(OsString::from).to_vec();
    args.push(app.as_os_str().to_owned());
    CommandSpec {
        program: PathBuf::from("codesign"),
        args,
        current_dir: app.parent().map_or_else(PathBuf::new, Path::to_path_buf),
        envs: Vec::new(),
        output_mode: OutputMode::Inherit,
    }
}

pub(crate) fn hdiutil_command(app_folder: &Path, dmg: &Path) -> CommandSpec {
    let mut args = ["create", "-volname"].map(OsString::from).to_vec();
    args.push(OsString::from(format!("Runvil {VERSION}")));
    args.push(OsString::from("-srcfolder"));
    args.push(app_folder.as_os_str().to_owned());
    args.extend(["-fs", "HFS+", "-format", "UDZO", "-ov"].map(OsString::from));
    args.push(dmg.as_os_str().to_owned());
    CommandSpec {
        program: PathBuf::from("hdiutil"),
        args,
        current_dir: app_folder.to_path_buf(),
        envs: Vec::new(),
        output_mode: OutputMode::Inherit,
    }
}

/// `YYYY-MM-DD` in UTC for `seconds` since the Unix epoch, the form IFW
/// requires for `ReleaseDate`.
pub(crate) fn utc_date(seconds: u64) -> String {
    // Howard Hinnant's days-to-civil algorithm on the proleptic Gregorian
    // calendar, with eras of 400 years starting on 0000-03-01.
    let days = seconds / 86_400 + 719_468;
    let era = days / 146_097;
    let day_of_era = days % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
