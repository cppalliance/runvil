//! One fixture library's whole lifecycle through `insert_plugin_dir`: it is
//! loaded, invoked, unloaded exactly once, and closed. The fixture's own
//! counters show the first three, read through a second handle on the
//! library the loader opened; the operating system's already-loaded query
//! shows the last.

use harness_plugin_loader::insert_plugin_dir;
use libloading::Library;
use runvil_plugin::{PackOrigin, PackRegistry};

use crate::support::{already_loaded, copy_fixture, counter, remove, scratch};

/// What the fixture's three Plugin counters read.
#[derive(Debug, PartialEq, Eq)]
struct Counters {
    /// Successful `RV_PLUGIN_LOAD` calls.
    loads: u32,
    /// `RV_PLUGIN_UNLOAD` calls that freed an instance.
    unloads: u32,
    /// Instances made and not yet dropped.
    live: u32,
}

/// Reads all three counters through `library`.
fn counters(library: &Library) -> Counters {
    Counters {
        loads: counter(library, "loads"),
        unloads: counter(library, "unloads"),
        live: counter(library, "live"),
    }
}

#[test]
fn the_fixture_is_loaded_invoked_unloaded_once_and_closed() {
    let dir = scratch("the_fixture_is_loaded_invoked_unloaded_once_and_closed");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let mut packs = PackRegistry::new();
    let report = insert_plugin_dir(&mut packs, &dir).expect("the directory reads");
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    let [plugin] = report.plugins() else {
        panic!("one Plugin loads, got {:?}", report.plugins());
    };
    assert_eq!(
        format!("{} {}", plugin.name(), plugin.version()),
        "test-plugin 0.1.0",
        "the load described the Plugin, so its entry point was invoked"
    );
    let inserted: Vec<(&str, &str, PackOrigin)> = packs
        .list()
        .filter(|(_, origin)| *origin != PackOrigin::Builtin)
        .map(|(info, origin)| (info.fqn, info.description, origin))
        .collect();
    assert_eq!(
        inserted,
        [(
            "fixture/test",
            "The loader's test fixture",
            PackOrigin::Plugin
        )],
        "the load inserted the fixture's one Pack"
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
            unloads: 0,
            live: 1
        },
        "the Pack in the registry still holds the Plugin"
    );
    drop(packs);
    assert_eq!(
        counters(&library),
        Counters {
            loads: 1,
            unloads: 1,
            live: 0
        },
        "dropping the last holder sent exactly one unload, which freed the instance"
    );
    drop(library);
    assert!(
        already_loaded(&file).is_err(),
        "the library closed once the loader and this test both let go of it"
    );
    remove(&dir);
}
