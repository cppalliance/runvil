//! Tests for the per-record rules a list enforces as `append` or a builder
//! adds a record, each refusal naming the position the record would take.

use serde_json::json;

use super::list::refusal;
use crate::test_vm::TestVm;
use crate::tests::user;

/// Appends every record of the Lua array `records` but the last to a
/// fresh list, then returns the refusal of the last one.
#[track_caller]
fn refused_append(records: &str) -> String {
    let vm = TestVm::new().build();
    vm.exec(&format!(
        "records = {records}\n\
             for i = 1, #records - 1 do messages:append(records[i]) end"
    ));
    refusal(&vm, "messages:append(records[#records])")
}

fn assert_refusals(cases: &[(&str, &str)]) {
    for (records, expected) in cases {
        assert_eq!(refused_append(records), *expected, "{records}");
    }
}

#[test]
fn malformed_tool_calls_are_refused_naming_the_index() {
    assert_refusals(&[
        (
            r#"{ { role = "assistant", content = "", tool_calls = { "raw" } } }"#,
            "messages[1] tool_calls[1] must be a table",
        ),
        (
            r#"{ { role = "assistant", content = "", tool_calls = { { name = "echo" } } } }"#,
            "messages[1] tool_calls[1] must set a string id",
        ),
        (
            r#"{ { role = "assistant", content = "", tool_calls = { { id = "call_1" } } } }"#,
            "messages[1] tool_calls[1] must set a string name",
        ),
        (
            r#"{ { role = "assistant", content = "", tool_calls = { { id = "call_1", name = "echo", arguments = "raw" } } } }"#,
            "messages[1] tool_calls[1] arguments must be a table",
        ),
        (
            r#"{ { role = "assistant", content = "", tool_calls = "raw" } }"#,
            "messages[1] tool_calls must be an array",
        ),
    ]);
}

#[test]
fn content_parts_validate_each_variants_payload() {
    assert_refusals(&[
        (
            r#"{ { role = "user", content = { { type = "text" } } } }"#,
            "messages[1] content part 1 is a text part and must set a string \
             text field",
        ),
        (
            r#"{ { role = "user", content = { { type = "image_url" } } } }"#,
            "messages[1] content part 1 is an image_url part and must set an \
             image_url table with a string url field",
        ),
        (
            r#"{ { role = "user", content = { { type = "image_url", image_url = { detail = "high" } } } } }"#,
            "messages[1] content part 1 is an image_url part and must set an \
             image_url table with a string url field",
        ),
    ]);
}

#[test]
fn a_non_string_tool_call_id_is_refused() {
    assert_refusals(&[(
        r#"{ { role = "user", content = "ok", tool_call_id = 7 } }"#,
        "messages[1] tool_call_id must be a string",
    )]);
}

#[test]
fn record_validation_names_the_offending_index() {
    assert_refusals(&[
        (
            r#"{ "not a table" }"#,
            "messages[1] must be a message table",
        ),
        (
            r#"{ { role = "user", content = "ok" }, { role = "wizard", content = "x" } }"#,
            "messages[2] role \"wizard\" is unknown; known roles: system, user, assistant, tool",
        ),
        (
            r#"{ { content = "no role" } }"#,
            "messages[1] role must be a string, one of: system, user, assistant, tool",
        ),
        (
            r#"{ { role = "user" } }"#,
            "messages[1] content must be a string or a non-empty array of content parts",
        ),
        (
            r#"{ { role = "user", content = { "bare string part" } } }"#,
            "messages[1] content part 1 must be a table with a string type field",
        ),
        (
            r#"{ { role = "user", content = { { type = "text", text = "ok" }, { type = "video" } } } }"#,
            "messages[1] content part 2 has unknown type \"video\"; known types: text, image_url",
        ),
        (
            r#"{ { role = "user", content = "ok" }, { role = "tool", content = "r" } }"#,
            "messages[2] is a tool message and must set a string tool_call_id",
        ),
        (
            r#"{ { role = "user", content = "ok" }, { role = "user", content = "x", tool_call_id = "call_1" } }"#,
            "messages[2] sets a tool_call_id but is not a tool message",
        ),
        (
            r#"{ { role = "user", content = {} } }"#,
            "messages[1] content must be a string or a non-empty array of content parts",
        ),
    ]);
}

#[test]
fn meta_of_8192_compact_json_bytes_is_kept_and_one_byte_more_is_refused() {
    let vm = TestVm::new().build();
    vm.exec("messages:user('fits', { k = string.rep('x', 8184) })");
    let fits = vm.messages().to_records();
    assert_eq!(
        fits[0].meta,
        Some(json!({ "k": "x".repeat(8184) })),
        "{{\"k\":\"\"}} is 8 bytes, so this meta is exactly 8192"
    );
    for source in [
        "messages:user('over', { k = string.rep('x', 8185) })",
        "messages:append({ role = 'user', content = 'over', meta = { k = string.rep('x', 8185) } })",
    ] {
        assert_eq!(
            refusal(&vm, source),
            "messages[2] meta is 8193 bytes as JSON; the limit is 8192 bytes",
            "{source}"
        );
    }
    assert_eq!(
        vm.messages().to_records(),
        fits,
        "a refused record leaves the list unchanged"
    );
}

#[test]
fn a_meta_that_is_not_json_representable_is_refused_with_the_builder_or_append_text() {
    let vm = TestVm::new().build();
    vm.exec("messages:user('kept')");
    for (source, text) in [
        (
            "messages:user('u', { f = function() end })",
            "messages[2] meta must be JSON-representable: ",
        ),
        (
            "messages:tool('r', 'call_1', function() end)",
            "messages[2] meta must be JSON-representable: ",
        ),
        (
            "messages:append({ role = 'user', content = 'u', meta = { f = function() end } })",
            "messages[2] must be a JSON-representable message table: ",
        ),
    ] {
        let refused = refusal(&vm, source);
        assert!(refused.starts_with(text), "{source}: {refused}");
    }
    assert_eq!(
        vm.messages().to_records(),
        vec![user("kept")],
        "a refused record leaves the list unchanged"
    );
}

#[test]
fn a_json_null_meta_means_none() {
    let vm = TestVm::new().build();
    let read_nil: bool = vm.eval(
        "messages:user('a', json.null)\
         :append({ role = 'user', content = 'b', meta = json.null })\
         return messages[1].meta == nil and messages[2].meta == nil",
    );
    assert!(read_nil, "a view reads no meta");
    assert_eq!(vm.messages().to_records(), vec![user("a"), user("b")]);
}
