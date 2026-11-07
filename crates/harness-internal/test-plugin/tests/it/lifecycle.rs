//! One fixture library's whole lifecycle through `insert_plugin_dir`: it is
//! loaded, invoked, unloaded exactly once, and closed. The fixture's own
//! counters show the first three, read through a second handle on the
//! library the loader opened; the operating system's already-loaded query
//! shows the last.

use std::path::Path;

use harness_plugin_loader::insert_plugin_dir;
use libloading::Library;
use runvil_plugin::PackRegistry;

use crate::support::{copy_fixture, remove, scratch};

/// What the fixture's three counters read.
#[derive(Debug, PartialEq, Eq)]
struct Counters {
    /// Successful `RV_PLUGIN_LOAD` calls.
    loads: u32,
    /// `RV_PLUGIN_UNLOAD` calls that freed an instance.
    unloads: u32,
    /// Instances made and not yet dropped.
    live: u32,
}

/// Opens the library at the absolute `path` only when the process already
/// has it loaded. The handle adds a reference, so the library stays mapped
/// until the handle drops.
#[cfg(windows)]
fn already_loaded(path: &Path) -> Result<Library, libloading::Error> {
    libloading::os::windows::Library::open_already_loaded(path).map(Library::from)
}

/// Opens the library at the absolute `path` only when the process already
/// has it loaded. The handle adds a reference, so the library stays mapped
/// until the handle drops.
#[cfg(unix)]
fn already_loaded(path: &Path) -> Result<Library, libloading::Error> {
    // SAFETY: with `RTLD_NOLOAD` the call loads nothing, so no library
    // initializer runs; it only adds a reference to one already loaded.
    let library = unsafe {
        libloading::os::unix::Library::open(Some(path), libc::RTLD_NOLOAD | libc::RTLD_LAZY)
    };
    library.map(Library::from)
}

/// Calls the counter the fixture exports as `name` through `library`.
fn counter(library: &Library, name: &str) -> u32 {
    // SAFETY: the fixture exports each counter as `extern "C" fn() -> u32`.
    let read = unsafe { library.get::<extern "C" fn() -> u32>(name.as_bytes()) }
        .unwrap_or_else(|error| panic!("the fixture exports {name}: {error}"));
    read()
}

/// Reads all three counters through `library`.
fn counters(library: &Library) -> Counters {
    Counters {
        loads: counter(library, "harness_test_plugin_loads"),
        unloads: counter(library, "harness_test_plugin_unloads"),
        live: counter(library, "harness_test_plugin_live"),
    }
}

#[test]
fn the_fixture_is_loaded_invoked_unloaded_once_and_closed() {
    let dir = scratch("the_fixture_is_loaded_invoked_unloaded_once_and_closed");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let report = insert_plugin_dir(&mut PackRegistry::new(), &dir).expect("the directory reads");
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    let [plugin] = report.plugins() else {
        panic!("one Plugin loads, got {:?}", report.plugins());
    };
    assert_eq!(
        format!("{} {}", plugin.name(), plugin.version()),
        "test-plugin 0.1.0",
        "the load described the Plugin, so its entry point was invoked"
    );

    let library = already_loaded(&file).expect("the loader holds the library open");
    assert_eq!(
        counters(&library),
        Counters {
            loads: 1,
            unloads: 0,
            live: 1
        },
        "one load made one instance, and nothing unloaded it yet"
    );
    drop(report);
    assert_eq!(
        counters(&library),
        Counters {
            loads: 1,
            unloads: 1,
            live: 0
        },
        "dropping the report sent exactly one unload, which freed the instance"
    );
    drop(library);
    assert!(
        already_loaded(&file).is_err(),
        "the library closed once the loader and this test both let go of it"
    );
    remove(&dir);
}
