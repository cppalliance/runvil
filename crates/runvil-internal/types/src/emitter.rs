//! The run-level event buffer and the task-scoped emitter over it.
//!
//! The Engine reports as values: every boundary, content report, and
//! debug capture becomes one [`Event`] pushed into the run's buffer,
//! stamped with a [`Provenance`] - the nearest enclosing task and that
//! task's next sequence number - and drained by the run's `step` and
//! `deliver` through the [`EventSink`]. The caller reads each drained
//! batch, the one path by which an event leaves this module.
//!
//! An [`Emitter`] is one chain's handle onto the buffer: it knows its task
//! (the main walk is task `0`; a `call` child shares its caller's emitter,
//! so it reports under the caller's task; a spawned chain gets its own
//! through [`Emitter::for_task`]) and stamps every event it pushes with
//! that task's next `seq`. The counters sit in the buffer, under its one
//! lock, so a task's sequence is dense from zero however its chains and
//! the run's leaf tasks interleave.
//!
//! The emitter is the one reporting seam every Engine crate takes: the
//! section VM reports its chunk boundaries through it, the tool-dispatch
//! body its results, the scheduler everything else. It sits in this crate
//! so those crates can name it without depending on the runtime.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::event::Event;
use crate::event::ReplyOrigin;
use crate::event_lifecycle::Lifecycle;
use crate::ids::{ChainId, Provenance, RoundId, TaskId};
use crate::metrics::{CallMetrics, ToolCallEvent};
use crate::tools::{OutputTrust, ToolId};

/// Names one phase of loading a library section into a section's environment.
///
/// [`Emitter::library_load`] reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LibraryLoad {
    /// Loading began.
    Started,
    /// The library's blocks ran to completion.
    Succeeded,
    /// Loading the library or running its blocks returned an error.
    Failed,
}

/// The run's event buffer: the events not yet drained, plus one sequence
/// counter per task the run has reported under.
#[derive(Debug, Default)]
struct EventBuffer {
    /// The events pushed since the last drain, in push order.
    events: Vec<Event>,
    /// Each task's next sequence number.
    seqs: HashMap<TaskId, u32>,
}

impl EventBuffer {
    /// Allocates `task`'s next provenance: its current counter, advanced
    /// by one. Saturating at `u32::MAX`, which no reachable run approaches.
    fn next_provenance(&mut self, task: &TaskId) -> Provenance {
        let seq = self.seqs.entry(task.clone()).or_insert(0);
        let provenance = Provenance::new(task.clone(), *seq);
        *seq = seq.saturating_add(1);
        provenance
    }
}

/// The shared handle onto one run's event buffer: every chain's emitter
/// pushes through a clone, and the run drains through its own.
#[derive(Clone, Debug, Default)]
pub struct EventSink(Arc<Mutex<EventBuffer>>);

impl EventSink {
    /// Pushes one event built from `task`'s next provenance. The lock is
    /// held only for the allocation and the push; `build` runs under it,
    /// so it must not touch the sink.
    fn push(&self, task: &TaskId, build: impl FnOnce(Provenance) -> Event) {
        // A poisoned lock means an emitter panicked mid-push; the buffer's
        // contents are still consistent, so recover it rather than turn a
        // report into a second panic.
        let mut buffer = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let provenance = buffer.next_provenance(task);
        buffer.events.push(build(provenance));
    }

    /// Allocates `task`'s next provenance without pushing an event: the
    /// stamp on an issued effect, drawn from the same counter as the
    /// task's events so effects and events from one task share one dense
    /// sequence.
    fn allocate(&self, task: &TaskId) -> Provenance {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .next_provenance(task)
    }

    /// Takes every event pushed since the last drain, in push order.
    #[must_use]
    pub fn take(&self) -> Vec<Event> {
        let mut buffer = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut buffer.events)
    }
}

/// One task's handle onto the run's event buffer: every event it pushes is
/// stamped with the task's next sequence number. Cheap to clone; a clone
/// shares the task and the buffer.
///
/// Every report is write-only: the Engine never reads an event back
/// through this path, so recording every event or dropping them all
/// leaves a run's outputs, errors, and ordering unchanged.
#[derive(Clone, Debug)]
pub struct Emitter {
    /// The run's buffer.
    sink: EventSink,
    /// The nearest enclosing task of the chain this emitter serves.
    task: TaskId,
    /// Whether the caller asked for raw request/response capture: the model
    /// rounds emit `Request` and `Response` only when it is set, so a run
    /// that did not opt in never clones a body.
    debug: bool,
}

impl Emitter {
    /// Builds the emitter for `task` over `sink`, capturing raw model-turn
    /// bodies when `debug` is set.
    #[must_use]
    pub fn new(sink: EventSink, task: TaskId, debug: bool) -> Self {
        Self { sink, task, debug }
    }

    /// The root task's emitter over `sink`: the main walk is task `0`.
    #[must_use]
    pub fn root(sink: EventSink, debug: bool) -> Self {
        Self::new(sink, TaskId::from(ChainId::root()), debug)
    }

    /// The emitter a spawned chain reports through: the same buffer under
    /// the chain's own task.
    #[must_use]
    pub fn for_task(&self, task: TaskId) -> Self {
        Self {
            sink: self.sink.clone(),
            task,
            debug: self.debug,
        }
    }

    /// The task this emitter stamps its events with.
    #[must_use]
    pub fn task(&self) -> &TaskId {
        &self.task
    }

    /// Whether the run captures raw model-turn bodies.
    #[must_use]
    pub fn captures_debug(&self) -> bool {
        self.debug
    }

    /// Stamps one issued effect, or an event the Engine returns to its
    /// caller instead of pushing it: this task's next provenance, drawn
    /// from the counter its events advance, so the stamped value orders
    /// among them.
    #[must_use]
    pub fn stamp_effect(&self) -> Provenance {
        self.sink.allocate(&self.task)
    }

    /// Pushes one event built from this task's next coordinates: the
    /// general form every named report below is a case of, for the
    /// variants with a payload and no dedicated method.
    pub fn emit(&self, section: &str, build: impl FnOnce(String, Provenance) -> Event) {
        let section = section.to_owned();
        self.sink
            .push(&self.task, |provenance| build(section, provenance));
    }

    /// Reports one payload-free lifecycle boundary under `section`.
    pub fn report(&self, section: &str, boundary: Lifecycle) {
        self.emit(section, boundary);
    }

    /// Reports one boundary of loading `library`, the library section's
    /// heading with its markers, into `section`'s environment.
    pub fn library_load(&self, section: &str, library: &str, phase: LibraryLoad) {
        let library = library.to_owned();
        self.emit(section, |section, provenance| match phase {
            LibraryLoad::Started => Event::LuaLibraryLoadStarted {
                section,
                provenance,
                library,
            },
            LibraryLoad::Succeeded => Event::LuaLibraryLoadSucceeded {
                section,
                provenance,
                library,
            },
            LibraryLoad::Failed => Event::LuaLibraryLoadFailed {
                section,
                provenance,
                library,
            },
        });
    }

    /// Reports the author's `log(message)` checkpoint.
    pub fn lua(&self, section: &str, message: &str) {
        self.emit(section, |section, provenance| Event::Lua {
            section,
            provenance,
            message: message.to_owned(),
        });
    }

    /// Reports one completed block of model thinking from `round`.
    pub fn thinking(&self, section: &str, turn: u32, round: RoundId, model: &str, text: &str) {
        self.emit(section, |section, provenance| Event::Thinking {
            section,
            provenance,
            turn,
            round,
            model: model.to_owned(),
            text: text.to_owned(),
        });
    }

    /// Reports one completed assistant reply: `round`'s text reply with
    /// its [`ReplyOrigin`] provenance.
    #[expect(
        clippy::too_many_arguments,
        reason = "the reply report names its full run coordinates, including the round and origin, in one call"
    )]
    pub fn assistant_reply(
        &self,
        section: &str,
        turn: u32,
        round: RoundId,
        text: &str,
        finish_reason: Option<&str>,
        model: &str,
        metrics: Option<&CallMetrics>,
        origin: ReplyOrigin,
    ) {
        self.emit(section, |section, provenance| Event::AssistantReply {
            section,
            provenance,
            turn,
            round,
            text: text.to_owned(),
            finish_reason: finish_reason.map(str::to_owned),
            model: model.to_owned(),
            metrics: metrics.cloned(),
            origin,
        });
    }

    /// Reports one batch of tool calls the model requested in `round`,
    /// unexecuted.
    pub fn assistant_tool_calls(
        &self,
        section: &str,
        turn: u32,
        round: RoundId,
        model: &str,
        calls: &[ToolCallEvent],
    ) {
        self.emit(section, |section, provenance| Event::AssistantToolCalls {
            section,
            provenance,
            turn,
            round,
            model: model.to_owned(),
            calls: calls.to_vec(),
        });
    }

    /// Reports the result of one dispatched tool call. `tool` is the tool
    /// `alias` resolved to. `vendor` is the vendor of the Pack that
    /// answered it, or `None` for an adhoc tool or when the caller reported
    /// none. The event records `trust` as its `trusted` flag: `true` only
    /// for [`OutputTrust::Trusted`].
    #[expect(
        clippy::too_many_arguments,
        reason = "the result report names its full run coordinates, including the alias, the tool it resolved to, and its vendor, in one call"
    )]
    pub fn tool_result(
        &self,
        section: &str,
        turn: u32,
        tool_call_id: &str,
        alias: &str,
        tool: &ToolId,
        vendor: Option<&str>,
        content: &str,
        trust: OutputTrust,
    ) {
        let trusted = trust == OutputTrust::Trusted;
        self.emit(section, |section, provenance| Event::ToolResult {
            section,
            provenance,
            turn,
            tool_call_id: tool_call_id.to_owned(),
            alias: alias.to_owned(),
            tool: tool.clone(),
            vendor: vendor.map(str::to_owned),
            content: content.to_owned(),
            trusted,
        });
    }

    /// Captures the request body of one completed model turn. The caller
    /// gates on [`captures_debug`](Self::captures_debug) so a run that did
    /// not opt in never clones a body.
    pub fn request(&self, section: &str, turn: u32, body: Value) {
        self.emit(section, |section, provenance| Event::Request {
            section,
            provenance,
            turn,
            body,
        });
    }

    /// Captures the response body of one completed model turn, with its
    /// parsed metadata. Gated as [`request`](Self::request) is.
    pub fn response(
        &self,
        section: &str,
        turn: u32,
        body: Value,
        finish_reason: Option<String>,
        reasoning_content: Option<String>,
    ) {
        self.emit(section, |section, provenance| Event::Response {
            section,
            provenance,
            turn,
            body,
            finish_reason,
            reasoning_content,
        });
    }
}

#[cfg(test)]
mod tests {
    //! Tests for `Emitter` sequencing, lifecycle reports, and payload events.

    use super::{Emitter, EventSink, LibraryLoad};
    use crate::event::Event;
    use crate::event_lifecycle as lifecycle;
    use crate::ids::{AbandonReason, ChainId, Provenance, RoundId, TaskId};
    use crate::tools::{OutputTrust, ToolId};

    fn root() -> TaskId {
        TaskId::from(ChainId::root())
    }

    fn emitter(sink: &EventSink, task: TaskId) -> Emitter {
        Emitter::new(sink.clone(), task, false)
    }

    #[test]
    fn each_task_counts_its_own_sequence_from_zero() {
        let sink = EventSink::default();
        let walk = emitter(&sink, root());
        let arm = walk.for_task("0.0".parse().expect("a task id parses"));
        walk.report("A", lifecycle::SECTION_STARTED);
        arm.report("W", lifecycle::SECTION_STARTED);
        walk.report("A", lifecycle::LUA_CHUNK_STARTED);
        arm.report("W", lifecycle::LUA_CHUNK_STARTED);
        arm.report("W", lifecycle::LUA_CHUNK_SUCCEEDED);

        let stamps: Vec<(String, u32)> = sink
            .take()
            .iter()
            .map(|event| {
                let provenance = event.provenance();
                (provenance.task.to_string(), provenance.seq)
            })
            .collect();
        assert_eq!(
            stamps,
            vec![
                ("0".to_owned(), 0),
                ("0.0".to_owned(), 0),
                ("0".to_owned(), 1),
                ("0.0".to_owned(), 1),
                ("0.0".to_owned(), 2),
            ],
            "interleaved tasks keep independent dense sequences"
        );
        assert!(sink.take().is_empty(), "a drain empties the buffer");
    }

    #[test]
    fn a_lifecycle_report_becomes_the_matching_event_with_its_coordinates() {
        let sink = EventSink::default();
        let walk = emitter(&sink, root());
        walk.report("Gather", lifecycle::VFS_WRITE_SUCCEEDED);
        assert_eq!(
            sink.take(),
            vec![Event::VfsWriteSucceeded {
                section: "Gather".to_owned(),
                provenance: Provenance::new(root(), 0),
            }]
        );
    }

    #[test]
    fn an_effect_stamp_shares_the_task_sequence_with_its_events() {
        let sink = EventSink::default();
        let walk = emitter(&sink, root());
        walk.report("A", lifecycle::SECTION_STARTED);
        let stamp = walk.stamp_effect();
        walk.report("A", lifecycle::SECTION_FINISHED);
        let events = sink.take();
        assert_eq!(
            stamp.seq, 1,
            "the effect takes the sequence between the two events"
        );
        assert_eq!(events[0].provenance().seq, 0);
        assert_eq!(events[1].provenance().seq, 2);
    }

    #[test]
    fn payload_variants_cross_field_for_field() {
        let sink = EventSink::default();
        let walk = emitter(&sink, root());
        let task: TaskId = "0.3".parse().expect("a task id parses");
        walk.emit("Spawner", |section, provenance| Event::TaskStarted {
            section,
            provenance,
            task: task.clone(),
            target: "Worker".to_owned(),
            input: Some("in".to_owned()),
            item: Some(serde_json::json!("x")),
            index: Some(2),
            var: serde_json::json!({ "k": 1 }),
        });
        walk.emit("Worker", |section, provenance| Event::TaskAbandoned {
            section,
            provenance,
            task: task.clone(),
            reason: AbandonReason::OwnerFailed,
        });
        walk.lua("Worker", "checkpoint");
        let events = sink.take();
        assert!(matches!(
            &events[0],
            Event::TaskStarted { task: started, target, input: Some(input), item: Some(_), index: Some(2), var, .. }
                if *started == task && target == "Worker" && input == "in" && var["k"] == 1
        ));
        assert!(matches!(
            &events[1],
            Event::TaskAbandoned { task: ended, reason: AbandonReason::OwnerFailed, .. } if *ended == task
        ));
        assert!(matches!(&events[2], Event::Lua { message, .. } if message == "checkpoint"));
    }

    #[test]
    fn a_library_load_report_stamps_the_coordinates_and_the_library() {
        let sink = EventSink::default();
        let walk = emitter(&sink, root());
        walk.library_load("Main", "## shared_stuff", LibraryLoad::Started);
        walk.library_load("Main", "## shared_stuff", LibraryLoad::Succeeded);
        walk.library_load("Other", "## helpers", LibraryLoad::Failed);
        let stamp = |seq| Provenance::new(root(), seq);
        assert_eq!(
            sink.take(),
            vec![
                Event::LuaLibraryLoadStarted {
                    section: "Main".to_owned(),
                    provenance: stamp(0),
                    library: "## shared_stuff".to_owned(),
                },
                Event::LuaLibraryLoadSucceeded {
                    section: "Main".to_owned(),
                    provenance: stamp(1),
                    library: "## shared_stuff".to_owned(),
                },
                Event::LuaLibraryLoadFailed {
                    section: "Other".to_owned(),
                    provenance: stamp(2),
                    library: "## helpers".to_owned(),
                },
            ],
            "each phase builds its own variant under the loading section"
        );
    }

    #[test]
    fn content_reports_land_in_the_buffer_in_order() {
        let sink = EventSink::default();
        let walk = emitter(&sink, root());
        let echo = ToolId::parse("tools/echo").expect("a valid tool id");
        walk.tool_result(
            "Chat",
            3,
            "call_1",
            "echo",
            &echo,
            Some("acme"),
            "out",
            OutputTrust::Trusted,
        );
        walk.thinking("Chat", 3, RoundId::new(2), "m", "hmm");
        let events = sink.take();
        assert!(matches!(
            &events[0],
            Event::ToolResult { turn: 3, tool_call_id, alias, tool, vendor: Some(vendor), content, trusted: true, .. }
                if tool_call_id == "call_1" && alias == "echo" && *tool == echo && vendor == "acme" && content == "out"
        ));
        assert!(matches!(
            &events[1],
            Event::Thinking { text, round, .. } if text == "hmm" && *round == RoundId::new(2)
        ));
        assert_eq!(events[1].provenance().seq, 1);
    }

    #[test]
    fn an_untrusted_tool_result_reports_the_event_with_trusted_false() {
        let sink = EventSink::default();
        let walk = emitter(&sink, root());
        let fetch = ToolId::parse("web/fetch").expect("a valid tool id");
        walk.tool_result(
            "Chat",
            2,
            "call_9",
            "fetch",
            &fetch,
            None,
            "<html>",
            OutputTrust::Untrusted,
        );
        assert_eq!(
            sink.take(),
            vec![Event::ToolResult {
                section: "Chat".to_owned(),
                provenance: Provenance::new(root(), 0),
                turn: 2,
                tool_call_id: "call_9".to_owned(),
                alias: "fetch".to_owned(),
                tool: fetch,
                vendor: None,
                content: "<html>".to_owned(),
                trusted: false,
            }],
            "an untrusted marking lands on the wire as `trusted: false`"
        );
    }

    #[test]
    fn the_root_emitter_reports_under_task_zero() {
        let sink = EventSink::default();
        let emitter = Emitter::root(sink.clone(), true);
        assert!(emitter.captures_debug());
        assert_eq!(emitter.task(), &root());
        emitter.report("Main", lifecycle::RUN_STARTED);
        let events = sink.take();
        assert_eq!(events[0].provenance().task, root());
    }
}
