//! Unit tests for the staged Qt IFW definition, the checked-in definition
//! itself, and the release date.

use super::bundle_tests::collect_files;
use super::*;
use crate::installer::Platform;
use crate::installer::ifw::{self, Sources};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASE_DATE: &str = "2026-10-11";

/// A payload as `collect` leaves it, with executables `0755`.
fn payload(root: &Path, macos: bool) -> PathBuf {
    let payload = root.join("payload");
    let executables = if macos {
        let gateway = payload.join("Runvil Gateway.app").join("Contents");
        write_file(&gateway.join("Info.plist"), b"plist");
        write_file(&gateway.join("Resources").join("icon.icns"), b"icns");
        vec![
            payload
                .join("Runvil.app")
                .join("Contents")
                .join("MacOS")
                .join("runvil-workshop"),
            gateway.join("MacOS").join("runvil-gateway"),
        ]
    } else {
        vec![
            payload.join("Runvil.AppImage"),
            payload.join("runvil-gateway"),
        ]
    };
    for executable in executables {
        write_file(&executable, b"program");
        set_mode(&executable, 0o755);
    }
    payload
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("set mode");
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("staged text")
}

fn staged_files(stage: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_files(stage, stage, &mut files);
    files.sort();
    files
}

fn expected_files(macos: bool) -> Vec<String> {
    let mut files = vec![
        "config/config.xml",
        "config/icon.icns",
        "config/icon.png",
        "config/installer-logo.png",
        "config/installer-logo@2x.png",
        "config/installer-watermark.png",
        "config/installer-watermark@2x.png",
        "packages/com.runvil.gateway/meta/LICENSE.txt",
        "packages/com.runvil.gateway/meta/installscript.qs",
        "packages/com.runvil.gateway/meta/package.xml",
        "packages/com.runvil.workshop/meta/LICENSE.txt",
        "packages/com.runvil.workshop/meta/installscript.qs",
        "packages/com.runvil.workshop/meta/package.xml",
    ];
    if macos {
        files.extend([
            "packages/com.runvil.gateway/data/Runvil Gateway.app/Contents/Info.plist",
            "packages/com.runvil.gateway/data/Runvil Gateway.app/Contents/MacOS/runvil-gateway",
            "packages/com.runvil.gateway/data/Runvil Gateway.app/Contents/Resources/icon.icns",
            "packages/com.runvil.workshop/data/Runvil.app/Contents/MacOS/runvil-workshop",
        ]);
    } else {
        files.extend([
            "packages/com.runvil.gateway/data/runvil-gateway",
            "packages/com.runvil.gateway/data/runvil-gateway.png",
            "packages/com.runvil.workshop/data/Runvil.AppImage",
            "packages/com.runvil.workshop/data/runvil-workshop.png",
        ]);
    }
    let mut files: Vec<String> = files.into_iter().map(str::to_owned).collect();
    files.sort();
    files
}

#[test]
fn the_staged_definition_is_filled_for_each_platform_with_the_payload_in_place() {
    for (target, target_dir) in [
        ("aarch64-apple-darwin", "@ApplicationsDir@/Runvil"),
        ("x86_64-unknown-linux-gnu", "@HomeDir@/Runvil"),
        ("aarch64-unknown-linux-gnu", "@HomeDir@/Runvil"),
    ] {
        let macos = target.ends_with("apple-darwin");
        let test_environment = environment();
        let temp = test_environment.temp.path();
        let sources = Sources::from_environment(&test_environment.environment);
        let payload = payload(temp, macos);
        let stage = temp.join("ifw");
        let platform = Platform::from_triple(target).expect("platform");

        ifw::stage(
            &sources,
            &payload,
            platform,
            RELEASE_DATE,
            GATEWAY_VERSION,
            &stage,
        )
        .expect("stage");

        assert_eq!(staged_files(&stage), expected_files(macos), "{target}");
        check_templates(&stage, target, target_dir);
        check_data(&stage, &sources, macos);
    }
}

fn check_templates(stage: &Path, target: &str, target_dir: &str) {
    let templates = staged_files(stage).into_iter().filter(|file| {
        Path::new(file)
            .extension()
            .is_some_and(|extension| extension == "xml" || extension == "qs")
    });
    for file in templates {
        let text = read(&stage.join(&file));
        assert!(!text.contains("{{"), "{target} {file}: {text}");
    }
    let config = read(&stage.join("config").join("config.xml"));
    for expected in [
        format!("<TargetDir>{target_dir}</TargetDir>"),
        format!("<Version>{VERSION}</Version>"),
    ] {
        assert!(config.contains(&expected), "{target}: {config}");
    }
    let packages = stage.join("packages");
    for (package, version) in [("workshop", VERSION), ("gateway", GATEWAY_VERSION)] {
        let xml = read(&packages.join(format!("com.runvil.{package}/meta/package.xml")));
        for expected in [
            format!("<Version>{version}</Version>"),
            format!("<ReleaseDate>{RELEASE_DATE}</ReleaseDate>"),
        ] {
            assert!(xml.contains(&expected), "{target}: {xml}");
        }
    }
    let license = std::fs::read(repository_root().join("LICENSE")).expect("LICENSE");
    for package in ["workshop", "gateway"] {
        let copy = packages.join(format!("com.runvil.{package}/meta/LICENSE.txt"));
        assert_eq!(std::fs::read(copy).expect("license copy"), license);
    }
    let gateway_script = read(&packages.join("com.runvil.gateway/meta/installscript.qs"));
    let gateway_executable = if target.ends_with("apple-darwin") {
        "Runvil Gateway.app/Contents/MacOS/runvil-gateway"
    } else {
        "runvil-gateway"
    };
    assert!(
        gateway_script.contains(&format!("\"/{gateway_executable}\"")),
        "{target}: {gateway_script}"
    );
    // The Workshop script reads its executable only on Linux.
    if !target.ends_with("apple-darwin") {
        let workshop_script = read(&packages.join("com.runvil.workshop/meta/installscript.qs"));
        assert!(
            workshop_script.contains("\"/Runvil.AppImage\""),
            "{target}: {workshop_script}"
        );
    }
}

/// The installer's own icons and the payload copies are byte-identical to
/// their sources, the copies are executable, and each Linux package holds
/// its own icon.
fn check_data(stage: &Path, sources: &Sources, macos: bool) {
    for name in ["icon.icns", "icon.png"] {
        assert_eq!(
            std::fs::read(stage.join("config").join(name)).expect("staged icon"),
            std::fs::read(sources.icons.join(name)).expect("Workshop's icon"),
            "{name}"
        );
    }
    let packages = stage.join("packages");
    let icon = std::fs::read(sources.icons.join("icon.png")).expect("icon");
    let items = if macos {
        [
            "Runvil.app/Contents/MacOS/runvil-workshop",
            "Runvil Gateway.app/Contents/MacOS/runvil-gateway",
        ]
    } else {
        ["Runvil.AppImage", "runvil-gateway"]
    };
    for (package, item) in ["workshop", "gateway"].into_iter().zip(items) {
        let data = packages.join(format!("com.runvil.{package}")).join("data");
        let copy = data.join(item);
        assert_eq!(std::fs::read(&copy).expect("data copy"), b"program");
        assert_executable(&copy);
        if !macos {
            let png = data.join(format!("runvil-{package}.png"));
            assert_eq!(std::fs::read(png).expect("package icon"), icon);
        }
    }
}

#[cfg(unix)]
fn assert_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755, "{}", path.display());
}

#[cfg(not(unix))]
fn assert_executable(_path: &Path) {}

fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).expect("read directory") {
        let path = entry.expect("entry").path();
        let destination = to.join(path.file_name().expect("name"));
        if path.is_dir() {
            copy_tree(&path, &destination);
        } else {
            write_file(&destination, &std::fs::read(&path).expect("read"));
        }
    }
}

#[test]
fn an_unknown_placeholder_fails_naming_the_file_and_the_placeholder() {
    let test_environment = environment();
    let temp = test_environment.temp.path();
    let checked_in = Sources::from_environment(&test_environment.environment);
    // The test edits a template, so it stages a copy of the definition.
    let definition = temp.join("definition");
    copy_tree(&checked_in.definition, &definition);
    let sources = Sources {
        definition,
        ..checked_in
    };
    let package = sources
        .definition
        .join("packages")
        .join("com.runvil.gateway")
        .join("meta")
        .join("package.xml");
    write_file(&package, b"<Package><Name>{{NOT_A_VALUE}}</Name></Package>");
    let payload = payload(temp, false);
    let platform = Platform::from_triple("x86_64-unknown-linux-gnu").expect("platform");

    let error = ifw::stage(
        &sources,
        &payload,
        platform,
        RELEASE_DATE,
        GATEWAY_VERSION,
        &temp.join("ifw"),
    )
    .expect_err("unknown placeholder");

    assert!(error.contains(&package.display().to_string()), "{error}");
    assert!(error.contains("{{NOT_A_VALUE}}"), "{error}");
}

#[cfg(unix)]
#[test]
fn a_symlink_in_the_payload_is_refused_naming_its_path() {
    let test_environment = environment();
    let temp = test_environment.temp.path();
    let payload = payload(temp, true);
    let link = payload
        .join("Runvil.app")
        .join("Contents")
        .join("Frameworks");
    std::os::unix::fs::symlink("/usr/lib", &link).expect("symlink");
    let platform = Platform::from_triple("aarch64-apple-darwin").expect("platform");

    let error = ifw::stage(
        &Sources::from_environment(&test_environment.environment),
        &payload,
        platform,
        RELEASE_DATE,
        GATEWAY_VERSION,
        &temp.join("ifw"),
    )
    .expect_err("symlink");

    assert!(error.contains(&link.display().to_string()), "{error}");
}

/// A PNG's width and height from its IHDR chunk, which follows the 8-byte
/// signature and the chunk's length and type.
fn png_size(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).expect("PNG");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{}", path.display());
    assert_eq!(&bytes[12..16], b"IHDR", "{}", path.display());
    let number = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().expect("u32"));
    (number(16), number(20))
}

/// The text of each `<name>` element in `xml`.
fn element<'a>(xml: &'a str, name: &str) -> &'a str {
    let open = format!("<{name}>");
    let start = xml.find(&open).expect("element") + open.len();
    let end = xml[start..].find("</").expect("closing tag") + start;
    &xml[start..end]
}

/// The icons are the two the test Workshop checkout holds, which are the
/// two the stage copies from a real one.
#[test]
fn the_definition_names_only_files_it_holds_with_exact_double_size_images() {
    let test_environment = environment();
    let sources = Sources::from_environment(&test_environment.environment);
    let config_dir = sources.definition.join("config");
    let config = read(&config_dir.join("config.xml"));
    for name in ["Logo", "Watermark"] {
        let image = config_dir.join(element(&config, name));
        let (width, height) = png_size(&image);
        let double = image.with_file_name(
            image
                .file_name()
                .expect("name")
                .to_string_lossy()
                .replace(".png", "@2x.png"),
        );
        assert_eq!(
            png_size(&double),
            (width * 2, height * 2),
            "{}",
            double.display()
        );
    }
    assert!(
        sources
            .icons
            .join(format!(
                "{}.icns",
                element(&config, "InstallerApplicationIcon")
            ))
            .is_file()
    );
    assert!(
        sources
            .icons
            .join(format!(
                "{}.png",
                element(&config, "InstallerApplicationIcon")
            ))
            .is_file()
    );
    assert!(
        sources
            .icons
            .join(element(&config, "InstallerWindowIcon"))
            .is_file()
    );
    let packages = sources.definition.join("packages");
    let gateway = read(&packages.join("com.runvil.gateway/meta/installscript.qs"));
    assert!(gateway.contains("{{GATEWAY_EXECUTABLE}}"), "{gateway}");
    let workshop = read(&packages.join("com.runvil.workshop/meta/installscript.qs"));
    assert!(workshop.contains("{{WORKSHOP_EXECUTABLE}}"), "{workshop}");
    for package in ["workshop", "gateway"] {
        let xml = read(&packages.join(format!("com.runvil.{package}/meta/package.xml")));
        assert!(xml.contains("file=\"LICENSE.txt\""), "{xml}");
        assert_eq!(element(&xml, "Script"), "installscript.qs");
    }
}

#[test]
fn a_hidden_page_list_comes_with_a_default_wizard_size() {
    // IFW's 800-wide default applies only beside a visible page list; without
    // one the wizard opens at its minimum size and clips its pages.
    let test_environment = environment();
    let sources = Sources::from_environment(&test_environment.environment);
    let config = read(&sources.definition.join("config/config.xml"));
    assert_eq!(element(&config, "WizardShowPageList"), "false");
    for name in ["WizardDefaultWidth", "WizardDefaultHeight"] {
        let pixels: u32 = element(&config, name).parse().expect(name);
        assert!(pixels > 0, "{name}");
    }
}

#[test]
fn the_release_date_is_the_utc_calendar_date() {
    for (seconds, date) in [
        (0, "1970-01-01"),
        (951_782_400, "2000-02-29"),
        (1_709_208_000, "2024-02-29"),
        (1_798_761_599, "2026-12-31"),
        (1_798_761_600, "2027-01-01"),
        (4_107_542_400, "2100-03-01"),
    ] {
        assert_eq!(ifw::utc_date(seconds), date, "{seconds}");
    }
}

#[test]
fn a_prerelease_or_build_suffix_is_left_off_the_ifw_version() {
    for (version, expected) in [
        ("0.3.0", "0.3.0"),
        ("0.4.0-rc.1", "0.4.0"),
        ("1.2.3+build.7", "1.2.3"),
    ] {
        assert_eq!(ifw::ifw_version(version), expected, "{version}");
    }
}
