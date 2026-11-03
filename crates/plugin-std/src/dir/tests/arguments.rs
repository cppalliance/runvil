//! What ListDir refuses about its arguments, and in which order.

use runvil_plugin::ErrorCode;
use serde_json::json;

use super::{failure_of, list, new_call};

#[test]
fn a_missing_target_names_the_parameter() {
    let call = new_call();
    let error = list(&call, &json!({})).expect_err("target_directory is required");
    assert_eq!(
        error.to_string(),
        "std_ListDir: `target_directory` is required, and the call left it out"
    );
    assert_eq!(error.code, ErrorCode::ToolInvalidArguments);
}

#[test]
fn a_target_that_is_not_a_string_names_the_parameter() {
    let call = new_call();
    assert_eq!(
        failure_of(list(&call, &json!({ "target_directory": 7 }))),
        "std_ListDir: `target_directory` must be a JSON string, and the call passed 7"
    );
}

#[test]
fn ignore_globs_must_be_an_array_of_strings() {
    let call = new_call();
    assert_eq!(
        failure_of(list(
            &call,
            &json!({ "target_directory": "/", "ignore_globs": "*.log" })
        )),
        "std_ListDir: `ignore_globs` must be a JSON array, and the call passed \"*.log\""
    );
    assert_eq!(
        failure_of(list(
            &call,
            &json!({ "target_directory": "/", "ignore_globs": ["a", 3] })
        )),
        "std_ListDir: `ignore_globs` must be a JSON string, and the call passed 3"
    );
}

#[test]
fn an_undeclared_key_is_refused_and_the_parameters_are_named() {
    let call = new_call();
    let error = list(&call, &json!({ "target_directory": "/", "path": "/x" }))
        .expect_err("path is not a parameter");
    assert_eq!(
        error.to_string(),
        "std_ListDir: `path` is not a parameter of this tool. \
         The parameters are `ignore_globs` and `target_directory`."
    );
    assert_eq!(error.code, ErrorCode::ToolInvalidArguments);
}

#[test]
fn an_undeclared_key_is_refused_before_a_blank_target() {
    let call = new_call();
    assert_eq!(
        failure_of(list(&call, &json!({ "target_directory": " ", "depth": 2 }))),
        "std_ListDir: `depth` is not a parameter of this tool. \
         The parameters are `ignore_globs` and `target_directory`."
    );
}

#[test]
fn a_blank_target_is_refused_before_a_bad_pattern() {
    let call = new_call();
    assert_eq!(
        failure_of(list(
            &call,
            &json!({ "target_directory": " ", "ignore_globs": ["a***"] })
        )),
        "std_ListDir: `target_directory` is empty or only whitespace, and the call passed \" \""
    );
}
