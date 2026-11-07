//! Tests for `Event` serde round trips and coordinate exposure.

use serde_json::json;

use super::Event;
use super::ReplyOrigin;
use crate::event_lifecycle::{self as lifecycle, Lifecycle};
use crate::ids::{AbandonReason, Provenance, RoundId, TaskId};
use crate::metrics::{CallMetrics, ToolCallEvent, Usage};
use crate::test_support::{assert_send_sync, assert_wire, round_trip};
use crate::tools::ToolId;

#[path = "event-tests-library.rs"]
mod library;

fn task(path: &str) -> TaskId {
    path.parse().expect("a task id parses")
}

/// The bound tool the content samples' `read_file` alias resolves to.
fn read_tool() -> ToolId {
    ToolId::parse("files/read").expect("a valid tool id")
}

fn provenance(path: &str, seq: u32) -> Provenance {
    Provenance::new(task(path), seq)
}

fn sample_metrics() -> CallMetrics {
    CallMetrics::new().with_usage(Some(Usage::new(7, 3, 10)))
}

fn round_trips(event: &Event) {
    let line = serde_json::to_string(event).expect("every event serializes");
    assert!(
        !line.contains('\n'),
        "one event must serialize to one line: {line}"
    );
    assert_eq!(&round_trip(event), event, "{line}");
}

/// The lifecycle constant that builds `event`, or `None` for a variant
/// with a payload. There is no wildcard arm, so a new variant fails to
/// compile here until it is mapped; a new payload variant also needs a
/// sample in `payload_samples`.
fn lifecycle_of(event: &Event) -> Option<Lifecycle> {
    match event {
        Event::ParseStarted { .. } => Some(lifecycle::PARSE_STARTED),
        Event::ParseSucceeded { .. } => Some(lifecycle::PARSE_SUCCEEDED),
        Event::ParseFailed { .. } => Some(lifecycle::PARSE_FAILED),
        Event::RunStarted { .. } => Some(lifecycle::RUN_STARTED),
        Event::RunSucceeded { .. } => Some(lifecycle::RUN_SUCCEEDED),
        Event::RunFailed { .. } => Some(lifecycle::RUN_FAILED),
        Event::SectionStarted { .. } => Some(lifecycle::SECTION_STARTED),
        Event::SectionFinished { .. } => Some(lifecycle::SECTION_FINISHED),
        Event::ModelTurnCompleted { .. } => Some(lifecycle::MODEL_TURN_COMPLETED),
        Event::ModelTurnFailed { .. } => Some(lifecycle::MODEL_TURN_FAILED),
        Event::ModelTurnTruncated { .. } => Some(lifecycle::MODEL_TURN_TRUNCATED),
        Event::ToolCallSucceeded { .. } => Some(lifecycle::TOOL_CALL_SUCCEEDED),
        Event::ToolCallFailed { .. } => Some(lifecycle::TOOL_CALL_FAILED),
        Event::LuaCompilationStarted { .. } => Some(lifecycle::LUA_COMPILATION_STARTED),
        Event::LuaCompilationSucceeded { .. } => Some(lifecycle::LUA_COMPILATION_SUCCEEDED),
        Event::LuaCompilationFailed { .. } => Some(lifecycle::LUA_COMPILATION_FAILED),
        Event::LuaChunkStarted { .. } => Some(lifecycle::LUA_CHUNK_STARTED),
        Event::LuaChunkSucceeded { .. } => Some(lifecycle::LUA_CHUNK_SUCCEEDED),
        Event::LuaChunkFailed { .. } => Some(lifecycle::LUA_CHUNK_FAILED),
        Event::LuaTeardownStarted { .. } => Some(lifecycle::LUA_TEARDOWN_STARTED),
        Event::LuaTeardownSucceeded { .. } => Some(lifecycle::LUA_TEARDOWN_SUCCEEDED),
        Event::ToolScopeValidationStarted { .. } => Some(lifecycle::TOOL_SCOPE_VALIDATION_STARTED),
        Event::ToolScopeValidationSucceeded { .. } => {
            Some(lifecycle::TOOL_SCOPE_VALIDATION_SUCCEEDED)
        }
        Event::ToolScopeValidationFailed { .. } => Some(lifecycle::TOOL_SCOPE_VALIDATION_FAILED),
        Event::VfsWriteSucceeded { .. } => Some(lifecycle::VFS_WRITE_SUCCEEDED),
        Event::VfsWriteFailed { .. } => Some(lifecycle::VFS_WRITE_FAILED),
        Event::VfsAppendSucceeded { .. } => Some(lifecycle::VFS_APPEND_SUCCEEDED),
        Event::VfsAppendFailed { .. } => Some(lifecycle::VFS_APPEND_FAILED),
        Event::VfsReadSucceeded { .. } => Some(lifecycle::VFS_READ_SUCCEEDED),
        Event::VfsReadFailed { .. } => Some(lifecycle::VFS_READ_FAILED),
        Event::VfsReadNumberedSucceeded { .. } => Some(lifecycle::VFS_READ_NUMBERED_SUCCEEDED),
        Event::VfsReadNumberedFailed { .. } => Some(lifecycle::VFS_READ_NUMBERED_FAILED),
        Event::VfsReplaceSucceeded { .. } => Some(lifecycle::VFS_REPLACE_SUCCEEDED),
        Event::VfsReplaceFailed { .. } => Some(lifecycle::VFS_REPLACE_FAILED),
        Event::VfsDeleteSucceeded { .. } => Some(lifecycle::VFS_DELETE_SUCCEEDED),
        Event::VfsDeleteFailed { .. } => Some(lifecycle::VFS_DELETE_FAILED),
        Event::VfsGlobSucceeded { .. } => Some(lifecycle::VFS_GLOB_SUCCEEDED),
        Event::VfsGlobFailed { .. } => Some(lifecycle::VFS_GLOB_FAILED),
        Event::VfsExistsSucceeded { .. } => Some(lifecycle::VFS_EXISTS_SUCCEEDED),
        Event::VfsExistsFailed { .. } => Some(lifecycle::VFS_EXISTS_FAILED),
        Event::ModelMetadataDegraded { .. }
        | Event::LuaLibraryLoadStarted { .. }
        | Event::LuaLibraryLoadSucceeded { .. }
        | Event::LuaLibraryLoadFailed { .. }
        | Event::Lua { .. }
        | Event::TaskStarted { .. }
        | Event::TaskSucceeded { .. }
        | Event::TaskFailed { .. }
        | Event::TaskCanceled { .. }
        | Event::TaskAbandoned { .. }
        | Event::Thinking { .. }
        | Event::AssistantReply { .. }
        | Event::AssistantToolCalls { .. }
        | Event::ToolResult { .. }
        | Event::Request { .. }
        | Event::Response { .. } => None,
    }
}

/// One sample of every variant with a payload; the payload-free variants
/// come from `lifecycle::ALL`.
fn payload_samples() -> Vec<Event> {
    let mut samples = lifecycle_payload_samples();
    samples.extend(library::samples());
    samples.extend(task_samples());
    samples.extend(content_samples());
    samples.extend(debug_samples());
    samples
}

fn lifecycle_payload_samples() -> Vec<Event> {
    vec![
        Event::Lua {
            execution: "run-1".to_owned(),
            section: "Gather".to_owned(),
            provenance: provenance("0", 1),
            message: "checkpoint".to_owned(),
        },
        Event::ModelMetadataDegraded {
            execution: "run-1".to_owned(),
            section: "Gather".to_owned(),
            provenance: provenance("0", 8),
            turn: 2,
            message: "malformed `usage` in completion response ignored: invalid type: string \"lots\", expected u64".to_owned(),
        },
    ]
}

fn task_samples() -> Vec<Event> {
    vec![
        Event::TaskStarted {
            execution: "run-1".to_owned(),
            section: "Gather".to_owned(),
            provenance: provenance("0", 2),
            task: task("0.0"),
            target: "Worker".to_owned(),
            input: Some("arg text".to_owned()),
            item: Some(json!({ "key": "value" })),
            index: Some(3),
            var: json!({ "topic": "leap days" }),
        },
        Event::TaskSucceeded {
            execution: "run-1".to_owned(),
            section: "Worker".to_owned(),
            provenance: provenance("0.0", 1),
            task: task("0.0"),
        },
        Event::TaskFailed {
            execution: "run-1".to_owned(),
            section: "Worker".to_owned(),
            provenance: provenance("0.1", 1),
            task: task("0.1"),
        },
        Event::TaskCanceled {
            execution: "run-1".to_owned(),
            section: "Worker".to_owned(),
            provenance: provenance("0.2", 1),
            task: task("0.2"),
        },
        Event::TaskAbandoned {
            execution: "run-1".to_owned(),
            section: "Worker".to_owned(),
            provenance: provenance("0.0", 4),
            task: task("0.0"),
            reason: AbandonReason::ToolLoopExhausted,
        },
    ]
}

fn content_samples() -> Vec<Event> {
    let reply = |seq, origin| Event::AssistantReply {
        execution: "run-1".to_owned(),
        section: "Gather".to_owned(),
        provenance: provenance("0", seq),
        turn: 2,
        round: RoundId::new(1),
        text: "hello".to_owned(),
        finish_reason: Some("stop".to_owned()),
        model: "llama-3".to_owned(),
        metrics: Some(sample_metrics()),
        origin,
    };
    vec![
        Event::Thinking {
            execution: "run-1".to_owned(),
            section: "Gather".to_owned(),
            provenance: provenance("0", 14),
            turn: 2,
            round: RoundId::new(1),
            model: "llama-3".to_owned(),
            text: "considering".to_owned(),
        },
        reply(3, ReplyOrigin::Chat),
        reply(7, ReplyOrigin::Infer),
        Event::AssistantToolCalls {
            execution: "run-1".to_owned(),
            section: "Gather".to_owned(),
            provenance: provenance("0", 4),
            turn: 2,
            round: RoundId::new(1),
            model: "llama-3".to_owned(),
            calls: vec![
                ToolCallEvent::new("call_1", "read_file", json!({ "path": "notes.txt" }))
                    .with_tool(Some(read_tool()))
                    .with_vendor(Some("acme".to_owned())),
            ],
        },
        Event::ToolResult {
            execution: "run-1".to_owned(),
            section: "Gather".to_owned(),
            provenance: provenance("0", 5),
            turn: 2,
            tool_call_id: "call_1".to_owned(),
            alias: "read_file".to_owned(),
            tool: read_tool(),
            vendor: Some("acme".to_owned()),
            content: "file contents".to_owned(),
            trusted: false,
        },
    ]
}

fn debug_samples() -> Vec<Event> {
    vec![
        Event::Request {
            execution: "run-1".to_owned(),
            section: "Gather".to_owned(),
            provenance: provenance("0", 16),
            turn: 2,
            body: json!({ "messages": [] }),
        },
        Event::Response {
            execution: "run-1".to_owned(),
            section: "Gather".to_owned(),
            provenance: provenance("0", 6),
            turn: 2,
            body: json!({ "choices": [] }),
            finish_reason: Some("length".to_owned()),
            reasoning_content: None,
        },
    ]
}

#[test]
fn every_variant_round_trips_through_serde() {
    for (_, build) in lifecycle::ALL {
        round_trips(&build(
            "run-1".to_owned(),
            "Gather".to_owned(),
            provenance("0", 0),
        ));
    }
    for event in payload_samples() {
        round_trips(&event);
    }
}

#[test]
fn every_payload_free_variant_maps_to_its_lifecycle_constant() {
    let coordinates = || ("run-1".to_owned(), "Gather".to_owned(), provenance("0", 0));
    for (variant, build) in lifecycle::ALL {
        let (execution, section, provenance) = coordinates();
        let event = build(execution, section, provenance);
        let mapped = lifecycle_of(&event)
            .unwrap_or_else(|| panic!("{variant} is mapped to no lifecycle constant"));
        let (execution, section, provenance) = coordinates();
        assert_eq!(
            mapped(execution, section, provenance),
            event,
            "{variant} is mapped to another variant's constant"
        );
    }
    for event in payload_samples() {
        assert!(
            lifecycle_of(&event).is_none(),
            "a payload variant is mapped to a lifecycle constant: {event:?}"
        );
    }
}

#[test]
fn a_reply_origin_defaults_to_chat() {
    assert_eq!(ReplyOrigin::default(), ReplyOrigin::Chat);
}

#[test]
fn a_reply_serializes_its_origin_and_its_round() {
    let event = Event::AssistantReply {
        execution: "run-1".to_owned(),
        section: "Gather".to_owned(),
        provenance: provenance("0", 20),
        turn: 1,
        round: RoundId::new(4),
        text: "inferred".to_owned(),
        finish_reason: None,
        model: "llama-3".to_owned(),
        metrics: None,
        origin: ReplyOrigin::Infer,
    };
    let line = serde_json::to_string(&event).expect("an event serializes");
    assert!(
        line.contains(r#""origin":"infer""#),
        "the origin must reach the wire: {line}"
    );
    assert!(
        line.contains(r#""round":4"#),
        "the round must reach the wire as a bare number: {line}"
    );
}

#[test]
fn a_tool_result_without_a_vendor_keeps_its_logged_shape() {
    // A result with no vendor writes no `vendor` key, and that line reads
    // back with no vendor.
    let line = r#"{"kind":"tool_result","execution":"run-1","section":"Gather","provenance":{"task":"0","seq":5},"turn":2,"tool_call_id":"","alias":"read_file","tool":"files/read","content":"hi","trusted":true}"#;
    let event: Event = serde_json::from_str(line).expect("a tool result without a vendor parses");
    assert!(matches!(
        &event,
        Event::ToolResult { tool, vendor: None, .. } if *tool == read_tool()
    ));
    assert_wire(&event, line);
}

#[test]
fn a_serialized_event_is_tagged_by_kind_with_its_coordinates_beside_the_payload() {
    // The tag and the three coordinates sit beside the payload in every
    // event's serialized form, so a caller can key an event by them
    // without inspecting the payload.
    let event = Event::VfsWriteSucceeded {
        execution: "run-1".to_owned(),
        section: "Gather".to_owned(),
        provenance: provenance("0.2", 9),
    };
    assert_wire(
        &event,
        r#"{"kind":"vfs_write_succeeded","execution":"run-1","section":"Gather","provenance":{"task":"0.2","seq":9}}"#,
    );
    let succeeded = Event::TaskSucceeded {
        execution: "run-1".to_owned(),
        section: "Worker".to_owned(),
        provenance: provenance("0.1", 10),
        task: task("0.1"),
    };
    assert_wire(
        &succeeded,
        r#"{"kind":"task_succeeded","execution":"run-1","section":"Worker","provenance":{"task":"0.1","seq":10},"task":"0.1"}"#,
    );
}

#[test]
fn a_degraded_metadata_report_serializes_its_turn_and_message_under_its_kind() {
    let event = Event::ModelMetadataDegraded {
        execution: "run-1".to_owned(),
        section: "Gather".to_owned(),
        provenance: provenance("0", 3),
        turn: 1,
        message: "completion response named no string `model`; recorded as empty".to_owned(),
    };
    assert_wire(
        &event,
        r#"{"kind":"model_metadata_degraded","execution":"run-1","section":"Gather","provenance":{"task":"0","seq":3},"turn":1,"message":"completion response named no string `model`; recorded as empty"}"#,
    );
}

#[test]
fn every_event_exposes_its_coordinates() {
    let events = [
        Event::SectionFinished {
            execution: "run-1".to_owned(),
            section: "Gather".to_owned(),
            provenance: provenance("0", 11),
        },
        Event::Lua {
            execution: "run-1".to_owned(),
            section: "Gather".to_owned(),
            provenance: provenance("0", 12),
            message: "hello".to_owned(),
        },
        Event::TaskSucceeded {
            execution: "run-1".to_owned(),
            section: "Worker".to_owned(),
            provenance: provenance("0.1", 0),
            task: task("0.1"),
        },
        Event::Request {
            execution: "run-1".to_owned(),
            section: "Gather".to_owned(),
            provenance: provenance("0", 13),
            turn: 4,
            body: json!({ "messages": [] }),
        },
    ];
    for event in &events {
        assert_eq!(event.execution(), "run-1");
    }
    assert_eq!(events[0].section(), "Gather");
    assert_eq!(events[2].section(), "Worker");
    assert_eq!(events[1].provenance(), &provenance("0", 12));
    assert_eq!(events[2].provenance(), &provenance("0.1", 0));
    assert_eq!(events[3].provenance().seq, 13);
}

#[test]
fn an_event_is_send_and_sync() {
    assert_send_sync::<Event>();
}
