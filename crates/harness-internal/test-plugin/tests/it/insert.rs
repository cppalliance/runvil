//! `insert_plugin_dir` against copies of the fixture library: what loads,
//! what fails and why, what is ignored, and that every library closes.
//! Each test drops its report, then removes its directory.

use std::fs;
use std::io;
use std::path::Path;

use harness_plugin_loader::{PluginReport, PluginSource, insert_plugin_dir};
use runvil_plugin::PackRegistry;

use crate::support::{copy_fixture, remove, scratch};

/// Inserts every library in `dir` into a fresh Pack registry.
fn insert(dir: &Path) -> PluginReport {
    insert_plugin_dir(&mut PackRegistry::new(), dir).expect("the directory reads")
}

/// The path `std::path::absolute` makes of `path`, as a library source.
fn library_source(path: &Path) -> PluginSource {
    PluginSource::Library(std::path::absolute(path).expect("the working directory is known"))
}

#[test]
fn insert_plugin_dir_loads_the_fixture() {
    let dir = scratch("insert_plugin_dir_loads_the_fixture");
    let file = copy_fixture(&dir, "plugin");
    let report = insert(&dir);
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    let [plugin] = report.plugins() else {
        panic!("one Plugin loads, got {:?}", report.plugins());
    };
    assert_eq!(
        format!("{} {}", plugin.name(), plugin.version()),
        "test-plugin 0.1.0"
    );
    assert_eq!(plugin.source(), &library_source(&file));
    drop(report);
    remove(&dir);
}

#[test]
fn the_same_fixture_in_two_directories_loads_in_both() {
    let first_dir = scratch("the_same_fixture_in_two_directories_first");
    let second_dir = scratch("the_same_fixture_in_two_directories_second");
    let first_file = copy_fixture(&first_dir, "plugin");
    let second_file = copy_fixture(&second_dir, "plugin");
    let first = insert(&first_dir);
    let second = insert(&second_dir);
    for (report, file) in [(&first, &first_file), (&second, &second_file)] {
        assert!(report.failures().is_empty(), "{:?}", report.failures());
        let [plugin] = report.plugins() else {
            panic!("one Plugin loads, got {:?}", report.plugins());
        };
        assert_eq!(plugin.name(), "test-plugin");
        assert_eq!(plugin.source(), &library_source(file));
    }
    drop((first, second));
    remove(&first_dir);
    remove(&second_dir);
}

#[test]
fn a_garbage_library_fails_at_insert_and_the_fixture_beside_it_still_loads() {
    let dir = scratch("a_garbage_library_fails_at_insert");
    // `garbage` sorts before `plugin`, so the failure comes first.
    let garbage = dir.join(format!("garbage.{}", std::env::consts::DLL_EXTENSION));
    fs::write(&garbage, b"this is not a library").expect("the garbage file is written");
    let file = copy_fixture(&dir, "plugin");
    let report = insert(&dir);
    let [failure] = report.failures() else {
        panic!("one file fails, got {:?}", report.failures());
    };
    let reason = failure.reason();
    let cause = reason
        .strip_prefix("the library could not be opened: ")
        .unwrap_or_else(|| panic!("the reason starts with the open failure: {reason}"));
    // On Windows `libloading` reports only "LoadLibraryExW failed" and keeps
    // the system's reason as the error's source, so the text after the
    // prefix must carry that reason too.
    assert_ne!(cause, "LoadLibraryExW failed", "{reason}");
    #[cfg(windows)]
    assert!(
        cause.starts_with("LoadLibraryExW failed: ") && cause.ends_with("(os error 193)"),
        "a file that is not a Win32 module reports ERROR_BAD_EXE_FORMAT: {reason}"
    );
    #[cfg(not(windows))]
    assert!(!cause.is_empty(), "{reason}");
    assert_eq!(failure.source(), &library_source(&garbage));
    let [plugin] = report.plugins() else {
        panic!("the fixture loads, got {:?}", report.plugins());
    };
    assert_eq!(plugin.source(), &library_source(&file));
    drop(report);
    remove(&dir);
}

#[test]
fn a_file_without_the_library_extension_is_ignored() {
    let dir = scratch("a_file_without_the_library_extension_is_ignored");
    fs::write(dir.join("README.txt"), b"not a Plugin").expect("the README is written");
    copy_fixture(&dir, "plugin");
    let report = insert(&dir);
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    assert_eq!(report.plugins().len(), 1);
    drop(report);
    remove(&dir);
}

#[test]
fn a_second_copy_with_the_same_plugin_name_fails_as_a_duplicate() {
    let dir = scratch("a_second_copy_fails_as_a_duplicate");
    // The first by file name loads; the second repeats its Plugin name.
    let first = copy_fixture(&dir, "first");
    let second = copy_fixture(&dir, "second");
    let report = insert(&dir);
    let [plugin] = report.plugins() else {
        panic!("one Plugin loads, got {:?}", report.plugins());
    };
    assert_eq!(plugin.source(), &library_source(&first));
    let [failure] = report.failures() else {
        panic!("the repeat fails, got {:?}", report.failures());
    };
    assert_eq!(
        failure.reason(),
        "a Plugin named test-plugin was already loaded from this directory"
    );
    assert_eq!(failure.source(), &library_source(&second));
    drop(report);
    remove(&dir);
}

#[test]
fn a_missing_directory_returns_an_io_error() {
    let dir = scratch("a_missing_directory_returns_an_io_error");
    let error = insert_plugin_dir(&mut PackRegistry::new(), &dir.join("missing"))
        .expect_err("a missing directory is an I/O error");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    remove(&dir);
}

/// The library is open as long as any `Arc` of its Plugin is, not as long
/// as the report is. Only Windows refuses to delete an open library, so
/// only there can a test see it.
#[cfg(windows)]
#[test]
fn a_library_stays_open_until_the_last_arc_of_its_plugin_drops() {
    use std::sync::Arc;

    let dir = scratch("a_library_stays_open_until_the_last_arc_drops");
    let file = copy_fixture(&dir, "plugin");
    let report = insert(&dir);
    let plugin = Arc::clone(&report.plugins()[0]);
    drop(report);
    assert!(
        fs::remove_file(&file).is_err(),
        "the clone of the Plugin still holds the library open"
    );
    drop(plugin);
    remove(&dir);
}
