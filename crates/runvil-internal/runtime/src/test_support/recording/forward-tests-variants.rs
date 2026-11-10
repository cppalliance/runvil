//! The every-variant coverage test: one value of each `Event` variant,
//! forwarded alone, reaches exactly the seam the table names.

use runvil_types::ids::{AbandonReason, RoundId};

use super::*;

/// One of the three sinks an event can land in, as an index into
/// [`SeamRecorder::counts`].
#[derive(Clone, Copy, Debug)]
enum Seam {
    Observed,
    Content,
    Captured,
}

impl SeamRecorder {
    /// How many records each seam holds, indexed by [`Seam`].
    fn counts(&self) -> [usize; 3] {
        [
            self.observed.lock().expect("not poisoned").len(),
            self.content.lock().expect("not poisoned").len(),
            self.captured.lock().expect("not poisoned").len(),
        ]
    }
}

/// Builds one payload-free lifecycle event per named variant, each bound
/// to the observation seam.
macro_rules! unit_events {
    ($($variant:ident),* $(,)?) => {
        vec![$((
            Event::$variant {
                section: "A".to_owned(),
                provenance: provenance(),
            },
            Seam::Observed,
        ),)*]
    };
}

/// One value of every [`Event`] variant, beside the seam that variant
/// must reach.
///
/// Hand-maintained, and deliberately written out rather than derived from
/// the forwarder's own or-patterns: `Event` is `#[non_exhaustive]` outside
/// `runvil-types`, so no match here can be exhaustive and no
/// compiler check can hold this list to the enum. A variant added to
/// `Event` must be added here by hand.
#[expect(
    clippy::too_many_lines,
    reason = "one flat table of every event variant; splitting it would hide the coverage it exists to show"
)]
fn one_of_every_event_variant() -> Vec<(Event, Seam)> {
    let mut events = unit_events![
        RunStarted,
        RunSucceeded,
        RunFailed,
        SectionStarted,
        SectionFinished,
        ModelTurnCompleted,
        ModelTurnFailed,
        ModelTurnTruncated,
        ToolCallSucceeded,
        ToolCallFailed,
        LuaChunkStarted,
        LuaChunkSucceeded,
        LuaChunkFailed,
        LuaTeardownStarted,
        LuaTeardownSucceeded,
        ToolScopeValidationStarted,
        ToolScopeValidationSucceeded,
        ToolScopeValidationFailed,
        VfsWriteSucceeded,
        VfsWriteFailed,
        VfsAppendSucceeded,
        VfsAppendFailed,
        VfsReadSucceeded,
        VfsReadFailed,
        VfsReadNumberedSucceeded,
        VfsReadNumberedFailed,
        VfsReplaceSucceeded,
        VfsReplaceFailed,
        VfsDeleteSucceeded,
        VfsDeleteFailed,
        VfsGlobSucceeded,
        VfsGlobFailed,
        VfsExistsSucceeded,
        VfsExistsFailed,
    ];
    let task: TaskId = "0.1".parse().expect("a task id parses");
    events.extend([
        (
            Event::ModelMetadataDegraded {
                section: "A".to_owned(),
                provenance: provenance(),
                turn: 1,
                message: "malformed `usage` in completion response ignored".to_owned(),
            },
            Seam::Observed,
        ),
        (
            Event::LuaLibraryLoadStarted {
                section: "A".to_owned(),
                provenance: provenance(),
                library: "## lib".to_owned(),
            },
            Seam::Observed,
        ),
        (
            Event::LuaLibraryLoadSucceeded {
                section: "A".to_owned(),
                provenance: provenance(),
                library: "## lib".to_owned(),
            },
            Seam::Observed,
        ),
        (
            Event::LuaLibraryLoadFailed {
                section: "A".to_owned(),
                provenance: provenance(),
                library: "## lib".to_owned(),
            },
            Seam::Observed,
        ),
        (
            Event::Lua {
                section: "A".to_owned(),
                provenance: provenance(),
                message: "note".to_owned(),
            },
            Seam::Observed,
        ),
        (
            Event::TaskStarted {
                section: "A".to_owned(),
                provenance: provenance(),
                task: task.clone(),
                target: "Child".to_owned(),
                input: None,
                item: None,
                index: None,
                var: serde_json::json!({}),
            },
            Seam::Observed,
        ),
        (
            Event::TaskSucceeded {
                section: "A".to_owned(),
                provenance: provenance(),
                task: task.clone(),
            },
            Seam::Observed,
        ),
        (
            Event::TaskFailed {
                section: "A".to_owned(),
                provenance: provenance(),
                task: task.clone(),
            },
            Seam::Observed,
        ),
        (
            Event::TaskCanceled {
                section: "A".to_owned(),
                provenance: provenance(),
                task: task.clone(),
            },
            Seam::Observed,
        ),
        (
            Event::TaskAbandoned {
                section: "A".to_owned(),
                provenance: provenance(),
                task: task.clone(),
                reason: AbandonReason::OwnerReturned,
            },
            Seam::Observed,
        ),
        (
            Event::InboxMessage {
                section: "A".to_owned(),
                provenance: provenance(),
                text: "also cover the tests".to_owned(),
            },
            Seam::Observed,
        ),
        (
            Event::InboxWaiting {
                section: "A".to_owned(),
                provenance: provenance(),
            },
            Seam::Observed,
        ),
        (
            Event::Thinking {
                section: "A".to_owned(),
                provenance: provenance(),
                turn: 1,
                round: RoundId::new(0),
                model: "m".to_owned(),
                text: "hmm".to_owned(),
            },
            Seam::Content,
        ),
        (
            Event::AssistantReply {
                section: "A".to_owned(),
                provenance: provenance(),
                turn: 1,
                round: RoundId::new(0),
                text: "hi".to_owned(),
                finish_reason: Some("stop".to_owned()),
                model: "m".to_owned(),
                metrics: None,
                origin: ReplyOrigin::Chat,
            },
            Seam::Content,
        ),
        (
            Event::AssistantReply {
                section: "A".to_owned(),
                provenance: provenance(),
                turn: 1,
                round: RoundId::new(1),
                text: "inferred".to_owned(),
                finish_reason: Some("stop".to_owned()),
                model: "m".to_owned(),
                metrics: None,
                origin: ReplyOrigin::Infer,
            },
            Seam::Content,
        ),
        (
            Event::AssistantToolCalls {
                section: "A".to_owned(),
                provenance: provenance(),
                turn: 1,
                round: RoundId::new(0),
                model: "m".to_owned(),
                calls: vec![ToolCallEvent::new(
                    "call_1",
                    "search",
                    serde_json::json!({}),
                )],
            },
            Seam::Content,
        ),
        (
            Event::ToolResult {
                section: "A".to_owned(),
                provenance: provenance(),
                turn: 1,
                tool_call_id: "call_1".to_owned(),
                alias: "search".to_owned(),
                tool: ToolId::parse("web/search").expect("a valid tool id"),
                vendor: None,
                content: "found".to_owned(),
                trusted: false,
            },
            Seam::Content,
        ),
        (
            Event::Request {
                section: "A".to_owned(),
                provenance: provenance(),
                turn: 1,
                body: serde_json::json!({}),
            },
            Seam::Captured,
        ),
        (
            Event::Response {
                section: "A".to_owned(),
                provenance: provenance(),
                turn: 1,
                body: serde_json::json!({}),
                finish_reason: None,
                reasoning_content: None,
            },
            Seam::Captured,
        ),
    ]);
    events
}

#[test]
fn every_event_variant_reaches_exactly_one_seam() {
    for (event, seam) in one_of_every_event_variant() {
        let named = format!("{event:?}");
        let recorder = SeamRecorder::default();
        // A variant no group claims hits `forward_one`'s catch-all and
        // panics here; a variant a group claims but does not destructure
        // falls through that group's own `_ => {}` and records nothing.
        forward_one(event, &recorder, Some(&recorder));
        let counts = recorder.counts();
        assert_eq!(
            counts[seam as usize], 1,
            "{named} must reach the {seam:?} seam exactly once, saw {counts:?}"
        );
        assert_eq!(
            counts.iter().sum::<usize>(),
            1,
            "{named} must reach no seam but {seam:?}, saw {counts:?}"
        );
    }
}
