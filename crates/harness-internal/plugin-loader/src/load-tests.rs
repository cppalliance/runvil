//! Tests for listing and opening Plugin libraries: which files in a Plugin
//! directory count, in what order, with what paths, and how a library
//! without `runvil_plugin` fails.

use std::env::consts::DLL_EXTENSION;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::load::{is_regular_file, plugin_files};

/// Tells apart the directories one process makes; nextest runs each test
/// in its own process, so the process id tells those apart.
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

/// A directory made for one test at `<parent>/<label>-<pid>-<n>`, removed
/// on drop. Its name reads no clock and no random generator.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// Makes the directory under the system temporary directory.
    fn new(label: &str) -> Scratch {
        Scratch::under(&std::env::temp_dir().join("runvil-tests"), label)
    }

    /// Makes the directory under `parent`, first removing anything an
    /// earlier process with the same id left at that path.
    fn under(parent: &Path, label: &str) -> Scratch {
        let n = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!("{label}-{}-{n}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).expect("a stale scratch directory is removed");
        }
        fs::create_dir_all(&path).expect("the scratch directory is made");
        Scratch { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let removed = fs::remove_dir_all(&self.path);
        assert!(
            removed.is_ok() || std::thread::panicking(),
            "the scratch directory {} must be removed: {removed:?}",
            self.path.display()
        );
    }
}

/// An empty file `name` in `dir`.
fn touch(dir: &Path, name: &str) {
    fs::write(dir.join(name), b"").expect("the file is written");
}

/// The name of a library file with this platform's extension.
fn library_name(stem: &str) -> String {
    format!("{stem}.{DLL_EXTENSION}")
}

/// The file names of `paths`, in order.
fn file_names(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|path| {
            path.file_name()
                .expect("a listed path has a file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn plugin_files_sorts_by_file_name_in_code_point_order() {
    let dir = Scratch::new("plugin_files_sorts_by_file_name");
    // Made out of order, with a capital that a case-insensitive listing,
    // such as NTFS's, would put second.
    for stem in ["c-plugin", "a-plugin", "B-plugin"] {
        touch(dir.path(), &library_name(stem));
    }
    let files = plugin_files(dir.path()).expect("the directory reads");
    assert_eq!(
        file_names(&files),
        [
            library_name("B-plugin"),
            library_name("a-plugin"),
            library_name("c-plugin"),
        ]
    );
}

#[test]
fn plugin_files_keeps_only_files_with_the_platform_library_extension() {
    let dir = Scratch::new("plugin_files_keeps_only_the_library_extension");
    touch(dir.path(), &library_name("plugin"));
    for other in [
        "README.txt".to_owned(),
        "notes".to_owned(),
        format!("plugin.{DLL_EXTENSION}.txt"),
        format!("plugin.{DLL_EXTENSION}.1"),
        DLL_EXTENSION.to_owned(),
    ] {
        touch(dir.path(), &other);
    }
    let files = plugin_files(dir.path()).expect("the directory reads");
    assert_eq!(file_names(&files), [library_name("plugin")]);
}

#[test]
fn plugin_files_lists_only_the_top_level_and_skips_directories() {
    let dir = Scratch::new("plugin_files_skips_directories");
    touch(dir.path(), &library_name("top"));
    // A directory named like a library is not a library, and neither is
    // anything inside a subdirectory.
    for sub in [library_name("nested"), "sub".to_owned()] {
        fs::create_dir(dir.path().join(&sub)).expect("the subdirectory is made");
        touch(&dir.path().join(&sub), &library_name("inner"));
    }
    let files = plugin_files(dir.path()).expect("the directory reads");
    assert_eq!(file_names(&files), [library_name("top")]);
}

#[test]
fn plugin_files_makes_the_paths_of_a_relative_directory_absolute() {
    // The one test that makes its directory under the working directory,
    // because a relative path from there to the temporary directory does
    // not exist when the two are on different Windows drives. An empty
    // parent keeps the path relative.
    let dir = Scratch::under(Path::new(""), "plugin_files_makes_paths_absolute");
    assert!(dir.path().is_relative());
    touch(dir.path(), &library_name("plugin"));
    let files = plugin_files(dir.path()).expect("the directory reads");
    let expected = std::path::absolute(dir.path().join(library_name("plugin")))
        .expect("the working directory is known");
    assert!(expected.is_absolute());
    assert_eq!(files, [expected]);
}

#[test]
fn plugin_files_returns_the_error_for_a_directory_it_cannot_read() {
    let dir = Scratch::new("plugin_files_returns_the_read_error");
    let error =
        plugin_files(&dir.path().join("missing")).expect_err("a missing directory is an I/O error");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn is_regular_file_tells_a_file_from_a_directory() {
    let dir = Scratch::new("is_regular_file_tells_a_file_from_a_directory");
    touch(dir.path(), "file");
    fs::create_dir(dir.path().join("sub")).expect("the subdirectory is made");
    assert!(is_regular_file(fs::metadata(dir.path().join("file"))).expect("a file reads"));
    assert!(!is_regular_file(fs::metadata(dir.path().join("sub"))).expect("a directory reads"));
}

#[test]
fn is_regular_file_treats_a_missing_entry_as_not_a_file() {
    // What `metadata` returns for a link that leads nowhere, or for an
    // entry removed after the directory was listed.
    let missing = Err(io::Error::from(io::ErrorKind::NotFound));
    assert!(!is_regular_file(missing).expect("a missing entry is not an error"));

    let dir = Scratch::new("is_regular_file_treats_a_missing_entry");
    let missing = fs::metadata(dir.path().join("missing"));
    assert!(!is_regular_file(missing).expect("a real missing path is not an error"));
}

#[test]
fn is_regular_file_returns_an_error_that_is_not_a_missing_entry() {
    // A permission error, for one, must not make an entry vanish from the
    // listing.
    let denied = Err(io::Error::from(io::ErrorKind::PermissionDenied));
    let error = is_regular_file(denied).expect_err("a permission error is returned");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);

    let other = Err(io::Error::other("the metadata could not be read"));
    let error = is_regular_file(other).expect_err("any other error is returned");
    assert_eq!(error.to_string(), "the metadata could not be read");
}

/// A library every machine of this platform has, and that exports no
/// `runvil_plugin`.
#[cfg(any(
    windows,
    all(target_os = "linux", target_env = "gnu"),
    target_os = "macos"
))]
fn system_library() -> PathBuf {
    #[cfg(windows)]
    {
        let root = std::env::var_os("SystemRoot").expect("Windows sets SystemRoot");
        PathBuf::from(root).join("System32").join("kernel32.dll")
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        PathBuf::from("libm.so.6")
    }
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/usr/lib/libSystem.B.dylib")
    }
}

#[cfg(any(
    windows,
    all(target_os = "linux", target_env = "gnu"),
    target_os = "macos"
))]
#[test]
fn a_library_without_runvil_plugin_fails_as_a_missing_entry_point() {
    use crate::load::{LoadError, open_entry};

    let error = open_entry(&system_library())
        .expect_err("a library with no `runvil_plugin` does not open as a Plugin");
    assert!(matches!(error, LoadError::MissingEntry(_)), "{error:?}");
    assert!(
        error.to_string().starts_with("runvil_plugin is missing: "),
        "{error}"
    );
}
