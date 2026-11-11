//! Tests for the Packs a Plugin's entry point inserts: what reaches the
//! registry, what one run's instance holds, what `add_tool` and the Pack
//! `refuse` accept, and that RV_PACK_DELETE and RV_PLUGIN_UNLOAD each
//! come once, in that order.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use runvil_plugin::{ErrorCode, ErrorFact, PackOrigin, PackRegistry};
use serde_json::json;

use crate::fakes::*;
use crate::plugin::insert_plugin;

/// The reason a run goes without a Pack whose Plugin misused the Pack
/// functions.
macro_rules! misuse_reason {
    ($detail:expr) => {
        concat!("the Plugin fake misused the Pack functions: ", $detail)
    };
}

pack_fake!(
    good,
    {
        /// Whether the RV_PACK_NEW call carried the contract's fields, and
        /// the Pack name it was sent.
        pub(super) static WELL_FORMED: AtomicUsize = AtomicUsize::new(0);
        pub(super) static PACK_NAME: Mutex<String> = Mutex::new(String::new());
    },
    new: |call| {
        WELL_FORMED.store(usize::from(contract_holds(call, RV_PACK_NEW)), Ordering::SeqCst);
        if let Ok(mut name) = PACK_NAME.lock() {
            *name = text(pack_args(call).pack);
        }
        set_pack_run(call, instance(PACK_TAG));
        add_tool_info(
            call,
            &RvToolInfo {
                wire_name: rv_str("Fetch"),
                description: rv_str("Fetches a page."),
                parameters: rv_str(r#"{"type":"object","properties":{"url":{"type":"string"}}}"#),
                ..tool_info("fetch")
            },
        );
        add_tool(call, "search")
    },
    step: |_call| RV_ERR_UNSUPPORTED
);

#[test]
fn a_plugins_pack_goes_into_the_registry_under_its_name_with_its_description() {
    let mut packs = PackRegistry::new();
    let plugin = insert_plugin(&mut packs, good::entry).expect("the Plugin loads");
    assert_eq!(plugin.name(), "fake");
    let listed: Vec<(&str, &str, PackOrigin)> = packs
        .list()
        .filter(|(info, _)| info.fqn == FAKE_PACK)
        .map(|(info, origin)| (info.fqn, info.description, origin))
        .collect();
    assert_eq!(listed, [(FAKE_PACK, "A fake Pack", PackOrigin::Plugin)]);
}

#[test]
fn an_instance_holds_the_tools_the_plugin_added_under_the_pack_id() {
    let pack = made(good::entry);
    assert_eq!(good::WELL_FORMED.load(Ordering::SeqCst), 1);
    assert_eq!(*good::PACK_NAME.lock().expect("the lock holds"), FAKE_PACK);
    let tools = pack.tools();
    let listed: Vec<(String, Option<&str>, &str)> = tools
        .iter()
        .map(|tool| {
            (
                tool.id.to_string(),
                tool.wire_name.as_deref(),
                tool.description.as_str(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("fake/fetch".to_owned(), Some("Fetch"), "Fetches a page."),
            ("fake/search".to_owned(), None, "A fake tool."),
        ]
    );
    assert_eq!(
        tools[0].parameters_schema,
        json!({ "type": "object", "properties": { "url": { "type": "string" } } })
    );
    assert!(
        tools.iter().all(|tool| !tool.structured_output
            && !tool.survives_stop
            && !tool.describes_tools
            && !tool.model_only_call),
        "the contract carries no tool flags"
    );
}

#[test]
fn dropping_an_instance_sends_one_delete_with_the_pointer_it_set_and_then_the_unload() {
    let pack = made(good::entry);
    assert_eq!(good::DELETES.load(Ordering::SeqCst), 0);
    assert_eq!(
        good::UNLOADS.load(Ordering::SeqCst),
        0,
        "the instance keeps its Plugin loaded"
    );
    drop(pack);
    assert_eq!(good::DELETES.load(Ordering::SeqCst), 1);
    assert_eq!(good::DELETED.load(Ordering::SeqCst), instance(PACK_TAG));
    assert_eq!(
        *good::ORDER.lock().expect("the lock holds"),
        [RV_PACK_DELETE, RV_PLUGIN_UNLOAD],
        "the Plugin unloads only after its last Pack instance is deleted"
    );
}

pack_fake!(
    kept,
    {},
    new: |call| {
        set_pack_run(call, instance(PACK_TAG));
        add_tool(call, "fetch")
    },
    step: |_call| RV_ERR_UNSUPPORTED
);

#[test]
fn a_pack_in_the_registry_keeps_its_plugin_loaded_and_each_run_gets_its_own_instance() {
    let mut packs = PackRegistry::new();
    drop(insert_plugin(&mut packs, kept::entry).expect("the Plugin loads"));
    assert_eq!(
        kept::UNLOADS.load(Ordering::SeqCst),
        0,
        "the registry's Pack holds the Plugin"
    );
    let (_info, id, factory) = packs
        .into_entries()
        .find(|(info, _, _)| info.fqn == FAKE_PACK)
        .expect("the Plugin inserted its Pack");
    let factory = factory.expect("the Pack is usable");
    let first = factory.new_pack(&id).expect("the first run's instance");
    let second = factory.new_pack(&id).expect("the second run's instance");
    assert!(!Arc::ptr_eq(&first, &second));
    drop(factory);
    drop(first);
    assert_eq!(kept::DELETES.load(Ordering::SeqCst), 1);
    assert_eq!(
        kept::UNLOADS.load(Ordering::SeqCst),
        0,
        "the second instance still holds the Plugin"
    );
    drop(second);
    assert_eq!(kept::DELETES.load(Ordering::SeqCst), 2);
    assert_eq!(kept::UNLOADS.load(Ordering::SeqCst), 1);
}

#[test]
fn a_pack_the_registry_refuses_fails_the_insert_and_names_the_pack() {
    let mut packs = PackRegistry::new();
    let _first = insert_plugin(&mut packs, kept::entry).expect("the first Plugin loads");
    let failure = insert_plugin(&mut packs, good::entry).expect_err("the Pack name is taken");
    assert_eq!(failure.code, ErrorCode::PluginLoadFailed);
    assert_eq!(reason(&failure), Some("pack"));
    assert_eq!(failure.facts.get(&ErrorFact::Name), Some(&json!(FAKE_PACK)));
    assert_eq!(
        failure.message,
        "the Pack acme/fake of the Plugin fake was not inserted: Pack `acme/fake` is already in \
         the registry"
    );
}

#[test]
fn an_instance_with_no_tools_is_made() {
    pack_fake!(
        plug,
        {},
        new: |call| {
            set_pack_run(call, instance(PACK_TAG));
            RV_OK
        },
        step: |_call| RV_ERR_UNSUPPORTED
    );
    assert!(made(plug::entry).tools().is_empty());
}

#[test]
fn a_refusal_is_the_reason_the_run_goes_without_the_pack_and_gets_no_delete() {
    pack_fake!(
        plug,
        {},
        new: |call| {
            set_pack_run(call, instance(PACK_TAG));
            refuse_pack(call, "this run has no network")
        },
        step: |_call| RV_ERR_UNSUPPORTED
    );
    let failure = new_pack(plug::entry).err().expect("the Plugin refuses");
    assert_eq!(failure.message, "this run has no network");
    assert_eq!(failure.prose, "this run has no network");
    assert_eq!(plug::DELETES.load(Ordering::SeqCst), 0);
    assert_eq!(
        plug::UNLOADS.load(Ordering::SeqCst),
        1,
        "nothing holds the Plugin once the factory and the failed instance are gone"
    );
}

#[test]
fn a_status_other_than_ok_fails_the_instance_and_gets_no_delete() {
    pack_fake!(
        plug,
        {},
        new: |call| {
            set_pack_run(call, instance(PACK_TAG));
            add_tool(call, "fetch");
            ODD_STATUS
        },
        step: |_call| RV_ERR_UNSUPPORTED
    );
    let failure = new_pack(plug::entry).err().expect("the call fails");
    assert_eq!(
        failure.message,
        "the Plugin fake answered RV_PACK_NEW with status 77"
    );
    assert_eq!(plug::DELETES.load(Ordering::SeqCst), 0);
}

/// Defines a test whose fake RV_PACK_NEW sets an instance, runs `$misuse`,
/// a Pack function call that must answer RV_ERR_MISUSE, and then returns
/// RV_OK. The instance must fail with `$detail`, and a failed
/// RV_PACK_NEW is never deleted.
macro_rules! misuse_test {
    ($test:ident, $detail:expr, |$call:ident| $misuse:expr) => {
        #[test]
        fn $test() {
            pack_fake!(
                plug,
                {
                    pub(super) static SEEN: AtomicUsize = AtomicUsize::new(0);
                },
                new: |$call| {
                    set_pack_run($call, instance(PACK_TAG));
                    let status: RvStatus = $misuse;
                    SEEN.store(status as usize, Ordering::SeqCst);
                    RV_OK
                },
                step: |_call| RV_ERR_UNSUPPORTED
            );
            let failure = new_pack(plug::entry).err().expect("the instance fails");
            assert_eq!(failure.message, misuse_reason!($detail));
            assert_eq!(plug::SEEN.load(Ordering::SeqCst), RV_ERR_MISUSE as usize);
            assert_eq!(plug::DELETES.load(Ordering::SeqCst), 0);
        }
    };
}

misuse_test!(
    a_tool_name_with_two_segments_is_misuse,
    "the tool's name is not one name segment, such as `fetch`",
    |call| add_tool(call, "web/fetch")
);

misuse_test!(
    an_empty_tool_name_is_misuse,
    "the tool's name is not one name segment, such as `fetch`",
    |call| add_tool(call, "")
);

misuse_test!(
    a_tool_name_over_64_bytes_is_misuse,
    "the tool's name is longer than 64 bytes",
    |call| add_tool(call, &"a".repeat(65))
);

misuse_test!(
    a_tool_added_twice_is_misuse,
    "two tools of one Pack instance share a name",
    |call| {
        add_tool(call, "fetch");
        add_tool(call, "fetch")
    }
);

misuse_test!(
    a_wire_name_with_a_slash_is_misuse,
    "the tool's wire name is not ASCII letters, digits, `_`, and `-`",
    |call| add_tool_info(
        call,
        &RvToolInfo {
            wire_name: rv_str("web/fetch"),
            ..tool_info("fetch")
        }
    )
);

misuse_test!(
    a_wire_name_over_64_bytes_is_misuse,
    "the tool's wire name is longer than 64 bytes",
    |call| add_tool_info(
        call,
        &RvToolInfo {
            wire_name: rv_str(&"A".repeat(65)),
            ..tool_info("fetch")
        }
    )
);

misuse_test!(
    an_empty_description_is_misuse,
    "the tool's description is empty",
    |call| add_tool_info(
        call,
        &RvToolInfo {
            description: rv_str(""),
            ..tool_info("fetch")
        }
    )
);

misuse_test!(
    a_description_over_65536_bytes_is_misuse,
    "the tool's description is longer than 65536 bytes",
    |call| add_tool_info(
        call,
        &RvToolInfo {
            description: rv_str(&"d".repeat(65537)),
            ..tool_info("fetch")
        }
    )
);

misuse_test!(
    parameters_that_are_not_json_are_misuse,
    "the tool's parameter schema is not the JSON text of an object",
    |call| add_tool_info(
        call,
        &RvToolInfo {
            parameters: rv_str("{"),
            ..tool_info("fetch")
        }
    )
);

misuse_test!(
    parameters_that_are_not_an_object_are_misuse,
    "the tool's parameter schema is not the JSON text of an object",
    |call| add_tool_info(
        call,
        &RvToolInfo {
            parameters: rv_str("[]"),
            ..tool_info("fetch")
        }
    )
);

misuse_test!(
    parameters_over_65536_bytes_are_misuse,
    "the tool's parameter schema is longer than 65536 bytes",
    |call| add_tool_info(
        call,
        &RvToolInfo {
            parameters: rv_str(&" ".repeat(65537)),
            ..tool_info("fetch")
        }
    )
);

misuse_test!(
    a_short_tool_info_size_is_misuse,
    "the info's size is shorter than RvToolInfo",
    |call| add_tool_info(
        call,
        &RvToolInfo {
            size: 71,
            ..tool_info("fetch")
        }
    )
);

misuse_test!(
    a_nonzero_reserved_field_in_a_tool_info_is_misuse,
    "the info's reserved field is not zero",
    |call| add_tool_info(
        call,
        &RvToolInfo {
            reserved: 1,
            ..tool_info("fetch")
        }
    )
);

misuse_test!(
    a_null_tool_info_is_misuse,
    "add_tool was handed a NULL info",
    |call| {
        // SAFETY: the loader's function accepts a NULL info and answers
        // misuse; the `ctx` is the open call's.
        unsafe { crate::pack::add_tool(pack_args(call).ctx, ptr::null()) }
    }
);

misuse_test!(
    a_tool_added_through_another_ctx_is_misuse,
    "a Pack function was handed a ctx other than the open RV_PACK_NEW call's",
    |call| add_tool_as(call, ptr::null_mut(), &tool_info("fetch"))
);

#[test]
fn a_second_refusal_of_an_instance_is_misuse_and_the_first_is_the_reason() {
    pack_fake!(
        plug,
        {
            pub(super) static SEEN: AtomicUsize = AtomicUsize::new(0);
        },
        new: |call| {
            refuse_pack(call, "first");
            let second = refuse_pack(call, "second");
            SEEN.store(second as usize, Ordering::SeqCst);
            RV_ERR_REFUSED
        },
        step: |_call| RV_ERR_UNSUPPORTED
    );
    let failure = new_pack(plug::entry).err().expect("the Plugin refuses");
    assert_eq!(failure.message, "first");
    assert_eq!(plug::SEEN.load(Ordering::SeqCst), RV_ERR_MISUSE as usize);
}

misuse_test!(
    a_load_function_called_during_pack_new_is_misuse,
    "a load function was called outside RV_PLUGIN_LOAD",
    |call| describe_with(pack_args(call).ctx, "acme-url", "1.0.0")
);

#[test]
fn tool_texts_at_their_bounds_are_accepted() {
    pack_fake!(
        plug,
        {},
        new: |call| {
            set_pack_run(call, instance(PACK_TAG));
            let padding = " ".repeat(65536 - NO_PARAMETERS.len());
            add_tool_info(
                call,
                &RvToolInfo {
                    wire_name: rv_str(&"W".repeat(64)),
                    description: rv_str(&"d".repeat(65536)),
                    parameters: rv_str(&format!("{NO_PARAMETERS}{padding}")),
                    ..tool_info(&"t".repeat(64))
                },
            )
        },
        step: |_call| RV_ERR_UNSUPPORTED
    );
    let pack = made(plug::entry);
    let tools = pack.tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].id.name(), "t".repeat(64));
    assert_eq!(tools[0].wire_name.as_deref(), Some("W".repeat(64).as_str()));
    assert_eq!(tools[0].description.len(), 65536);
}
