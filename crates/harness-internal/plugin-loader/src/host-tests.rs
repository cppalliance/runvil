//! Tests for the load boundary: the `RvHost` table, what `describe_plugin`
//! and `refuse` accept, what they answer RV_ERR_MISUSE, which `ctx` they
//! answer, and which failure the loader reports when a Plugin misuses
//! them.

use std::sync::atomic::Ordering;

use crate::fakes::*;
use crate::host::HOST;

/// What a name that is not one name segment is reported as.
macro_rules! bad_name {
    () => {
        "the name is not one name segment, such as `acme-url`"
    };
}

/// What a Pack name that is not a vendor and a Pack id is reported as.
macro_rules! bad_pack_name {
    () => {
        "the Pack's name is not a vendor and a Pack id joined by `/`, such as `acme/url`"
    };
}

/// The reason of a failure caused by misused load functions.
macro_rules! misuse_reason {
    ($detail:expr) => {
        concat!("the Plugin misused the load functions: ", $detail)
    };
}

/// What a load function handed a `ctx` other than the open load's is
/// reported as.
macro_rules! other_ctx {
    () => {
        "a load function was handed a ctx other than the open RV_PLUGIN_LOAD call's"
    };
}

/// Defines a test whose fake load runs `$misuse`, a load function call
/// that must answer RV_ERR_MISUSE, and then returns RV_OK. The load must
/// fail with `$detail`, and a failed load is never unloaded.
macro_rules! misuse_test {
    ($test:ident, $detail:expr, |$call:ident| $misuse:expr) => {
        #[test]
        fn $test() {
            fake!(
                plug,
                {
                    pub(super) static SEEN: AtomicUsize = AtomicUsize::new(0);
                },
                |$call| {
                    let status: RvStatus = $misuse;
                    SEEN.store(status as usize, Ordering::SeqCst);
                    RV_OK
                }
            );
            let failure = failure(plug::entry);
            assert_eq!(failure.message, misuse_reason!($detail));
            assert_eq!(reason(&failure), Some("misuse"));
            assert_eq!(plug::SEEN.load(Ordering::SeqCst), RV_ERR_MISUSE as usize);
            assert_eq!(plug::UNLOADS.load(Ordering::SeqCst), 0);
        }
    };
}

misuse_test!(
    a_name_over_64_bytes_is_misuse,
    "the name is longer than 64 bytes",
    |call| describe(call, &"a".repeat(65), "1.0.0")
);

misuse_test!(
    a_name_with_a_null_pointer_and_a_length_is_misuse,
    "the name has a NULL pointer and a nonzero length",
    |call| describe_info(
        call,
        &info(
            RvStr {
                ptr: ptr::null(),
                len: 4
            },
            rv_str("1.0.0")
        )
    )
);

misuse_test!(a_null_empty_name_is_misuse, bad_name!(), |call| {
    describe_info(
        call,
        &info(
            RvStr {
                ptr: ptr::null(),
                len: 0,
            },
            rv_str("1.0.0"),
        ),
    )
});

misuse_test!(a_name_with_two_segments_is_misuse, bad_name!(), |call| {
    describe(call, "acme/url", "1.0.0")
});

misuse_test!(
    a_name_with_an_uppercase_letter_is_misuse,
    bad_name!(),
    |call| describe(call, "Acme", "1.0.0")
);

misuse_test!(
    a_name_that_is_not_utf8_is_misuse,
    "the name is not UTF-8",
    |call| describe_info(call, &info(rv_str_bytes(&[0xff, 0xfe]), rv_str("1.0.0")))
);

misuse_test!(an_empty_version_is_misuse, "the version is empty", |call| {
    describe(call, "acme-url", "")
});

misuse_test!(
    a_version_over_64_bytes_is_misuse,
    "the version is longer than 64 bytes",
    |call| describe(call, "acme-url", &"1".repeat(65))
);

misuse_test!(
    a_version_with_a_control_character_is_misuse,
    "the version has a control character",
    |call| describe(call, "acme-url", "1.0\n0")
);

misuse_test!(
    a_version_with_a_null_pointer_and_a_length_is_misuse,
    "the version has a NULL pointer and a nonzero length",
    |call| describe_info(
        call,
        &info(
            rv_str("acme-url"),
            RvStr {
                ptr: ptr::null(),
                len: 3
            }
        )
    )
);

misuse_test!(
    a_short_info_size_is_misuse,
    "the info's size is shorter than RvPluginInfo",
    |call| describe_info(
        call,
        &RvPluginInfo {
            size: 0,
            ..info(rv_str("acme-url"), rv_str("1.0.0"))
        }
    )
);

misuse_test!(
    an_info_size_one_byte_short_is_misuse,
    "the info's size is shorter than RvPluginInfo",
    |call| {
        let whole = info(rv_str("acme-url"), rv_str("1.0.0"));
        describe_info(
            call,
            &RvPluginInfo {
                size: whole.size - 1,
                ..whole
            },
        )
    }
);

misuse_test!(
    a_nonzero_reserved_field_is_misuse,
    "the info's reserved field is not zero",
    |call| describe_info(
        call,
        &RvPluginInfo {
            reserved: 1,
            ..info(rv_str("acme-url"), rv_str("1.0.0"))
        }
    )
);

misuse_test!(
    a_null_info_is_misuse,
    "describe_plugin was handed a NULL info",
    |call| {
        // SAFETY: the loader's function accepts a NULL info and answers
        // misuse; the call's `ctx` is the open load's.
        unsafe { crate::host::describe_plugin(ctx(call), ptr::null()) }
    }
);

misuse_test!(
    a_misaligned_info_is_misuse,
    "describe_plugin was handed a misaligned info",
    |call| {
        // SAFETY: the loader's function answers misuse for a misaligned
        // info without reading it, so address 1 is safe to hand it; the
        // call's `ctx` is the open load's.
        unsafe { crate::host::describe_plugin(ctx(call), ptr::without_provenance(1)) }
    }
);

misuse_test!(
    a_second_description_is_misuse,
    "describe_plugin was called twice",
    |call| {
        describe(call, "acme-url", "1.0.0");
        describe(call, "acme-url", "1.0.0")
    }
);

misuse_test!(
    a_refusal_reason_over_1024_bytes_is_misuse,
    "the refusal reason is longer than 1024 bytes",
    |call| refuse(call, &"r".repeat(1025))
);

misuse_test!(
    a_refusal_reason_that_is_not_utf8_is_misuse,
    "the refusal reason is not UTF-8",
    |call| refuse_str(call, rv_str_bytes(&[0xc3, 0x28]))
);

misuse_test!(
    a_refusal_reason_with_a_null_pointer_and_a_length_is_misuse,
    "the refusal reason has a NULL pointer and a nonzero length",
    |call| refuse_str(
        call,
        RvStr {
            ptr: ptr::null(),
            len: 5
        }
    )
);

misuse_test!(
    a_ctx_other_than_the_open_loads_is_misuse,
    other_ctx!(),
    |call| {
        let mut other = 0_u64;
        let other: *mut c_void = (&raw mut other).cast();
        describe_as(call, other, &info(rv_str("acme-url"), rv_str("1.0.0")))
    }
);

misuse_test!(a_null_ctx_is_misuse, other_ctx!(), |call| refuse_as(
    call,
    ptr::null_mut(),
    rv_str("no")
));

// A Plugin that stays loaded while `the_ctx_of_another_loaded_plugin_is_misuse`
// loads a second one, which hands this one's `ctx` to its load functions.
fake!(kept, |call| describe(call, "kept-one", "1.0.0"));

#[test]
fn the_ctx_of_another_loaded_plugin_is_misuse() {
    let kept = loaded(kept::entry);
    fake!(
        plug,
        {
            pub(super) static SEEN: AtomicUsize = AtomicUsize::new(0);
        },
        |call| {
            let kept_ctx = super::kept::LOAD_CTX.load(Ordering::SeqCst);
            let status = describe_as(call, kept_ctx, &info(rv_str("acme-url"), rv_str("1.0.0")));
            SEEN.store(status as usize, Ordering::SeqCst);
            RV_OK
        }
    );
    let failure = failure(plug::entry);
    assert_eq!(failure.message, misuse_reason!(other_ctx!()));
    assert_eq!(reason(&failure), Some("misuse"));
    assert_eq!(plug::SEEN.load(Ordering::SeqCst), RV_ERR_MISUSE as usize);
    assert_ne!(
        plug::LOAD_CTX.load(Ordering::SeqCst),
        kept::LOAD_CTX.load(Ordering::SeqCst),
        "two loads with a Plugin still loaded get two ctx pointers"
    );
    assert_eq!(kept.name(), "kept-one", "the kept Plugin is untouched");
    assert_eq!(kept::UNLOADS.load(Ordering::SeqCst), 0);
}

#[test]
fn a_name_and_a_version_of_exactly_64_bytes_are_accepted() {
    fake!(plug, |call| describe(
        call,
        &"a".repeat(64),
        &"1".repeat(64)
    ));
    let plugin = loaded(plug::entry);
    assert_eq!(plugin.name(), "a".repeat(64));
    assert_eq!(plugin.version(), "1".repeat(64));
}

#[test]
fn a_refusal_reason_of_exactly_1024_bytes_is_accepted() {
    fake!(plug, |call| refuse(call, &"r".repeat(1024)));
    let failure = failure(plug::entry);
    assert_eq!(
        failure.message,
        format!("the Plugin refused: {}", "r".repeat(1024))
    );
}

#[test]
fn a_refusal_with_no_reason_is_accepted() {
    fake!(
        plug,
        {
            pub(super) static SEEN: AtomicUsize = AtomicUsize::new(0);
        },
        |call| {
            let status = refuse_str(
                call,
                RvStr {
                    ptr: ptr::null(),
                    len: 0,
                },
            );
            SEEN.store(status as usize, Ordering::SeqCst);
            RV_ERR_REFUSED
        }
    );
    let failure = failure(plug::entry);
    assert_eq!(failure.message, "the Plugin refused: ");
    assert_eq!(plug::SEEN.load(Ordering::SeqCst), RV_ERR_REFUSED as usize);
}

#[test]
fn a_second_refusal_is_misuse_and_the_first_is_reported() {
    fake!(
        plug,
        {
            pub(super) static SEEN: AtomicUsize = AtomicUsize::new(0);
        },
        |call| {
            refuse(call, "first");
            let second = refuse(call, "second");
            SEEN.store(second as usize, Ordering::SeqCst);
            RV_ERR_REFUSED
        }
    );
    let failure = failure(plug::entry);
    assert_eq!(failure.message, "the Plugin refused: first");
    assert_eq!(plug::SEEN.load(Ordering::SeqCst), RV_ERR_MISUSE as usize);
}

#[test]
fn a_refusal_wins_over_a_misuse_made_before_it() {
    fake!(plug, |call| {
        describe(call, "", "1.0.0");
        refuse(call, "no thanks")
    });
    assert_eq!(
        failure(plug::entry).message,
        "the Plugin refused: no thanks"
    );
}

#[test]
fn a_misuse_followed_by_a_non_ok_status_reports_the_misuse() {
    fake!(plug, |call| {
        describe(call, "", "1.0.0");
        ODD_STATUS
    });
    assert_eq!(failure(plug::entry).message, misuse_reason!(bad_name!()));
}

#[test]
fn a_misuse_is_reported_even_when_a_good_description_follows() {
    fake!(
        plug,
        {
            pub(super) static SEEN: AtomicUsize = AtomicUsize::new(0);
        },
        |call| {
            let status = describe(call, "Not A Name", "1.0.0");
            SEEN.store(status as usize, Ordering::SeqCst);
            describe(call, "acme-url", "1.0.0")
        }
    );
    let failure = failure(plug::entry);
    assert_eq!(failure.message, misuse_reason!(bad_name!()));
    assert_eq!(plug::SEEN.load(Ordering::SeqCst), RV_ERR_MISUSE as usize);
}

#[test]
fn a_load_function_called_from_another_thread_during_the_load_is_misuse() {
    fake!(
        plug,
        {
            pub(super) static SEEN: AtomicUsize = AtomicUsize::new(0);
        },
        |call| {
            let address = ctx(call) as usize;
            let (described, refused) = std::thread::spawn(move || {
                let ctx = ptr::with_exposed_provenance_mut::<c_void>(address);
                (
                    describe_with(ctx, "acme-url", "1.0.0"),
                    refuse_with(ctx, "from another thread"),
                )
            })
            .join()
            .expect("the thread finishes");
            SEEN.store(
                usize::from(described == RV_ERR_MISUSE && refused == RV_ERR_MISUSE),
                Ordering::SeqCst,
            );
            describe(call, "acme-url", "1.0.0")
        }
    );
    let plugin = loaded(plug::entry);
    assert_eq!(
        plug::SEEN.load(Ordering::SeqCst),
        1,
        "both calls from the other thread got misuse"
    );
    assert_eq!(plugin.name(), "acme-url");
}

#[test]
fn a_late_call_through_a_ctx_stashed_during_a_successful_load_is_misuse() {
    fake!(plug, |call| describe(call, "acme-url", "1.0.0"));
    let plugin = loaded(plug::entry);
    let stashed = plug::LOAD_CTX.load(Ordering::SeqCst);
    assert!(!stashed.is_null(), "the load was handed a ctx");
    assert_eq!(describe_with(stashed, "other", "2.0.0"), RV_ERR_MISUSE);
    assert_eq!(refuse_with(stashed, "too late"), RV_ERR_MISUSE);
    assert_eq!(plugin.name(), "acme-url", "the late calls changed nothing");
    assert_eq!(plug::UNLOADS.load(Ordering::SeqCst), 0);
}

#[test]
fn a_late_call_through_a_rejected_loads_ctx_is_misuse() {
    fake!(plug, |call| refuse(call, "no"));
    drop(failure(plug::entry));
    let stashed = plug::LOAD_CTX.load(Ordering::SeqCst);
    assert!(!stashed.is_null(), "the load was handed a ctx");
    assert_eq!(describe_with(stashed, "acme-url", "1.0.0"), RV_ERR_MISUSE);
}

#[test]
fn the_load_call_carries_the_contract_fields() {
    fake!(
        plug,
        {
            pub(super) static SEEN: AtomicUsize = AtomicUsize::new(0);
        },
        |call| {
            SEEN.store(
                usize::from(contract_holds(call, RV_PLUGIN_LOAD)),
                Ordering::SeqCst,
            );
            describe(call, "acme-url", "1.0.0")
        }
    );
    let _plugin = loaded(plug::entry);
    assert_eq!(plug::SEEN.load(Ordering::SeqCst), 1);
}

#[test]
fn the_table_reports_version_0_4_in_16_bytes() {
    assert_eq!(HOST.api_major, 0);
    assert_eq!(HOST.api_minor, 4);
    assert_eq!(HOST.reserved, 0);
    assert_eq!(HOST.size, 16);
    assert_eq!(usize::try_from(HOST.size), Ok(size_of::<RvHost>()));
}

/// Defines a test whose fake load describes itself, runs `$misuse`, an
/// `insert_pack` call that must answer RV_ERR_MISUSE, and then returns
/// RV_OK. The load must fail with `$detail`, and a failed load is never
/// unloaded.
macro_rules! pack_misuse_test {
    ($test:ident, $detail:expr, |$call:ident| $misuse:expr) => {
        misuse_test!($test, $detail, |$call| {
            describe($call, "acme-url", "1.0.0");
            $misuse
        });
    };
}

pack_misuse_test!(
    a_pack_name_without_a_vendor_is_misuse,
    bad_pack_name!(),
    |call| insert_pack(call, "url", "")
);

pack_misuse_test!(
    a_pack_name_with_three_segments_is_misuse,
    bad_pack_name!(),
    |call| insert_pack(call, "acme/url/extra", "")
);

pack_misuse_test!(
    a_pack_name_with_an_empty_segment_is_misuse,
    bad_pack_name!(),
    |call| insert_pack(call, "acme/", "")
);

pack_misuse_test!(
    a_pack_name_with_an_uppercase_letter_is_misuse,
    bad_pack_name!(),
    |call| insert_pack(call, "acme/Url", "")
);

pack_misuse_test!(
    a_pack_name_with_a_segment_over_64_bytes_is_misuse,
    bad_pack_name!(),
    |call| insert_pack(call, &format!("acme/{}", "a".repeat(65)), "")
);

pack_misuse_test!(
    a_pack_name_over_129_bytes_is_misuse,
    "the Pack's name is longer than 129 bytes",
    |call| insert_pack(call, &"a".repeat(130), "")
);

pack_misuse_test!(
    a_pack_name_that_is_not_utf8_is_misuse,
    "the Pack's name is not UTF-8",
    |call| insert_pack_info(call, &pack_info(rv_str_bytes(&[0xff, 0xfe]), rv_str("")))
);

pack_misuse_test!(
    a_pack_inserted_twice_is_misuse,
    "two Packs of one Plugin share a name",
    |call| {
        insert_pack(call, "acme/url", "");
        insert_pack(call, "acme/url", "again")
    }
);

pack_misuse_test!(
    a_pack_description_over_1024_bytes_is_misuse,
    "the Pack's description is longer than 1024 bytes",
    |call| insert_pack(call, "acme/url", &"d".repeat(1025))
);

pack_misuse_test!(
    a_pack_description_with_a_control_character_is_misuse,
    "the Pack's description has a control character",
    |call| insert_pack(call, "acme/url", "two\nlines")
);

pack_misuse_test!(
    a_short_pack_info_size_is_misuse,
    "the info's size is shorter than RvPackInfo",
    |call| insert_pack_info(
        call,
        &RvPackInfo {
            size: 8,
            ..pack_info(rv_str("acme/url"), rv_str(""))
        }
    )
);

pack_misuse_test!(
    a_nonzero_reserved_field_in_a_pack_info_is_misuse,
    "the info's reserved field is not zero",
    |call| insert_pack_info(
        call,
        &RvPackInfo {
            reserved: 1,
            ..pack_info(rv_str("acme/url"), rv_str(""))
        }
    )
);

pack_misuse_test!(
    a_null_pack_info_is_misuse,
    "insert_pack was handed a NULL info",
    |call| {
        // SAFETY: the loader's function accepts a NULL info and answers
        // misuse; the call's `ctx` is the open load's.
        unsafe { crate::host::insert_pack(ctx(call), ptr::null()) }
    }
);

pack_misuse_test!(
    a_pack_function_called_during_a_load_is_misuse,
    "a Pack function was called outside RV_PACK_NEW",
    |call| {
        let tool = tool_info("fetch");
        // SAFETY: the loader's function reads through `ctx` only when it
        // is the open call's, and answers misuse for a call of another
        // verb before it reads `tool`.
        unsafe { crate::pack::add_tool(ctx(call), &raw const tool) }
    }
);

pack_misuse_test!(
    finish_called_during_a_load_is_misuse,
    "a tool call function was called outside RV_TOOL_CALL",
    |call| {
        let result = result(RV_RESULT_TRUSTED, rv_str("done"));
        // SAFETY: as in the test above.
        unsafe { crate::call::finish(ctx(call), &raw const result) }
    }
);

#[test]
fn a_pack_name_and_a_description_at_their_bounds_are_accepted() {
    fake!(plug, |call| {
        describe(call, "acme-url", "1.0.0");
        insert_pack(
            call,
            &format!("{}/{}", "v".repeat(64), "p".repeat(64)),
            &"d".repeat(1024),
        )
    });
    let mut packs = runvil_plugin::PackRegistry::new();
    let _plugin = crate::plugin::insert_plugin(&mut packs, plug::entry).expect("the Plugin loads");
    let fqn = format!("{}/{}", "v".repeat(64), "p".repeat(64));
    let description = packs
        .list()
        .find(|(info, _)| info.fqn == fqn)
        .map(|(info, _)| info.description);
    assert_eq!(description, Some("d".repeat(1024).as_str()));
}
