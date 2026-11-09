use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{
    GATEWAY_BUNDLE_NAME, Layout, WORKSHOP_APPIMAGE_NAME, WORKSHOP_BUNDLE_NAME, app_bundle,
    bundle_exe, first_file, gateway_candidates, installed_gateway, running_appimage,
};

/// Plants an empty executable file at `root/relative`, creating its
/// directories.
fn plant(root: &Path, relative: &Path) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("a planted file has a parent"))
        .expect("create fixture directories");
    std::fs::write(&path, b"").expect("plant fixture file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("mark fixture executable");
    }
    path
}

fn gateway_for(layout: Layout, workshop_exe: &Path, appimage: Option<&Path>) -> Option<PathBuf> {
    first_file(gateway_candidates(layout, workshop_exe, appimage))
}

#[test]
fn windows_peers_sit_beside_each_other() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let workshop = plant(root.path(), Path::new("runvil-workshop.exe"));
    let gateway = plant(root.path(), Path::new("runvil-gateway.exe"));

    assert_eq!(gateway_for(Layout::Windows, &workshop, None), Some(gateway));
}

#[test]
fn a_development_tree_finds_the_gateway_beside_workshop() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let debug = Path::new("target").join("debug");
    let workshop = plant(root.path(), &debug.join("runvil-workshop"));
    let gateway = plant(root.path(), &debug.join("runvil-gateway"));

    for layout in [Layout::MacOs, Layout::Linux] {
        assert_eq!(
            gateway_for(layout, &workshop, None),
            Some(gateway.clone()),
            "{layout:?}"
        );
    }
}

#[test]
fn macos_peers_are_sibling_bundles() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let workshop = plant(
        root.path(),
        &bundle_exe(Path::new(""), WORKSHOP_BUNDLE_NAME, "runvil-workshop"),
    );
    let gateway = plant(
        root.path(),
        &bundle_exe(Path::new(""), GATEWAY_BUNDLE_NAME, "runvil-gateway"),
    );

    assert_eq!(gateway_for(Layout::MacOs, &workshop, None), Some(gateway));
}

#[test]
fn macos_bundle_lookup_needs_the_bundle_structure() {
    let root = tempfile::TempDir::new().expect("tempdir");
    plant(
        root.path(),
        &bundle_exe(Path::new(""), GATEWAY_BUNDLE_NAME, "runvil-gateway"),
    );
    let loose = root
        .path()
        .join("Contents")
        .join("MacOS")
        .join("runvil-workshop");

    assert_eq!(
        gateway_for(Layout::MacOs, &loose, None),
        None,
        "an executable outside a .app bundle has no bundle siblings"
    );
}

#[test]
fn linux_workshop_finds_the_gateway_beside_its_appimage() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let install = root.path().join("Runvil");
    let appimage = plant(&install, Path::new(WORKSHOP_APPIMAGE_NAME));
    let gateway = plant(&install, Path::new("runvil-gateway"));
    let mount = root
        .path()
        .join("mount")
        .join("usr")
        .join("bin")
        .join("runvil-workshop");

    assert_eq!(
        gateway_for(Layout::Linux, &mount, Some(appimage.as_path())),
        Some(gateway)
    );
}

#[test]
fn linux_workshop_prefers_the_gateway_beside_its_appimage_to_one_in_its_mount() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let install = root.path().join("Runvil");
    let appimage = plant(&install, Path::new(WORKSHOP_APPIMAGE_NAME));
    let installed = plant(&install, Path::new("runvil-gateway"));
    let bin = root.path().join("mount").join("usr").join("bin");
    let workshop = plant(&bin, Path::new("runvil-workshop"));
    plant(&bin, Path::new("runvil-gateway"));

    assert_eq!(
        gateway_for(Layout::Linux, &workshop, Some(appimage.as_path())),
        Some(installed)
    );
}

#[test]
fn an_appimage_counts_only_when_workshop_runs_from_its_mount() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let mount = root.path().join(".mount_PromptXYZ");
    let workshop = mount.join("usr").join("bin").join("runvil-workshop");
    let appimage = root.path().join("Runvil").join(WORKSHOP_APPIMAGE_NAME);
    let other_mount = root.path().join(".mount_CodeXYZ");
    let other_appimage = root.path().join("Apps").join("Code.AppImage");
    let found = |appimage: &OsStr, appdir: &OsStr| {
        running_appimage(&workshop, Some(appimage), Some(appdir))
    };

    assert_eq!(
        found(appimage.as_os_str(), mount.as_os_str()),
        Some(appimage.clone())
    );
    assert_eq!(
        found(other_appimage.as_os_str(), other_mount.as_os_str()),
        None,
        "an $APPIMAGE inherited from another AppImage is ignored"
    );
    assert_eq!(
        found(OsStr::new(WORKSHOP_APPIMAGE_NAME), mount.as_os_str()),
        None,
        "a relative $APPIMAGE is ignored"
    );
    assert_eq!(
        found(OsStr::new(""), mount.as_os_str()),
        None,
        "empty $APPIMAGE"
    );
    assert_eq!(
        found(appimage.as_os_str(), OsStr::new("")),
        None,
        "empty $APPDIR"
    );
    assert_eq!(
        running_appimage(&workshop, Some(appimage.as_os_str()), None),
        None
    );
    assert_eq!(
        running_appimage(&workshop, None, Some(mount.as_os_str())),
        None
    );
}

#[cfg(unix)]
#[test]
fn an_appimage_counts_when_appdir_reaches_the_mount_through_a_symlink() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let mount = root.path().join(".mount_PromptXYZ");
    std::fs::create_dir_all(&mount).expect("create the mount");
    let link = root.path().join("tmp-link");
    std::os::unix::fs::symlink(&mount, &link).expect("link to the mount");
    let workshop = std::fs::canonicalize(&mount)
        .expect("resolve the mount")
        .join("usr")
        .join("bin")
        .join("runvil-workshop");
    let appimage = root.path().join("Runvil").join(WORKSHOP_APPIMAGE_NAME);

    assert_eq!(
        running_appimage(
            &workshop,
            Some(appimage.as_os_str()),
            Some(link.as_os_str())
        ),
        Some(appimage)
    );
}

#[cfg(unix)]
#[test]
fn a_peer_without_an_execute_bit_is_not_found() {
    use std::os::unix::fs::PermissionsExt as _;
    let root = tempfile::TempDir::new().expect("tempdir");
    let workshop = plant(root.path(), Path::new("runvil-workshop"));
    let gateway = plant(root.path(), Path::new("runvil-gateway"));
    assert_eq!(
        gateway_for(Layout::Linux, &workshop, None),
        Some(gateway.clone())
    );
    std::fs::set_permissions(&gateway, std::fs::Permissions::from_mode(0o644))
        .expect("clear the execute bits");

    assert_eq!(gateway_for(Layout::Linux, &workshop, None), None);
}

#[test]
fn the_bundle_walks_up_from_contents_macos_to_the_app() {
    let exe = Path::new("/Applications/Runvil.app/Contents/MacOS/runvil-gateway");
    assert_eq!(app_bundle(exe), Some(Path::new("/Applications/Runvil.app")));
}

#[test]
fn the_bundle_probe_rejects_unbundled_and_partial_paths() {
    assert_eq!(app_bundle(Path::new("/usr/local/bin/runvil-gateway")), None);
    assert_eq!(
        app_bundle(Path::new("/Applications/Runvil.app/Contents/MacOS")),
        None,
        "the exe itself must sit below MacOS"
    );
    assert_eq!(
        app_bundle(Path::new("/opt/x/Contents/MacOS/runvil-gateway")),
        None,
        "the parent two levels up must be a .app"
    );
    assert_eq!(app_bundle(Path::new("runvil-gateway")), None);
}

#[test]
fn a_missing_peer_is_none_in_every_layout() {
    for layout in [Layout::Windows, Layout::MacOs, Layout::Linux] {
        let root = tempfile::TempDir::new().expect("tempdir");
        let exe = if matches!(layout, Layout::Windows) {
            "runvil-workshop.exe"
        } else {
            "runvil-workshop"
        };
        let workshop = plant(root.path(), Path::new(exe));
        plant(
            &root.path().join("elsewhere"),
            Path::new(layout.gateway_exe()),
        );

        assert_eq!(gateway_for(layout, &workshop, None), None, "{layout:?}");
    }
}

#[test]
fn a_missing_install_directory_is_none() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let absent = root.path().join("absent").join("runvil-gateway");
    assert_eq!(installed_gateway(&absent, None), None);
}
