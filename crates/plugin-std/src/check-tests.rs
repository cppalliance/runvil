//! Argument checking: a matching call passes, and each kind of mismatch
//! fails naming the parameter.

use runvil_plugin::ErrorCode;
use serde_json::json;

use super::{check, invalid_pattern, plural, refuse_blank, refuse_unknown};
use crate::matcher::PatternError;

/// The schema the `StrReplace` tool ships, which has a required string, an
/// optional string, and an optional boolean.
fn replace() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "The path." },
            "old_string": { "type": "string", "description": "The old text." },
            "new_string": { "type": "string", "description": "The new text." },
            "replace_all": { "type": "boolean", "description": "Every occurrence." }
        },
        "required": ["path", "old_string", "new_string"]
    })
}

/// The refusal of `args` against `schema`, as the message and code.
fn refused(schema: &serde_json::Value, args: &serde_json::Value) -> (String, ErrorCode) {
    let Err(error) = check("StrReplace", schema, args) else {
        panic!("{args} is refused");
    };
    (error.to_string(), error.code)
}

#[test]
fn matching_arguments_pass() {
    let full = json!({
        "path": "/a.md",
        "old_string": "one",
        "new_string": "two",
        "replace_all": true
    });
    assert!(check("StrReplace", &replace(), &full).is_ok(), "{full}");
    let least = json!({ "path": "/a.md", "old_string": "one", "new_string": "two" });
    assert!(check("StrReplace", &replace(), &least).is_ok(), "{least}");
}

#[test]
fn a_null_argument_list_reads_as_an_empty_object() {
    let schema = json!({ "type": "object", "properties": {} });
    assert!(check("Read", &schema, &json!(null)).is_ok());
}

#[test]
fn a_missing_required_parameter_fails_naming_it() {
    let args = json!({ "path": "/a.md", "old_string": "one" });
    let (message, kind) = refused(&replace(), &args);
    assert_eq!(
        message,
        "std_StrReplace: `new_string` is required, and the call left it out"
    );
    assert_eq!(kind, ErrorCode::ToolInvalidArguments);
}

#[test]
fn a_required_parameter_passed_as_null_fails_naming_it() {
    let args = json!({ "path": null, "old_string": "one", "new_string": "two" });
    let (message, kind) = refused(&replace(), &args);
    assert_eq!(
        message,
        "std_StrReplace: `path` is required, and the call passed null"
    );
    assert_eq!(kind, ErrorCode::ToolInvalidArguments);
}

#[test]
fn a_parameter_of_the_wrong_type_fails_naming_it_and_the_value() {
    let args = json!({ "path": "/a.md", "old_string": "one", "new_string": 2 });
    let (message, kind) = refused(&replace(), &args);
    assert_eq!(
        message,
        "std_StrReplace: `new_string` must be a JSON string, and the call passed 2"
    );
    assert_eq!(kind, ErrorCode::ToolInvalidArguments);
}

#[test]
fn arguments_that_are_not_an_object_fail() {
    let schema = json!({ "type": "object", "properties": {} });
    let (message, kind) = refused(&schema, &json!(["a"]));
    assert_eq!(
        message,
        "std_StrReplace: the arguments must be a JSON object"
    );
    assert_eq!(kind, ErrorCode::ToolInvalidArguments);
}

#[test]
fn a_value_outside_an_enum_fails_naming_the_allowed_values() {
    let schema = json!({
        "type": "object",
        "properties": {
            "output_mode": { "type": "string", "enum": ["content", "count"] }
        }
    });
    let (message, kind) = refused(&schema, &json!({ "output_mode": "files" }));
    assert_eq!(
        message,
        "std_StrReplace: `output_mode` must be one of \"content\", \"count\", and the call passed \"files\""
    );
    assert_eq!(kind, ErrorCode::ToolInvalidArguments);
}

#[test]
fn an_integer_schema_accepts_a_whole_number_and_refuses_a_fraction() {
    let schema = json!({
        "type": "object",
        "properties": { "limit": { "type": "integer" } }
    });
    assert!(check("Read", &schema, &json!({ "limit": 3 })).is_ok());
    assert!(check("Read", &schema, &json!({ "limit": 3.5 })).is_err());
}

#[test]
fn an_array_schema_checks_every_element() {
    let schema = json!({
        "type": "object",
        "properties": {
            "paths": { "type": "array", "items": { "type": "string" } }
        }
    });
    assert!(check("Read", &schema, &json!({ "paths": ["/a", "/b"] })).is_ok());
    assert!(check("Read", &schema, &json!({ "paths": ["/a", 2] })).is_err());
}

#[test]
fn a_parameter_the_schema_does_not_declare_passes_so_the_tool_can_see_it() {
    let args = json!({ "path": "/a.md", "old_string": "one", "new_string": "two", "x": 1 });
    assert!(
        check("StrReplace", &replace(), &args).is_ok(),
        "extra arguments reach the tool, which refuses them itself"
    );
}

/// The schema the `Read` tool ships, whose properties are `path`,
/// `offset`, and `limit`.
fn read() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string" },
            "offset": { "type": "integer" },
            "limit": { "type": "integer" }
        },
        "required": ["path"]
    })
}

#[test]
fn an_undeclared_key_is_refused_naming_it_and_listing_the_parameters_sorted() {
    let args = json!({ "path": "/a.md", "lines": 3 });
    let Err(error) = refuse_unknown("Read", &read(), &args) else {
        panic!("{args} is refused");
    };
    assert_eq!(
        error.to_string(),
        "std_Read: `lines` is not a parameter of this tool. \
         The parameters are `limit`, `offset` and `path`."
    );
    assert_eq!(error.code, ErrorCode::ToolInvalidArguments);
}

#[test]
fn the_parameter_list_is_sorted_bytewise_whatever_order_the_schema_holds() {
    let schema = json!({
        "type": "object",
        "properties": { "zeta": {}, "alpha": {}, "Mid": {} }
    });
    let Err(error) = refuse_unknown("Grep", &schema, &json!({ "x": 1 })) else {
        panic!("x is refused");
    };
    assert_eq!(
        error.to_string(),
        "std_Grep: `x` is not a parameter of this tool. \
         The parameters are `Mid`, `alpha` and `zeta`."
    );
}

#[test]
fn two_parameters_are_joined_with_and_and_one_parameter_has_no_and() {
    let two = json!({ "type": "object", "properties": { "b": {}, "a": {} } });
    let Err(error) = refuse_unknown("Read", &two, &json!({ "c": 1 })) else {
        panic!("c is refused");
    };
    assert!(
        error
            .to_string()
            .ends_with("The parameters are `a` and `b`."),
        "{error}"
    );
    let one = json!({ "type": "object", "properties": { "a": {} } });
    let Err(error) = refuse_unknown("Read", &one, &json!({ "c": 1 })) else {
        panic!("c is refused");
    };
    assert!(
        error.to_string().ends_with("The parameters are `a`."),
        "{error}"
    );
}

#[test]
fn declared_keys_a_null_argument_list_and_a_null_value_pass_the_unknown_key_check() {
    assert!(refuse_unknown("Read", &read(), &json!({ "path": "/a.md", "limit": 2 })).is_ok());
    assert!(refuse_unknown("Read", &read(), &json!(null)).is_ok());
    assert!(refuse_unknown("Read", &read(), &json!({ "path": "/a.md", "limit": null })).is_ok());
}

#[test]
fn a_null_value_under_an_undeclared_key_is_still_refused() {
    let args = json!({ "path": "/a.md", "lines": null });
    assert!(refuse_unknown("Read", &read(), &args).is_err(), "{args}");
}

#[test]
fn the_smallest_undeclared_key_is_the_one_named() {
    let args = json!({ "path": "/a.md", "zz": 1, "aa": 2, "mm": 3 });
    let Err(error) = refuse_unknown("Read", &read(), &args) else {
        panic!("{args} is refused");
    };
    assert!(
        error.to_string().contains("`aa` is not a parameter"),
        "{error}"
    );
}

#[test]
fn a_schema_without_properties_refuses_no_key() {
    let schema = json!({ "type": "object" });
    assert!(refuse_unknown("Read", &schema, &json!({ "x": 1 })).is_ok());
}

#[test]
fn a_schema_with_an_empty_property_list_says_the_tool_takes_none() {
    let schema = json!({ "type": "object", "properties": {} });
    let Err(error) = refuse_unknown("Read", &schema, &json!({ "x": 1 })) else {
        panic!("x is refused");
    };
    assert_eq!(
        error.to_string(),
        "std_Read: `x` is not a parameter of this tool. This tool takes no parameters."
    );
}

#[test]
fn a_blank_value_fails_naming_the_parameter_and_echoing_the_value() {
    let Err(error) = refuse_blank("Read", "path", "   ") else {
        panic!("whitespace is refused");
    };
    assert_eq!(
        error.to_string(),
        "std_Read: `path` is empty or only whitespace, and the call passed \"   \""
    );
    assert_eq!(error.code, ErrorCode::ToolInvalidArguments);
}

#[test]
fn an_empty_or_whitespace_only_value_is_blank_and_any_text_is_not() {
    for blank in ["", " ", "\t\n"] {
        assert!(refuse_blank("Read", "path", blank).is_err(), "{blank:?}");
    }
    for text in ["a", " a ", "/notes.md"] {
        assert!(refuse_blank("Read", "path", text).is_ok(), "{text:?}");
    }
}

#[test]
fn plural_adds_s_for_every_count_but_one() {
    assert_eq!(plural(1, "line"), "1 line");
    assert_eq!(plural(10, "line"), "10 lines");
    assert_eq!(plural(0, "line"), "0 lines");
    assert_eq!(
        plural(2, "invalid UTF-8 sequence"),
        "2 invalid UTF-8 sequences"
    );
}

#[test]
fn plural_turns_a_consonant_then_y_into_ies_and_keeps_a_vowel_then_y() {
    assert_eq!(plural(1, "directory"), "1 directory");
    assert_eq!(plural(2, "directory"), "2 directories");
    assert_eq!(plural(0, "entry"), "0 entries");
    assert_eq!(plural(3, "key"), "3 keys");
    assert_eq!(plural(5, "day"), "5 days");
}

#[test]
fn a_bad_pattern_names_the_parameter_echoes_the_pattern_and_gives_the_reason() {
    let error = invalid_pattern("ListDir", "ignore_globs", "a***", PatternError::StarRun);
    assert_eq!(
        error.to_string(),
        "std_ListDir: `ignore_globs` is invalid: \"a***\" does not parse: \
         more than two consecutive '*' are not supported."
    );
    assert_eq!(error.code, ErrorCode::ToolInvalidArguments);
}

#[test]
fn a_bad_pattern_gives_each_reason_text_as_the_matcher_words_it() {
    for error in [
        PatternError::Empty,
        PatternError::TooLong,
        PatternError::Control,
        PatternError::Backslash,
        PatternError::StarRun,
        PatternError::PartialDoubleStar,
    ] {
        let text = invalid_pattern("Grep", "glob", "x", error).to_string();
        assert_eq!(
            text,
            format!(
                "std_Grep: `glob` is invalid: \"x\" does not parse: {}.",
                error.reason()
            )
        );
    }
}

#[test]
fn a_pattern_over_80_bytes_is_echoed_cut_with_dots_and_80_bytes_exactly_is_not() {
    let eighty = "a".repeat(80);
    let kept = invalid_pattern("ListDir", "ignore_globs", &eighty, PatternError::Empty);
    assert!(
        kept.to_string().contains(&format!("\"{eighty}\" does")),
        "{kept}"
    );
    let long = "a".repeat(81);
    let cut = invalid_pattern("ListDir", "ignore_globs", &long, PatternError::Empty);
    assert!(
        cut.to_string().contains(&format!("\"{eighty}...\" does")),
        "{cut}"
    );
}

#[test]
fn a_pattern_is_cut_on_a_character_boundary() {
    // 79 ASCII bytes and then a 4-byte emoji: byte 80 is inside the emoji,
    // so the echo stops at byte 79 rather than splitting it.
    let pattern = format!("{}\u{1f4e6}tail", "a".repeat(79));
    let text =
        invalid_pattern("ListDir", "ignore_globs", &pattern, PatternError::Empty).to_string();
    assert!(
        text.contains(&format!("\"{}...\" does", "a".repeat(79))),
        "{text}"
    );
    assert!(!text.contains('\u{1f4e6}'), "{text}");
}
