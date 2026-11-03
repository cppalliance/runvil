//! The store answer's record: each outcome's fixed run-log JSON, and a
//! failure's tag and display text.

use super::*;

#[test]
fn the_store_answer_records_byte_identical_json_for_every_outcome() {
    // `AnswerRecord::Vfs` holds the `VfsOutcome` itself, and its JSON
    // is a fixed run-log format: the unit outcome as a bare string, the
    // payload outcomes in serde's externally tagged form.
    let unit = EffectAnswer::Vfs(Ok(VfsOutcome::Unit)).to_record();
    assert_eq!(
        serde_json::to_string(&unit).expect("a store answer records"),
        r#"{"Vfs":{"Ok":"Unit"}}"#
    );
    assert_eq!(answer_round_trip(&unit), unit);

    let text = EffectAnswer::Vfs(Ok(VfsOutcome::Text("the read".to_owned()))).to_record();
    assert_eq!(
        serde_json::to_string(&text).expect("a store answer records"),
        r#"{"Vfs":{"Ok":{"Text":"the read"}}}"#
    );
    assert_eq!(answer_round_trip(&text), text);

    let paths = EffectAnswer::Vfs(Ok(VfsOutcome::Paths(vec![
        "a.txt".to_owned(),
        "b.txt".to_owned(),
    ])))
    .to_record();
    assert_eq!(
        serde_json::to_string(&paths).expect("a store answer records"),
        r#"{"Vfs":{"Ok":{"Paths":["a.txt","b.txt"]}}}"#
    );
    assert_eq!(answer_round_trip(&paths), paths);

    let flag = EffectAnswer::Vfs(Ok(VfsOutcome::Bool(true))).to_record();
    assert_eq!(
        serde_json::to_string(&flag).expect("a store answer records"),
        r#"{"Vfs":{"Ok":{"Bool":true}}}"#
    );
    assert_eq!(answer_round_trip(&flag), flag);
}

#[test]
fn a_failed_store_answer_records_the_vfs_errors_tag_and_display_text() {
    let error = VfsError::NotFound {
        path: "missing.txt".to_owned(),
    };
    let expected = error.to_string();
    assert_eq!(expected, "not found: missing.txt");
    let record = EffectAnswer::Vfs(Err(error)).to_record();
    assert_eq!(
        record,
        AnswerRecord::Vfs(Err(FailureRecord::new("not_found", expected)))
    );
    assert_eq!(
        serde_json::to_string(&record).expect("a store answer records"),
        r#"{"Vfs":{"Err":{"kind":"not_found","message":"not found: missing.txt"}}}"#
    );
    assert_eq!(answer_round_trip(&record), record);
}
