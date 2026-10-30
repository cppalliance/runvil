//! The progress wire contract, pinned by golden JSON.
//!
//! Workshop only deserializes `Progress`, so this file pins the field names
//! and their JSON types rather than a round trip. A rename on the Gateway's
//! side, or a type change here, fails one of these assertions instead of
//! silently leaving the status bar stuck.

use super::*;

#[test]
fn the_canonical_document_decodes_to_the_two_fields() {
    let snapshot: Progress =
        serde_json::from_str(r#"{"busy":true,"text":"Downloading qwen3-8b.gguf 45%"}"#)
            .expect("the canonical document decodes");
    assert_eq!(
        snapshot,
        Progress {
            busy: true,
            text: "Downloading qwen3-8b.gguf 45%".to_owned(),
        }
    );
}

#[test]
fn an_idle_document_decodes_to_an_empty_text() {
    let snapshot: Progress =
        serde_json::from_str(r#"{"busy":false,"text":""}"#).expect("an idle document decodes");
    assert_eq!(snapshot, Progress::default());
}

#[test]
fn the_field_names_and_json_types_are_the_contract() {
    // Pins the wire names and types through a decode of each, so a rename
    // on either side fails here even if nobody updates a fixture. The
    // future OpenAPI spec describes these same two fields.
    //
    // Both fields are required: the Gateway writes the whole snapshot on
    // every event, so a missing field is a version skew the decoder should
    // report rather than paper over with a default.
    for document in [r#"{"busy":true}"#, r#"{"text":"Loading"}"#] {
        let decoded = serde_json::from_str::<Progress>(document);
        assert!(
            decoded.is_err(),
            "both fields are required, so {document} must not decode: {decoded:?}"
        );
    }

    let wrong_type = serde_json::from_str::<Progress>(r#"{"busy":"yes","text":""}"#);
    assert!(
        wrong_type.is_err(),
        "busy is a boolean, not a string: {wrong_type:?}"
    );
    let wrong_text = serde_json::from_str::<Progress>(r#"{"busy":false,"text":7}"#);
    assert!(
        wrong_text.is_err(),
        "text is a string, not a number: {wrong_text:?}"
    );
}

#[test]
fn an_unknown_field_is_tolerated() {
    // A newer Gateway adding a field must not break an older Workshop.
    let snapshot: Progress =
        serde_json::from_str(r#"{"busy":true,"text":"Loading","fraction":0.45}"#)
            .expect("an unknown field is ignored");
    assert_eq!(snapshot.text, "Loading");
}

#[test]
fn the_old_operation_event_shape_is_not_a_progress_snapshot() {
    // A gateway emitting the retired weighted-tree operation events is a
    // version skew the decoder reports, never renders.
    let stale = serde_json::json!({
        "operation": 7,
        "path": "local-models/ggml/download",
        "label": "Download",
        "state": {"Updated": {"fraction": 0.5}},
    })
    .to_string();
    let decoded = serde_json::from_str::<Progress>(&stale);
    assert!(
        decoded.is_err(),
        "a payload without busy and text is not a snapshot: {decoded:?}"
    );
}
