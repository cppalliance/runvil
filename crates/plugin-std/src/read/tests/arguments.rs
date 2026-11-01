//! The argument rules: what Read refuses before it touches a file, and in
//! what order.

use runvil_plugin::ToolErrorKind;
use serde_json::{Value, json};

use super::{new_call, numbered_file, read, seeded, text_of};

/// The error kind and message of a refused call over an empty filesystem.
fn refused(args: &Value) -> (ToolErrorKind, String) {
    let call = new_call();
    match read(&call, args) {
        Ok(output) => panic!("{args} is refused, but it answered {:?}", output.text()),
        Err(error) => (error.kind(), error.to_string()),
    }
}

#[test]
fn a_limit_below_one_is_refused_with_the_value_as_given() {
    for (limit, passed) in [(json!(0), "0"), (json!(-3), "-3")] {
        let (kind, message) = refused(&json!({ "path": "/a.md", "limit": limit }));
        assert_eq!(kind, ToolErrorKind::InvalidArguments);
        assert_eq!(
            message,
            format!("std_Read: `limit` must be at least 1, and the call passed {passed}")
        );
    }
}

#[test]
fn a_string_offset_is_refused_and_not_coerced() {
    let (kind, message) = refused(&json!({ "path": "/a.md", "offset": "3" }));
    assert_eq!(kind, ToolErrorKind::InvalidArguments);
    assert_eq!(
        message,
        "std_Read: `offset` must be a JSON integer, and the call passed \"3\""
    );
}

#[test]
fn a_string_limit_and_a_fractional_offset_are_refused() {
    let (_, message) = refused(&json!({ "path": "/a.md", "limit": "3" }));
    assert_eq!(
        message,
        "std_Read: `limit` must be a JSON integer, and the call passed \"3\""
    );
    let (_, message) = refused(&json!({ "path": "/a.md", "offset": 1.5 }));
    assert_eq!(
        message,
        "std_Read: `offset` must be a JSON integer, and the call passed 1.5"
    );
}

#[test]
fn an_integer_that_does_not_fit_in_64_bits_is_refused() {
    for name in ["offset", "limit"] {
        let mut args = json!({ "path": "/a.md" });
        args[name] = json!(u64::MAX);
        let (kind, message) = refused(&args);
        assert_eq!(kind, ToolErrorKind::InvalidArguments);
        assert_eq!(
            message,
            format!(
                "std_Read: `{name}` must be a JSON integer that fits in 64 bits, \
                 and the call passed 18446744073709551615"
            )
        );
    }
}

#[test]
fn a_whole_number_written_as_a_float_reads_as_that_integer() {
    let call = seeded("/a.md", numbered_file(10, true).as_bytes());
    for (written, plain) in [
        (json!({ "offset": 3.0 }), json!({ "offset": 3 })),
        (json!({ "offset": -2.0 }), json!({ "offset": -2 })),
        (json!({ "offset": 0.0 }), json!({ "offset": 0 })),
        (json!({ "limit": 2.0 }), json!({ "limit": 2 })),
        (
            json!({ "offset": 4.0, "limit": 3.0 }),
            json!({ "offset": 4, "limit": 3 }),
        ),
        (
            json!({ "offset": -9_223_372_036_854_775_808.0_f64 }),
            json!({ "offset": i64::MIN }),
        ),
    ] {
        let mut written_args = written.clone();
        written_args["path"] = json!("/a.md");
        let mut plain_args = plain;
        plain_args["path"] = json!("/a.md");
        assert_eq!(
            text_of(read(&call, &written_args)),
            text_of(read(&call, &plain_args)),
            "{written}"
        );
    }
}

#[test]
fn a_whole_number_float_past_the_64_bit_range_is_refused() {
    for (name, passed) in [
        ("offset", json!(1e30)),
        ("limit", json!(1e30)),
        ("offset", json!(9_223_372_036_854_775_808.0_f64)),
        ("offset", json!(-1e19)),
    ] {
        let mut args = json!({ "path": "/a.md" });
        args[name] = passed.clone();
        let (kind, message) = refused(&args);
        assert_eq!(kind, ToolErrorKind::InvalidArguments);
        assert_eq!(
            message,
            format!(
                "std_Read: `{name}` must be a JSON integer that fits in 64 bits, \
                 and the call passed {passed}"
            )
        );
    }
}

#[test]
fn a_float_limit_below_one_is_refused_with_the_value_as_given() {
    let (kind, message) = refused(&json!({ "path": "/a.md", "limit": 0.0 }));
    assert_eq!(kind, ToolErrorKind::InvalidArguments);
    assert_eq!(
        message,
        "std_Read: `limit` must be at least 1, and the call passed 0.0"
    );
}

#[test]
fn an_undeclared_key_is_refused_naming_it_and_the_parameters() {
    let (kind, message) = refused(&json!({ "path": "/a.md", "lines": 2 }));
    assert_eq!(kind, ToolErrorKind::InvalidArguments);
    assert_eq!(
        message,
        "std_Read: `lines` is not a parameter of this tool. \
         The parameters are `limit`, `offset` and `path`."
    );
}

#[test]
fn a_blank_path_is_refused_before_any_io() {
    for (blank, shown) in [("", ""), ("   ", "   ")] {
        let (kind, message) = refused(&json!({ "path": blank }));
        assert_eq!(kind, ToolErrorKind::InvalidArguments);
        assert_eq!(
            message,
            format!(
                "std_Read: `path` is empty or only whitespace, and the call passed \"{shown}\""
            )
        );
    }
}

#[test]
fn a_missing_path_is_refused_by_the_shared_check() {
    let (kind, message) = refused(&json!({}));
    assert_eq!(kind, ToolErrorKind::InvalidArguments);
    assert_eq!(
        message,
        "std_Read: `path` is required, and the call left it out"
    );
}

#[test]
fn the_unknown_key_is_refused_before_the_blank_path() {
    let (_, message) = refused(&json!({ "path": " ", "bogus": 1 }));
    assert!(message.contains("`bogus` is not a parameter"), "{message}");
}

#[test]
fn the_blank_path_is_refused_before_the_limit() {
    let (_, message) = refused(&json!({ "path": "", "limit": 0 }));
    assert!(
        message.contains("`path` is empty or only whitespace"),
        "{message}"
    );
}

#[test]
fn the_limit_is_refused_before_the_file_is_looked_up() {
    let (kind, message) = refused(&json!({ "path": "/missing.md", "limit": 0 }));
    assert_eq!(kind, ToolErrorKind::InvalidArguments);
    assert!(message.contains("`limit` must be at least 1"), "{message}");
}

#[test]
fn a_missing_file_is_a_plain_message_and_not_an_invalid_arguments_error() {
    let (kind, message) = refused(&json!({ "path": "/missing.md" }));
    assert_ne!(kind, ToolErrorKind::InvalidArguments);
    assert_eq!(message, "File not found: /missing.md");
}

#[test]
fn a_null_offset_and_a_null_limit_read_as_absent() {
    let call = seeded("/a.md", b"one\ntwo\n");
    let plain = text_of(read(&call, &json!({ "path": "/a.md" })));
    let nulls = text_of(read(
        &call,
        &json!({ "path": "/a.md", "offset": null, "limit": null }),
    ));
    assert_eq!(plain, nulls);
    assert_eq!(plain, "     1|one\n     2|two\n");
}

#[test]
fn a_null_argument_list_is_refused_for_its_missing_path() {
    let (kind, message) = refused(&Value::Null);
    assert_eq!(kind, ToolErrorKind::InvalidArguments);
    assert_eq!(
        message,
        "std_Read: `path` is required, and the call left it out"
    );
}
