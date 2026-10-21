//! Tests for the `RvHost` boundary: what `describe_plugin` and `refuse`
//! accept, what they answer RV_ERR_MISUSE, which call they answer, and
//! which failure the loader reports when a Plugin misuses the table.

use std::sync::atomic::Ordering;

use crate::fakes::*;
use crate::host::HOST;

/// What a name that is not one Pack id segment is reported as.
macro_rules! bad_name {
    () => {
        "the name is not one Pack id segment, such as `acme-url`"
    };
}

/// The reason of a failure caused by a misused `RvHost` table.
macro_rules! misuse_reason {
    ($detail:expr) => {
        concat!("the Plugin misused the RvHost table: ", $detail)
    };
}

/// Defines a test whose fake load runs `$misuse`, an `RvHost` call that
/// must answer RV_ERR_MISUSE, and then returns RV_OK. The load must fail
/// with `$detail`, and a failed load is never unloaded.
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
            assert_eq!(failure.reason(), misuse_reason!($detail));
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
        // misuse; the call is live.
        unsafe { (host(call).describe_plugin)(call, ptr::null()) }
    }
);

misuse_test!(
    a_misaligned_info_is_misuse,
    "describe_plugin was handed a misaligned info",
    |call| {
        // SAFETY: the loader's function answers misuse for a misaligned
        // info without reading it, so address 1 is safe to hand it; the
        // call is live.
        unsafe { (host(call).describe_plugin)(call, ptr::without_provenance(1)) }
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
    a_call_other_than_the_open_one_is_misuse,
    "an RvHost function was handed a call other than the open RV_PLUGIN_LOAD call",
    |call| {
        let mut other = RvCall {
            size: 0,
            verb: RV_PLUGIN_LOAD,
            host: ptr::null(),
            plugin: ptr::null_mut(),
            pack_run: ptr::null_mut(),
            args: ptr::null(),
        };
        let other: *const RvCall = &raw mut other;
        describe_as(call, other, &info(rv_str("acme-url"), rv_str("1.0.0")))
    }
);

misuse_test!(
    a_null_call_is_misuse,
    "an RvHost function was handed a call other than the open RV_PLUGIN_LOAD call",
    |call| refuse_as(call, ptr::null(), rv_str("no"))
);

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
        failure.reason(),
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
    assert_eq!(failure.reason(), "the Plugin refused: ");
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
    assert_eq!(failure.reason(), "the Plugin refused: first");
    assert_eq!(plug::SEEN.load(Ordering::SeqCst), RV_ERR_MISUSE as usize);
}

#[test]
fn a_refusal_wins_over_a_misuse_made_before_it() {
    fake!(plug, |call| {
        describe(call, "", "1.0.0");
        refuse(call, "no thanks")
    });
    assert_eq!(
        failure(plug::entry).reason(),
        "the Plugin refused: no thanks"
    );
}

#[test]
fn a_misuse_followed_by_a_non_ok_status_reports_the_misuse() {
    fake!(plug, |call| {
        describe(call, "", "1.0.0");
        ODD_STATUS
    });
    assert_eq!(failure(plug::entry).reason(), misuse_reason!(bad_name!()));
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
    assert_eq!(failure.reason(), misuse_reason!(bad_name!()));
    assert_eq!(plug::SEEN.load(Ordering::SeqCst), RV_ERR_MISUSE as usize);
}

#[test]
fn an_rvhost_function_called_from_another_thread_is_misuse() {
    fake!(
        plug,
        {
            pub(super) static SEEN: AtomicUsize = AtomicUsize::new(0);
        },
        |call| {
            let address = call as usize;
            let (described, refused) = std::thread::spawn(move || {
                let call = ptr::with_exposed_provenance_mut::<RvCall>(address);
                (
                    describe(call, "acme-url", "1.0.0"),
                    refuse(call, "from another thread"),
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
fn an_rvhost_function_called_after_the_load_returned_is_misuse() {
    fake!(plug, |call| describe(call, "acme-url", "1.0.0"));
    let _plugin = loaded(plug::entry);
    let stale = plug::LAST_CALL
        .load(Ordering::SeqCst)
        .cast::<RvCall>()
        .cast_const();
    let info = info(rv_str("other"), rv_str("2.0.0"));
    // SAFETY: the loader's functions never dereference the call, so a
    // stale pointer is safe to hand them, and `info` is live.
    let described = unsafe { (HOST.describe_plugin)(stale, &raw const info) };
    // SAFETY: as above.
    let refused = unsafe { (HOST.refuse)(stale, rv_str("too late")) };
    assert_eq!(described, RV_ERR_MISUSE);
    assert_eq!(refused, RV_ERR_MISUSE);
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
fn the_table_reports_version_0_1() {
    assert_eq!(HOST.api_major, 0);
    assert_eq!(HOST.api_minor, 1);
    assert_eq!(HOST.reserved, 0);
    assert_eq!(usize::try_from(HOST.size), Ok(size_of::<RvHost>()));
}
