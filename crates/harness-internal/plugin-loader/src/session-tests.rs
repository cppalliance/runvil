//! Tests for the checks every function a Plugin calls back begins with:
//! what a string field and a struct pointer are refused for, and which
//! call a function answers.

use std::ffi::c_void;
use std::ptr;

use runvil_plugin_sys::{RV_ERR_MISUSE, RV_ERR_REFUSED, RV_OK, RvPluginInfo, RvStr};

use super::{
    Field, LOAD, PACK_NEW, Shape, TOOL_CALL, Verb, answer, closed, field, open, refuse, rv_str,
    shape,
};

const WORD: Field = field!("the word", 4);
const INFO: Shape = shape!("describe_plugin", "info", "RvPluginInfo");

fn bytes(bytes: &[u8]) -> RvStr {
    RvStr {
        ptr: bytes.as_ptr().cast(),
        len: bytes.len(),
    }
}

#[test]
fn a_field_copies_a_string_within_its_bound() {
    // SAFETY: the string points at a live literal of its length.
    let word = unsafe { WORD.read(rv_str("word")) };
    assert_eq!(word, Ok("word".to_owned()));
    let null_and_empty = RvStr {
        ptr: ptr::null(),
        len: 0,
    };
    // SAFETY: a zero length reads no byte.
    let empty = unsafe { WORD.read(null_and_empty) };
    assert_eq!(empty, Ok(String::new()));
}

#[test]
fn a_field_names_each_way_a_string_breaks_its_rules() {
    // SAFETY: the length is refused before any byte is read.
    let too_long = unsafe { WORD.read(rv_str("words")) };
    assert_eq!(too_long, Err("the word is longer than 4 bytes"));
    let null = RvStr {
        ptr: ptr::null(),
        len: 2,
    };
    // SAFETY: a NULL pointer is refused before any byte is read.
    let null = unsafe { WORD.read(null) };
    assert_eq!(
        null,
        Err("the word has a NULL pointer and a nonzero length")
    );
    // SAFETY: the string points at a live literal of its length.
    let not_utf8 = unsafe { WORD.read(bytes(&[0xff, 0xfe])) };
    assert_eq!(not_utf8, Err("the word is not UTF-8"));
}

#[test]
fn a_shape_names_each_way_a_struct_is_handed_over_wrongly() {
    let whole = RvPluginInfo {
        size: 40,
        reserved: 0,
        name: rv_str("a"),
        version: rv_str("1"),
    };
    // SAFETY: `whole` is a live `RvPluginInfo` of its stated size.
    assert!(unsafe { INFO.read(&raw const whole) }.is_ok());
    // SAFETY: NULL is refused before anything is read.
    let null = unsafe { INFO.read(ptr::null::<RvPluginInfo>()) };
    assert_eq!(null.err(), Some("describe_plugin was handed a NULL info"));
    let odd = (&raw const whole).wrapping_byte_add(1);
    // SAFETY: a misaligned pointer is refused before anything is read.
    let misaligned = unsafe { INFO.read(odd) };
    assert_eq!(
        misaligned.err(),
        Some("describe_plugin was handed a misaligned info")
    );
    let short = RvPluginInfo { size: 39, ..whole };
    // SAFETY: `short` is whole; only its `size` field claims less.
    let short = unsafe { INFO.read(&raw const short) };
    assert_eq!(
        short.err(),
        Some("the info's size is shorter than RvPluginInfo")
    );
}

#[test]
fn a_function_answers_only_the_open_call_of_its_own_verb_through_its_own_ctx() {
    for (verb, family, other) in [
        (Verb::Load, &LOAD, &PACK_NEW),
        (Verb::PackNew, &PACK_NEW, &TOOL_CALL),
        (Verb::ToolCall, &TOOL_CALL, &LOAD),
    ] {
        let (status, session) = open(verb, |ctx| answer(ctx, family, |_| Ok(RV_OK)));
        assert_eq!(status, RV_OK, "{verb:?}");
        assert_eq!(session.misuse, None, "{verb:?}");

        let (status, session) = open(verb, |ctx| answer(ctx, other, |_| Ok(RV_OK)));
        assert_eq!(status, RV_ERR_MISUSE, "{verb:?}");
        assert_eq!(session.misuse, Some(other.wrong_call), "{verb:?}");

        let (status, session) = open(verb, |_| answer(ptr::null_mut(), family, |_| Ok(RV_OK)));
        assert_eq!(status, RV_ERR_MISUSE, "{verb:?}");
        assert_eq!(session.misuse, Some(family.wrong_ctx), "{verb:?}");
    }
}

#[test]
fn a_function_called_with_no_call_open_answers_misuse_and_reads_nothing() {
    let mut stale = 0_u64;
    let stale: *mut c_void = (&raw mut stale).cast();
    assert_eq!(answer(stale, &LOAD, |_| Ok(RV_OK)), RV_ERR_MISUSE);
    let (status, session) = open(Verb::Load, |ctx| {
        closed(|| answer(ctx, &LOAD, |_| Ok(RV_OK)))
    });
    assert_eq!(
        status, RV_ERR_MISUSE,
        "a call with no arguments runs with no session open"
    );
    assert_eq!(session.misuse, None, "and it records nothing");
}

#[test]
fn only_the_first_misuse_of_a_call_is_kept() {
    let (_, session) = open(Verb::Load, |ctx| {
        answer(ctx, &LOAD, |_| Err("first"));
        answer(ctx, &LOAD, |_| Err("second"))
    });
    assert_eq!(session.misuse, Some("first"));
}

#[test]
fn a_call_inside_a_call_keeps_the_outer_session_open() {
    let (status, outer) = open(Verb::Load, |outer_ctx| {
        let (inner_status, inner) = open(Verb::PackNew, |inner_ctx| {
            // The outer call's `ctx` is not the open call's meanwhile.
            answer(outer_ctx, &LOAD, |_| Ok(RV_OK));
            answer(inner_ctx, &PACK_NEW, |_| Ok(RV_OK))
        });
        assert_eq!(inner_status, RV_OK);
        assert_eq!(inner.misuse, Some(LOAD.wrong_ctx));
        answer(outer_ctx, &LOAD, |_| Ok(RV_OK))
    });
    assert_eq!(status, RV_OK, "the outer call is open again");
    assert_eq!(outer.misuse, None);
}

#[test]
fn a_refusal_is_recorded_once_with_its_reason() {
    let (status, session) = open(Verb::PackNew, |ctx| {
        let first = refuse(ctx, &PACK_NEW, rv_str("no network"));
        assert_eq!(first, RV_ERR_REFUSED);
        refuse(ctx, &PACK_NEW, rv_str("again"))
    });
    assert_eq!(status, RV_ERR_MISUSE);
    assert_eq!(session.refusal.as_deref(), Some("no network"));
    assert_eq!(session.misuse, Some("refuse was called twice"));
}
