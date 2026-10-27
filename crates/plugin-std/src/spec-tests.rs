//! The definitions: one file per tool, each parsed, each carrying the
//! wire name Cursor uses for the same tool.

use super::all;

/// The wire name each id segment must carry, which is Cursor's name for
/// the same tool.
const WIRE_NAMES: [(&str, &str); 6] = [
    ("read", "Read"),
    ("write", "Write"),
    ("replace", "StrReplace"),
    ("rm", "Delete"),
    ("glob", "Glob"),
    ("grep", "Grep"),
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
