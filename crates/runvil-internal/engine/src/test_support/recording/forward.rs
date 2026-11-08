//! The adapter from the Engine's [`Event`] values to the suites' recording
//! seams: each event replayed onto the [`Observer`] and, for the debug
//! variants, the [`DebugCapture`].
//!
//! Lifecycle variants become the matching [`Observation`], and an inbox
//! message an `Other` trace line; content variants become the `on_*`
//! hooks; `Request` and `Response` become the capture pair. The order of
//! the batch is the order the observer sees.

use runvil_types::event::Event;
use runvil_types::tools::OutputTrust;

use super::{
    DebugCapture, DebugEvent, Observation, Observer, ReplyReport, ThinkingReport, ToolCallsReport,
    ToolResultReport,
};

/// Declares the payload-free lifecycle pairs once and derives the
/// event-to-observation fold from the one list.
macro_rules! lifecycle_pairs {
    ($($variant:ident),* $(,)?) => {
        /// The payload-free [`Observation`] matching a payload-free
        /// [`Event`], or `None` for any other variant.
        fn unit_observation(event: &Event) -> Option<Observation> {
            Some(match event {
                $(Event::$variant { .. } => Observation::$variant,)*
                _ => return None,
            })
        }
    };
}

lifecycle_pairs! {
    ParseStarted,
    ParseSucceeded,
    ParseFailed,
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
    LuaCompilationStarted,
    LuaCompilationSucceeded,
    LuaCompilationFailed,
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
}

/// The lifecycle, task, and inbox variants with payloads, the ones
/// [`forward_lifecycle`] owns.
macro_rules! task_lifecycle_variants {
    () => {
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
            | Event::InboxMessage { .. }
    };
}

/// The content variants [`forward_content`] owns.
macro_rules! content_variants {
    () => {
        Event::Thinking { .. }
            | Event::AssistantReply { .. }
            | Event::AssistantToolCalls { .. }
            | Event::ToolResult { .. }
    };
}

/// The debug pair [`forward_debug`] owns.
macro_rules! debug_variants {
    () => {
        Event::Request { .. } | Event::Response { .. }
    };
}

/// Replays `events`, in order, onto `observer`, dropping the debug pair.
pub(crate) fn forward(events: Vec<Event>, observer: &dyn Observer) {
    forward_impl(events, observer, None);
}

/// Replays `events`, in order, onto `observer` and, for the debug variants,
/// `debug`.
fn forward_impl(events: Vec<Event>, observer: &dyn Observer, debug: Option<&dyn DebugCapture>) {
    for event in events {
        forward_one(event, observer, debug);
    }
}

/// Routes one event to the seam its group belongs to. [`Event`] is
/// `#[non_exhaustive]` in `runvil-types`, so the match cannot be
/// total here and no compiler check holds the groups to the enum. What
/// holds instead is
/// `tests::every_event_variant_reaches_exactly_one_seam`, which drives one
/// value of every variant through this function and asserts each reaches
/// exactly one seam: it covers the variants written into its own list, so
/// a variant added to `Event` is covered only once someone adds it to a
/// group's or-pattern and to that list, both by hand.
///
/// Within that list the routing is total: a variant no group claims panics
/// naming itself rather than passing a suite vacuously, and a variant a
/// group claims but does not destructure records nothing and fails the
/// test.
///
/// # Panics
///
/// When `event` is a variant none of the groups above claims.
pub(crate) fn forward_one(event: Event, observer: &dyn Observer, debug: Option<&dyn DebugCapture>) {
    if let Some(observation) = unit_observation(&event) {
        observer.observe(event.section(), observation);
        return;
    }
    match event {
        task_lifecycle_variants!() => forward_lifecycle(event, observer),
        content_variants!() => forward_content(event, observer),
        debug_variants!() => forward_debug(event, debug),
        // The payload-free variants were forwarded above; anything else
        // is a variant no group claims (`Event` is `#[non_exhaustive]` in
        // `runvil-types`), which the test recorder must never
        // drop in silence.
        _ => unreachable!("Event variant no group claims: {event:?}"),
    }
}

/// The lifecycle, task, and inbox variants with payloads, as
/// observations.
fn forward_lifecycle(event: Event, observer: &dyn Observer) {
    match event {
        Event::ModelMetadataDegraded {
            section, message, ..
        } => observer.observe(
            &section,
            Observation::Other(format!("Model metadata degraded: {message}")),
        ),
        Event::InboxMessage { section, text, .. } => {
            observer.observe(
                &section,
                Observation::Other(format!("Inbox message: {text}")),
            );
        }
        Event::LuaLibraryLoadStarted {
            section, library, ..
        } => observer.observe(&section, Observation::LuaLibraryLoadStarted { library }),
        Event::LuaLibraryLoadSucceeded {
            section, library, ..
        } => observer.observe(&section, Observation::LuaLibraryLoadSucceeded { library }),
        Event::LuaLibraryLoadFailed {
            section, library, ..
        } => observer.observe(&section, Observation::LuaLibraryLoadFailed { library }),
        Event::Lua {
            section, message, ..
        } => observer.observe(&section, Observation::Lua(message)),
        _ => forward_task(event, observer),
    }
}

/// The task variants, as observations.
fn forward_task(event: Event, observer: &dyn Observer) {
    match event {
        Event::TaskStarted {
            section,
            task,
            target,
            input,
            item,
            index,
            var,
            ..
        } => observer.observe(
            &section,
            Observation::TaskStarted {
                task,
                target,
                input,
                item,
                index,
                var,
            },
        ),
        Event::TaskSucceeded { section, task, .. } => {
            observer.observe(&section, Observation::TaskSucceeded { task });
        }
        Event::TaskFailed { section, task, .. } => {
            observer.observe(&section, Observation::TaskFailed { task });
        }
        Event::TaskCanceled { section, task, .. } => {
            observer.observe(&section, Observation::TaskCanceled { task });
        }
        Event::TaskAbandoned {
            section,
            task,
            reason,
            ..
        } => observer.observe(&section, Observation::TaskAbandoned { task, reason }),
        // Routed here by `forward_one` for this group alone; `Event` is
        // `#[non_exhaustive]` in `runvil-types`.
        _ => {}
    }
}

/// The content variants, as the observer's `on_*` hooks.
fn forward_content(event: Event, observer: &dyn Observer) {
    match event {
        Event::Thinking {
            section,
            turn,
            model,
            text,
            ..
        } => observer.on_thinking(ThinkingReport {
            section: &section,
            turn,
            model: &model,
            text: &text,
        }),
        Event::AssistantReply {
            section,
            turn,
            text,
            finish_reason,
            model,
            metrics,
            origin,
            ..
        } => observer.on_assistant_reply(ReplyReport {
            section: &section,
            turn,
            text: &text,
            finish_reason: finish_reason.as_deref(),
            model: &model,
            metrics: metrics.as_ref(),
            origin,
        }),
        Event::AssistantToolCalls {
            section,
            turn,
            model,
            calls,
            ..
        } => observer.on_assistant_tool_calls(ToolCallsReport {
            section: &section,
            turn,
            model: &model,
            calls: &calls,
        }),
        Event::ToolResult {
            section,
            turn,
            tool_call_id,
            alias,
            content,
            trusted,
            ..
        } => observer.on_tool_result(ToolResultReport {
            section: &section,
            turn,
            tool_call_id: &tool_call_id,
            alias: &alias,
            content: &content,
            trust: if trusted {
                OutputTrust::Trusted
            } else {
                OutputTrust::Untrusted
            },
        }),
        // Routed here by `forward_one` for this group alone; `Event` is
        // `#[non_exhaustive]` in `runvil-types`.
        _ => {}
    }
}

/// The debug pair, as the capture's events; dropped when the caller set no
/// capture.
fn forward_debug(event: Event, debug: Option<&dyn DebugCapture>) {
    match event {
        Event::Request {
            section,
            turn,
            body,
            ..
        } => {
            if let Some(capture) = debug {
                capture.on_event(&section, turn, DebugEvent::request(body));
            }
        }
        Event::Response {
            section,
            turn,
            body,
            finish_reason,
            reasoning_content,
            ..
        } => {
            if let Some(capture) = debug {
                capture.on_event(
                    &section,
                    turn,
                    DebugEvent::response(body, finish_reason, reasoning_content),
                );
            }
        }
        // Routed here by `forward_one` for this group alone; `Event` is
        // `#[non_exhaustive]` in `runvil-types`.
        _ => {}
    }
}

#[cfg(test)]
#[path = "forward-tests.rs"]
mod tests;
