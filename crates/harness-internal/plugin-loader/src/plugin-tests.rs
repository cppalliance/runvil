//! Tests for inserting a linked Plugin and for what owns it afterward:
//! what loads and what fails, how a failure's reason is chosen, and that
//! RV_PLUGIN_UNLOAD is sent exactly once, when the last `Arc` drops.

use std::cell::RefCell;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use runvil_plugin::PackRegistry;

use crate::fakes::*;
use crate::plugin::{Plugin, PluginFailure, PluginSource, insert_plugin};

/// A Plugin and a failure may cross threads, and a Plugin may be shared.
const _: () = {
    const fn send_and_sync<T: Send + Sync>() {}
    send_and_sync::<Plugin>();
    send_and_sync::<PluginFailure>();
};

#[test]
fn a_plugin_that_describes_itself_and_sets_its_instance_loads() {
    fake!(plug, |call| {
        set_plugin(call, instance(0x1000));
        describe(call, "acme-url", "1.2.3")
    });
    let plugin = loaded(plug::entry);
    assert_eq!(plugin.name(), "acme-url");
    assert_eq!(plugin.version(), "1.2.3");
    assert_eq!(plugin.source(), &PluginSource::Linked);
    assert_eq!(
        plug::UNLOADS.load(Ordering::SeqCst),
        0,
        "a live Plugin is not unloaded"
    );
}

#[test]
fn a_plugin_that_keeps_no_instance_loads() {
    fake!(plug, |call| describe(call, "stateless", "0.0.1"));
    let plugin = loaded(plug::entry);
    assert_eq!(plugin.name(), "stateless");
    assert_eq!(plugin.source(), &PluginSource::Linked);
}

#[test]
fn a_load_that_returns_ok_without_describing_fails() {
    fake!(plug, |_call| RV_OK);
    let failure = failure(plug::entry);
    assert_eq!(
        failure.reason(),
        "RV_PLUGIN_LOAD returned without describing the Plugin"
    );
    assert_eq!(failure.source(), &PluginSource::Linked);
}

#[test]
fn a_refusal_fails_with_its_reason() {
    fake!(plug, |call| refuse(call, "needs a license key"));
    let failure = failure(plug::entry);
    assert_eq!(failure.reason(), "the Plugin refused: needs a license key");
    assert_eq!(failure.source(), &PluginSource::Linked);
}

#[test]
fn a_refusal_wins_over_a_description_and_an_ok_status() {
    fake!(plug, |call| {
        describe(call, "acme-url", "1.0.0");
        refuse(call, "changed my mind");
        RV_OK
    });
    assert_eq!(
        failure(plug::entry).reason(),
        "the Plugin refused: changed my mind"
    );
}

#[test]
fn a_refusal_before_the_description_still_wins() {
    fake!(plug, |call| {
        refuse(call, "first");
        describe(call, "acme-url", "1.0.0");
        RV_OK
    });
    assert_eq!(failure(plug::entry).reason(), "the Plugin refused: first");
}

#[test]
fn a_non_ok_status_alone_reports_the_status() {
    fake!(plug, |_call| ODD_STATUS);
    assert_eq!(
        failure(plug::entry).reason(),
        "RV_PLUGIN_LOAD returned status 77"
    );
}

#[test]
fn a_non_ok_status_fails_a_load_that_described_itself() {
    fake!(plug, |call| {
        describe(call, "acme-url", "1.0.0");
        ODD_STATUS
    });
    assert_eq!(
        failure(plug::entry).reason(),
        "RV_PLUGIN_LOAD returned status 77"
    );
}

#[test]
fn dropping_the_arc_sends_one_unload_with_the_instance_it_set() {
    fake!(plug, |call| {
        set_plugin(call, instance(0x2a40));
        describe(call, "acme-url", "1.0.0")
    });
    let plugin = loaded(plug::entry);
    drop(plugin);
    assert_eq!(plug::UNLOADS.load(Ordering::SeqCst), 1);
    assert_eq!(plug::UNLOADED.load(Ordering::SeqCst), instance(0x2a40));
}

#[test]
fn an_unload_call_carries_the_contract_fields() {
    fake!(plug, |call| {
        set_plugin(call, instance(0x2a40));
        describe(call, "acme-url", "1.0.0")
    });
    drop(loaded(plug::entry));
    assert!(plug::UNLOAD_WELL_FORMED.load(Ordering::SeqCst));
}

#[test]
fn a_plugin_without_an_instance_is_unloaded_with_null() {
    fake!(plug, |call| describe(call, "stateless", "0.0.1"));
    drop(loaded(plug::entry));
    assert_eq!(plug::UNLOADS.load(Ordering::SeqCst), 1);
    assert!(plug::UNLOADED.load(Ordering::SeqCst).is_null());
}

#[test]
fn a_clone_keeps_the_plugin_loaded_until_it_drops() {
    fake!(plug, |call| {
        set_plugin(call, instance(0x77));
        describe(call, "acme-url", "1.0.0")
    });
    let plugin = loaded(plug::entry);
    let clone = Arc::clone(&plugin);
    drop(plugin);
    assert_eq!(
        plug::UNLOADS.load(Ordering::SeqCst),
        0,
        "the clone still holds the Plugin"
    );
    assert_eq!(clone.name(), "acme-url");
    drop(clone);
    assert_eq!(plug::UNLOADS.load(Ordering::SeqCst), 1);
}

#[test]
fn the_last_arc_may_drop_on_another_thread() {
    fake!(plug, |call| {
        set_plugin(call, instance(0x99));
        describe(call, "acme-url", "1.0.0")
    });
    let plugin = loaded(plug::entry);
    std::thread::spawn(move || drop(plugin))
        .join()
        .expect("the thread finishes");
    assert_eq!(plug::UNLOADS.load(Ordering::SeqCst), 1);
    assert_eq!(plug::UNLOADED.load(Ordering::SeqCst), instance(0x99));
}

#[test]
fn the_last_arc_may_drop_in_a_thread_local_destructor_after_the_session_is_gone() {
    thread_local! {
        static HOLDER: RefCell<Option<Arc<Plugin>>> = const { RefCell::new(None) };
    }
    fake!(plug, |call| {
        set_plugin(call, instance(0x31));
        describe(call, "acme-url", "1.0.0")
    });
    fake!(toucher, |call| describe(call, "toucher", "1.0.0"));
    let plugin = loaded(plug::entry);
    std::thread::spawn(move || {
        // `HOLDER` registers its destructor first, with the only `Arc`.
        HOLDER.with(|holder| *holder.borrow_mut() = Some(plugin));
        // This load is the first use of the loader's own thread-local
        // session on this thread, so that destructor registers second and
        // runs first when the thread exits, before `HOLDER` drops the `Arc`.
        drop(loaded(toucher::entry));
    })
    .join()
    .expect("the thread finishes");
    assert_eq!(plug::UNLOADS.load(Ordering::SeqCst), 1);
    assert_eq!(plug::UNLOADED.load(Ordering::SeqCst), instance(0x31));
    assert_eq!(
        plug::UNLOAD_STATUS.load(Ordering::SeqCst) - 1,
        RV_ERR_MISUSE as usize,
        "with no session slot left, the RvHost functions answer misuse"
    );
}

#[test]
fn an_rvhost_function_called_during_unload_is_misuse() {
    fake!(plug, |call| describe(call, "acme-url", "1.0.0"));
    drop(loaded(plug::entry));
    let tried = plug::UNLOAD_STATUS.load(Ordering::SeqCst);
    assert_ne!(
        tried, 0,
        "the fake tried describe_plugin during RV_PLUGIN_UNLOAD"
    );
    assert_eq!(tried - 1, RV_ERR_MISUSE as usize);
}

#[test]
fn a_load_that_sets_an_instance_and_then_refuses_is_never_unloaded() {
    fake!(plug, |call| {
        set_plugin(call, instance(0x5150));
        refuse(call, "no")
    });
    drop(failure(plug::entry));
    assert_eq!(plug::UNLOADS.load(Ordering::SeqCst), 0);
}

#[test]
fn a_load_that_sets_an_instance_and_then_fails_with_a_status_is_never_unloaded() {
    fake!(plug, |call| {
        set_plugin(call, instance(0x5150));
        describe(call, "acme-url", "1.0.0");
        ODD_STATUS
    });
    drop(failure(plug::entry));
    assert_eq!(plug::UNLOADS.load(Ordering::SeqCst), 0);
}

#[test]
fn a_load_that_sets_an_instance_and_misuses_the_table_is_never_unloaded() {
    fake!(plug, |call| {
        set_plugin(call, instance(0x5150));
        describe(call, "BAD", "1.0.0");
        RV_OK
    });
    drop(failure(plug::entry));
    assert_eq!(plug::UNLOADS.load(Ordering::SeqCst), 0);
}

#[test]
fn inserting_a_plugin_leaves_the_pack_registry_as_it_was() {
    fake!(plug, |call| describe(call, "acme-url", "1.0.0"));
    let mut packs = PackRegistry::new();
    let listed = |packs: &PackRegistry| {
        packs
            .list()
            .map(|(info, origin)| (info.fqn, origin))
            .collect::<Vec<_>>()
    };
    let before = listed(&packs);
    assert!(!before.is_empty(), "the registry holds its built-in Pack");
    let _plugin = insert_plugin(&mut packs, plug::entry).expect("the Plugin loads");
    assert_eq!(listed(&packs), before);
}

#[test]
fn a_plugin_shows_its_name_version_and_source_and_never_its_instance() {
    fake!(plug, |call| {
        set_plugin(call, instance(0x7a11));
        describe(call, "acme-url", "1.2.3")
    });
    let plugin = loaded(plug::entry);
    assert_eq!(
        format!("{plugin:?}"),
        r#"Plugin { name: "acme-url", version: "1.2.3", source: Linked, .. }"#
    );
}

/// The Plugin the nested-unload test's second load drops mid-load.
static HELD: Mutex<Option<Arc<Plugin>>> = Mutex::new(None);

fake!(held, |call| {
    set_plugin(call, instance(0x10));
    describe(call, "held-one", "1.0.0")
});

fake!(dropper, |call| {
    drop(super::HELD.lock().expect("the lock is not poisoned").take());
    describe(call, "dropper-one", "1.0.0")
});

#[test]
fn an_unload_during_another_load_does_not_taint_that_load() {
    *HELD.lock().expect("the lock is not poisoned") = Some(loaded(held::entry));
    let plugin = loaded(dropper::entry);
    assert_eq!(plugin.name(), "dropper-one");
    assert_eq!(held::UNLOADS.load(Ordering::SeqCst), 1);
    assert_eq!(
        held::UNLOAD_STATUS.load(Ordering::SeqCst) - 1,
        RV_ERR_MISUSE as usize,
        "the dropped Plugin's own call finds no session"
    );
}

fake!(inner, |call| {
    set_plugin(call, instance(0x20));
    describe(call, "inner-one", "1.0.0")
});

fake!(
    outer,
    {
        pub(super) static INNER_LOADED: AtomicUsize = AtomicUsize::new(0);
    },
    |call| {
        let inner = insert(super::inner::entry);
        INNER_LOADED.store(usize::from(inner.is_ok()), Ordering::SeqCst);
        drop(inner);
        describe(call, "outer-one", "1.0.0")
    }
);

#[test]
fn a_load_inside_a_load_leaves_the_outer_session_open() {
    let plugin = loaded(outer::entry);
    assert_eq!(plugin.name(), "outer-one");
    assert_eq!(outer::INNER_LOADED.load(Ordering::SeqCst), 1);
    assert_eq!(
        inner::UNLOADS.load(Ordering::SeqCst),
        1,
        "the inner Plugin dropped inside the load"
    );
}
