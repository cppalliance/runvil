//! Tests for the JSON a section's Lua reads as `sys`: its keys, their
//! value types, and which optional keys are left out.

use std::sync::Arc;

use runvil_types::ids::{ChainId, TaskId};
use serde_json::json;

use super::SectionSys;

fn sys() -> SectionSys {
    SectionSys {
        when: Arc::from("2026-10-26T18:00:00+00:00"),
        id: "0.2.1".to_owned(),
        taskid: TaskId::from(ChainId::root().child(2)),
        section_name: "Draft".to_owned(),
        execution: Arc::from("run-7"),
        section_count: 3,
        index: None,
        model: None,
    }
}

#[test]
fn sys_json_holds_the_six_shared_fields_with_a_string_task_id() {
    assert_eq!(
        sys().to_json(),
        json!({
            "when": "2026-10-26T18:00:00+00:00",
            "id": "0.2.1",
            "taskid": "0.2",
            "section_name": "Draft",
            "execution": "run-7",
            "section_count": 3,
        })
    );
}

#[test]
fn sys_json_leaves_out_an_unset_index_and_model_instead_of_writing_null() {
    let json = sys().to_json();
    assert!(
        json.get("index").is_none(),
        "an unset index is absent: {json}"
    );
    assert!(
        json.get("model").is_none(),
        "an unset model is absent: {json}"
    );
}

#[test]
fn sys_json_adds_a_set_index_as_a_number_and_a_set_model_as_a_string() {
    let sys = SectionSys {
        index: Some(4),
        model: Some("qwen3".to_owned()),
        ..sys()
    };
    assert_eq!(
        sys.to_json(),
        json!({
            "when": "2026-10-26T18:00:00+00:00",
            "id": "0.2.1",
            "taskid": "0.2",
            "section_name": "Draft",
            "execution": "run-7",
            "section_count": 3,
            "index": 4,
            "model": "qwen3",
        })
    );
}
