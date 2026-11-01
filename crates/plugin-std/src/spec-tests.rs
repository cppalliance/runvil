//! The definitions: one file per tool, each parsed, each carrying the
//! wire name Cursor uses for the same tool.

use super::all;

/// The wire name each id segment must carry, which is Cursor's name for
/// the same tool.
const WIRE_NAMES: [(&str, &str); 7] = [
    ("read", "Read"),
    ("write", "Write"),
    ("replace", "StrReplace"),
    ("rm", "Delete"),
    ("glob", "Glob"),
    ("grep", "Grep"),
    ("dir", "ListDir"),
];

#[test]
fn every_definition_parses_and_keeps_the_wire_name_it_was_snapshotted_with() {
    let definitions = all().expect("every embedded definition parses");
    let read: Vec<(&str, &str)> = definitions
        .iter()
        .map(|definition| (definition.segment, definition.wire_name.as_str()))
        .collect();
    assert_eq!(read, WIRE_NAMES);
}

#[test]
fn every_definition_holds_a_description_and_an_object_schema_with_at_least_one_property() {
    for definition in all().expect("every embedded definition parses") {
        assert!(
            !definition.description.trim().is_empty(),
            "the {} tool has a description",
            definition.segment
        );
        assert_eq!(
            definition.parameters.get("type").and_then(|t| t.as_str()),
            Some("object"),
            "the {} tool's schema describes an object: {}",
            definition.segment,
            definition.parameters
        );
        let properties = definition
            .parameters
            .get("properties")
            .and_then(|p| p.as_object());
        assert!(
            properties.is_some_and(|properties| !properties.is_empty()),
            "the {} tool takes parameters: {}",
            definition.segment,
            definition.parameters
        );
    }
}

#[test]
fn every_required_parameter_is_one_the_schema_declares_and_describes() {
    for definition in all().expect("every embedded definition parses") {
        let Some(required) = definition
            .parameters
            .get("required")
            .and_then(|r| r.as_array())
        else {
            continue;
        };
        let properties = definition
            .parameters
            .get("properties")
            .and_then(|p| p.as_object())
            .expect("a schema with required parameters declares properties");
        for name in required {
            let name = name.as_str().expect("a required entry is a string");
            let property = properties.get(name).unwrap_or_else(|| {
                panic!(
                    "the {} tool requires {name}, which its schema does not declare",
                    definition.segment
                )
            });
            assert!(
                property
                    .get("description")
                    .and_then(|d| d.as_str())
                    .is_some_and(|text| !text.trim().is_empty()),
                "the {} tool's {name} parameter has a description: {property}",
                definition.segment
            );
        }
    }
}

#[test]
fn the_read_description_holds_the_limits_the_code_uses_and_no_unfilled_placeholder() {
    let definitions = all().expect("every embedded definition parses");
    let read = definitions
        .iter()
        .find(|definition| definition.segment == "read")
        .expect("the read definition exists");
    let text = &read.description;
    assert!(text.contains("100000"), "the byte cap is named: {text}");
    assert!(
        text.contains("16777216"),
        "the file ceiling is named: {text}"
    );
    assert!(!text.contains("cat -n"), "the cat -n line is gone: {text}");
    assert!(
        !text.contains("2000 characters"),
        "the 2000-character line is gone: {text}"
    );
    for placeholder in ["{byte_cap}", "{file_ceiling}"] {
        assert!(
            !text.contains(placeholder),
            "{placeholder} is filled in: {text}"
        );
    }
}

#[test]
fn the_dir_description_holds_the_body_cap_and_no_unfilled_placeholder() {
    let definitions = all().expect("every embedded definition parses");
    let dir = definitions
        .iter()
        .find(|definition| definition.segment == "dir")
        .expect("the dir definition exists");
    let text = &dir.description;
    assert!(text.contains("15000"), "the body cap is named: {text}");
    assert!(
        !text.contains("{body_cap}"),
        "{{body_cap}} is filled in: {text}"
    );
}

#[test]
fn the_dir_schema_requires_the_target_and_takes_an_array_of_strings_for_the_ignores() {
    let definitions = all().expect("every embedded definition parses");
    let dir = definitions
        .iter()
        .find(|definition| definition.segment == "dir")
        .expect("the dir definition exists");
    let properties = &dir.parameters["properties"];
    assert_eq!(properties["target_directory"]["type"], "string");
    assert_eq!(properties["ignore_globs"]["type"], "array");
    assert_eq!(properties["ignore_globs"]["items"]["type"], "string");
    assert_eq!(
        dir.parameters["required"],
        serde_json::json!(["target_directory"])
    );
}

#[test]
fn the_dir_description_states_the_behavior_the_tool_has() {
    let definitions = all().expect("every embedded definition parses");
    let dir = definitions
        .iter()
        .find(|definition| definition.segment == "dir")
        .expect("the dir definition exists");
    let text = &dir.description;
    for phrase in [
        "breadth first",
        "`.git`",
        "dotfile",
        "link",
        "ignore_globs",
        "any depth",
        "byte order",
    ] {
        assert!(
            text.contains(phrase),
            "the description names {phrase}: {text}"
        );
    }
}

#[test]
fn the_read_schema_takes_integers_for_offset_and_limit() {
    let definitions = all().expect("every embedded definition parses");
    let read = definitions
        .iter()
        .find(|definition| definition.segment == "read")
        .expect("the read definition exists");
    for name in ["offset", "limit"] {
        assert_eq!(
            read.parameters["properties"][name]["type"], "integer",
            "{name} is an integer"
        );
    }
}

#[test]
fn no_definition_names_behavior_this_build_does_not_have() {
    for definition in all().expect("every embedded definition parses") {
        let text = format!("{} {}", definition.description, definition.parameters);
        for absent in ["PDF", "Shell tool", "Cursor workspace"] {
            assert!(
                !text.contains(absent),
                "the {} tool still mentions {absent}: {text}",
                definition.segment
            );
        }
    }
}
