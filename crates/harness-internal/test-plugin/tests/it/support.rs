//! Where the fixture library is, copies of it, the scratch directories
//! the tests put the copies in, and a second handle on a library the
//! loader opened, through which a test reads the fixture's counters.

use std::env::consts::{DLL_EXTENSION, DLL_PREFIX, DLL_SUFFIX};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use libloading::Library;

/// Tells apart the directories one process makes; nextest runs each test
/// in its own process, so the process id tells those apart.
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

/// The fixture library Cargo built beside this test executable. With
/// `crate-type = ["cdylib", "rlib"]`, Cargo builds the library before the
/// package's own integration tests run.
pub(crate) fn fixture_library() -> PathBuf {
    let exe = std::env::current_exe().expect("the test executable has a path");
    let dir = exe.parent().expect("the test executable has a directory");
    dir.join(format!("{DLL_PREFIX}harness_test_plugin{DLL_SUFFIX}"))
}

/// Makes a fresh directory for the test named `label`, at
/// `<target tmp>/<label>-<pid>-<n>`, first removing anything an earlier
/// process with the same id left there. Its name reads no clock and no
/// random generator.
pub(crate) fn scratch(label: &str) -> PathBuf {
    let n = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{label}-{}-{n}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("a stale scratch directory is removed");
    }
    fs::create_dir_all(&dir).expect("the scratch directory is made");
    dir
}

/// Copies the fixture library into `dir` as `<stem>.<library extension>`
/// and returns the copy's path.
pub(crate) fn copy_fixture(dir: &Path, stem: &str) -> PathBuf {
    let copy = dir.join(format!("{stem}.{DLL_EXTENSION}"));
    fs::copy(fixture_library(), &copy).expect("the fixture library is copied");
    copy
}

/// Removes `dir` and requires success. Windows refuses to delete an open
/// library, so a test that has dropped everything holding its copies
/// shows that every library closed when this succeeds. An antivirus
/// scanner can also hold a freshly copied or just-closed library, so check
/// for that before treating a Windows failure as a leak.
pub(crate) fn remove(dir: &Path) {
    fs::remove_dir_all(dir).expect("the scratch directory is removed, so no library is still open");
}

/// Opens the library at the absolute `path` only when the process already
/// has it loaded. The handle adds a reference, so the library stays mapped
/// until the handle drops.
#[cfg(windows)]
pub(crate) fn already_loaded(path: &Path) -> Result<Library, libloading::Error> {
    libloading::os::windows::Library::open_already_loaded(path).map(Library::from)
}

/// Opens the library at the absolute `path` only when the process already
/// has it loaded. The handle adds a reference, so the library stays mapped
/// until the handle drops.
#[cfg(unix)]
pub(crate) fn already_loaded(path: &Path) -> Result<Library, libloading::Error> {
    // SAFETY: with `RTLD_NOLOAD` the call loads nothing, so no library
    // initializer runs; it only adds a reference to one already loaded.
    let library = unsafe {
        libloading::os::unix::Library::open(Some(path), libc::RTLD_NOLOAD | libc::RTLD_LAZY)
    };
    library.map(Library::from)
}

/// Calls the counter the fixture exports as `harness_test_plugin_<name>`
/// through `library`.
pub(crate) fn counter(library: &Library, name: &str) -> u32 {
    let symbol = format!("harness_test_plugin_{name}");
    // SAFETY: the fixture exports each counter as `extern "C" fn() -> u32`.
    let read = unsafe { library.get::<extern "C" fn() -> u32>(symbol.as_bytes()) }
        .unwrap_or_else(|error| panic!("the fixture exports {symbol}: {error}"));
    read()
}
