//! What Grep refuses about its arguments, and in which order.

use runvil_plugin::ToolErrorKind;
use serde_json::{Value, json};

use super::{definition, failure_of, grep, new_call, seeded};
use crate::grep::args::{self, Mode};

/// The thirteen parameters, sorted bytewise, as an undeclared-key refusal
/// lists them.
const PARAMETERS: &str = "`-A`, `-B`, `-C`, `-i`, `glob`, `head_limit`, `literal`, `multiline`, \
                          `offset`, `output_mode`, `path`, `pattern` and `type`";

/// The refusal text for a call with `args`.
fn refusal(args: &Value) -> String {
    failure_of(grep(&new_call(), args))
}

/// The request `args` parse to.
fn parsed(args: &Value) -> args::Request<'_> {
    args::parse(definition(), args).expect("the arguments parse")
}

#[test]
fn a_missing_pattern_names_the_parameter() {
    let error = grep(&new_call(), &json!({})).expect_err("pattern is required");
    assert_eq!(
        error.to_string(),
        "std_Grep: `pattern` is required, and the call left it out"
    );
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
}

#[test]
fn an_undeclared_key_is_refused_naming_it_and_listing_the_sorted_parameters() {
    let error = grep(&new_call(), &json!({ "pattern": "x", "context": 3 }))
        .expect_err("context is not a parameter");
    assert_eq!(
        error.to_string(),
        format!(
            "std_Grep: `context` is not a parameter of this tool. The parameters are {PARAMETERS}."
        )
    );
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
}

#[test]
fn the_one_undeclared_key_that_is_accepted_is_n_when_it_is_true() {
    let call = seeded(&[("/a.txt", "needle\n")]);
    for accepted in [json!(true), Value::Null] {
        let output = grep(&call, &json!({ "pattern": "needle", "-n": accepted }))
            .expect("-n true or null is accepted");
        assert_eq!(output.text(), "/a.txt\n  1:needle");
    }
    let off = "std_Grep: `-n` must be true or left out, because line numbers are always shown.";
    for refused in [json!(false), json!("yes"), json!(1)] {
        assert_eq!(refusal(&json!({ "pattern": "x", "-n": refused })), off);
    }
}

#[test]
fn an_empty_pattern_is_refused_with_the_advice_to_pass_a_dot() {
    assert_eq!(
        refusal(&json!({ "pattern": "" })),
        "std_Grep: `pattern` is empty. Pass \".\" to match every line."
    );
}

#[test]
fn a_negative_context_is_refused_naming_the_key_and_the_value_in_a_fixed_order() {
    assert_eq!(
        refusal(&json!({ "pattern": "x", "-A": -1 })),
        "std_Grep: `-A` is -1. Pass 0 or more context lines."
    );
    assert_eq!(
        refusal(&json!({ "pattern": "x", "-C": -2, "-B": -3, "-A": -4 })),
        "std_Grep: `-A` is -4. Pass 0 or more context lines.",
        "-A comes first"
    );
    assert_eq!(
        refusal(&json!({ "pattern": "x", "-C": -2, "-B": -3 })),
        "std_Grep: `-B` is -3. Pass 0 or more context lines.",
        "-B comes before -C"
    );
    assert_eq!(
        refusal(&json!({ "pattern": "x", "-C": -1.5 })),
        "std_Grep: `-C` is -1.5. Pass 0 or more context lines."
    );
}

#[test]
fn a_negative_head_limit_or_offset_is_refused_head_limit_first() {
    assert_eq!(
        refusal(&json!({ "pattern": "x", "head_limit": -5 })),
        "std_Grep: `head_limit` is -5. Pass 0 or more."
    );
    assert_eq!(
        refusal(&json!({ "pattern": "x", "offset": -1, "head_limit": -5 })),
        "std_Grep: `head_limit` is -5. Pass 0 or more."
    );
    assert_eq!(
        refusal(&json!({ "pattern": "x", "offset": -1 })),
        "std_Grep: `offset` is -1. Pass 0 or more."
    );
}

#[test]
fn an_unknown_type_is_refused_with_the_types_to_try() {
    assert_eq!(
        refusal(&json!({ "pattern": "x", "type": "foo" })),
        "std_Grep: Unrecognized file type \"foo\". Use a type such as js, ts, py, rust, go or \
         java, or filter with glob."
    );
}

#[test]
fn an_invalid_glob_is_refused_with_the_rule_it_broke() {
    assert_eq!(
        refusal(&json!({ "pattern": "x", "glob": "a***" })),
        "std_Grep: `glob` is invalid: \"a***\" does not parse: more than two consecutive '*' \
         are not supported."
    );
}

#[test]
fn a_whitespace_only_path_is_refused_echoing_it() {
    assert_eq!(
        refusal(&json!({ "pattern": "x", "path": "   " })),
        "std_Grep: `path` is empty or only whitespace, and the call passed \"   \""
    );
}

#[test]
fn a_call_that_breaks_several_rules_always_hears_about_the_first_in_order() {
    let mut args = json!({
        "pattern": "",
        "context": 1,
        "-n": false,
        "-A": -1,
        "head_limit": -1,
        "type": "foo",
        "glob": "a***",
        "path": " ",
    });
    let order = [
        ("context", "`context` is not a parameter"),
        ("-n", "`-n` must be true"),
        ("pattern", "`pattern` is empty"),
        ("-A", "`-A` is -1"),
        ("head_limit", "`head_limit` is -1"),
        ("type", "Unrecognized file type"),
        ("glob", "`glob` is invalid"),
        ("path", "`path` is empty or only whitespace"),
    ];
    for (key, expected) in order {
        let message = refusal(&args);
        assert!(
            message.contains(expected),
            "expected {expected:?} before {key} is fixed: {message}"
        );
        match key {
            "pattern" => args["pattern"] = json!("("),
            other => {
                args.as_object_mut().expect("an object").remove(other);
            }
        }
    }
    assert!(
        refusal(&args).contains("Invalid regular expression \"(\""),
        "the pattern is compiled after every other check"
    );
}

#[test]
fn an_empty_path_glob_or_type_reads_as_absent() {
    let call = seeded(&[("/src/a.rs", "needle\n"), ("/b.txt", "needle\n")]);
    let output = grep(
        &call,
        &json!({
            "pattern": "needle",
            "output_mode": "files_with_matches",
            "path": "",
            "glob": "",
            "type": "",
        }),
    )
    .expect("empty strings read as absent");
    assert_eq!(output.text(), "/b.txt\n/src/a.rs");
}

#[test]
fn head_limit_and_context_round_up_and_offset_rounds_down() {
    let args = json!({ "pattern": "x", "head_limit": 2.2, "offset": 2.9, "-A": 0.2 });
    let request = parsed(&args);
    assert_eq!(request.limit, Some(3));
    assert_eq!(request.offset, 2);
    assert_eq!(request.after, 1);
}

#[test]
fn head_limit_zero_and_an_absent_head_limit_both_mean_no_limit() {
    for args in [
        json!({ "pattern": "x", "head_limit": 0 }),
        json!({ "pattern": "x" }),
        json!({ "pattern": "x", "head_limit": null }),
    ] {
        assert_eq!(parsed(&args).limit, None, "{args}");
    }
}

#[test]
fn context_c_sets_both_sides_and_a_and_b_override_it_even_at_zero() {
    let sides = |args: Value| {
        let request = parsed(&args);
        (request.before, request.after)
    };
    assert_eq!(sides(json!({ "pattern": "x", "-C": 2 })), (2, 2));
    assert_eq!(sides(json!({ "pattern": "x", "-C": 2, "-A": 0 })), (2, 0));
    assert_eq!(sides(json!({ "pattern": "x", "-C": 2, "-B": 5 })), (5, 2));
    assert_eq!(
        sides(json!({ "pattern": "x", "-C": 2, "-B": 0, "-A": 1 })),
        (0, 1)
    );
    assert_eq!(sides(json!({ "pattern": "x", "-A": 3 })), (0, 3));
}

#[test]
fn context_is_ignored_outside_content_mode() {
    for mode in ["files_with_matches", "count"] {
        let args = json!({ "pattern": "x", "output_mode": mode, "-C": 4 });
        let request = parsed(&args);
        assert_eq!((request.before, request.after), (0, 0), "{mode}");
    }
    let args = json!({ "pattern": "x", "-C": 4 });
    let request = parsed(&args);
    assert_eq!(request.mode, Mode::Content, "content is the default mode");
    assert_eq!((request.before, request.after), (4, 4));
}

#[test]
fn a_value_past_the_range_of_usize_saturates_instead_of_failing() {
    let args = json!({ "pattern": "x", "offset": 1e30, "-C": 1e30 });
    let request = parsed(&args);
    assert_eq!(request.offset, usize::MAX);
    assert_eq!(request.before, usize::MAX);
}
