//! The per-section frame: one section entry's owned state within a run.
//!
//! [`SectionContext`] is born at a section entry and dies at its teardown.
//! It owns the section VM plus the state the block walk reads and writes -
//! the section's [`SectionSys`], a spawned chain's `item`, and the tool-call counts -
//! and it holds the frame's effective reporting handles
//! (the task-scoped event emitter and the turn counter) seeded out of the
//! run context; a task chain's context is the spawn's fork, so the handles
//! reach the frame and the task's nested chains through the one value.
//! Each section entry is one construct-run-teardown cycle: a frame
//! constructor absorbs the VM construction and setup preamble
//! ([`SectionContext::new`] for a walked section, a spawned task's first
//! entry included, and [`SectionContext::new_h1`] for the H1 pass;
//! the two sit in the `construct` sibling), the scheduler's chain
//! steps run the blocks, and the frame's [`Drop`] impl is the single
//! teardown boundary.
//!
//! The run-scoped inputs (bindings, models, limits, the shared tools)
//! arrive through the [`RunState`].

#[path = "section_context-construct.rs"]
mod construct;

use std::sync::Arc;

use runvil_types::event::lifecycle;
use runvil_types::ids::TaskId;
use runvil_types::tools::ToolId;

use crate::lua::{ProseState, ScriptReport, SectionVm, ToolCallCounts};
use crate::{Error, Result, subst};

use super::context::{ReportingHandles, RunState};

/// The seeds a spawned task chain's first section entry receives beyond the
/// shared Engine values: `tasks.spawn`'s `opts.item` (installed as the
/// `item` global and the `{{ item }}` substitution source) and `opts.index`
/// (reported as `sys.index`). Empty for every other entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TaskSeed {
    /// The chain's `item` global, when the spawn gave one.
    pub(super) item: Option<serde_json::Value>,
    /// The chain's `sys.index`, when the spawn gave one.
    pub(super) index: Option<u64>,
}

/// The values one section entry's Lua reads as `sys`.
///
/// `index` and `model` are left out of [`SectionSys::to_json`] when unset,
/// never written as null: Lua raises on an absent `sys` field but reads a
/// null one as nil.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SectionSys {
    /// The run's `started_at` as RFC 3339, the same in every section.
    pub(super) when: Arc<str>,
    /// The section entry's hierarchical id.
    pub(super) id: String,
    /// The nearest enclosing task: the main walk is task `0`, and a `call`
    /// child reports its caller's task.
    pub(super) taskid: TaskId,
    /// The section's name, or the prompt's title for the H1 pass.
    pub(super) section_name: String,
    /// The run's execution identifier.
    pub(super) execution: Arc<str>,
    /// The prompt's top-level section count.
    pub(super) section_count: usize,
    /// A spawned chain's `opts.index`, on its first entry only.
    pub(super) index: Option<u64>,
    /// The name of the section's bound model, set by the first-call install.
    pub(super) model: Option<String>,
}

impl SectionSys {
    /// The JSON object the section VM seals as `sys`.
    pub(super) fn to_json(&self) -> serde_json::Value {
        let mut sys = serde_json::json!({
            "when": &*self.when,
            "id": self.id,
            "taskid": self.taskid.to_string(),
            "section_name": self.section_name,
            "execution": &*self.execution,
            "section_count": self.section_count,
        });
        if let Some(index) = self.index {
            sys["index"] = serde_json::Value::from(index);
        }
        if let Some(model) = &self.model {
            sys["model"] = serde_json::Value::from(model.as_str());
        }
        sys
    }
}

/// One adhoc tool call whose handler is running in the frame's block
/// coroutine: opened when the `tool_call` is answered with the handler,
/// closed by the shim's `adhoc_tool_done`, which reports under it.
pub(super) struct AdhocCall {
    /// The adhoc tool's id, which its result reports under.
    pub(super) tool: ToolId,
    /// The adhoc tool's wire name.
    pub(super) alias: String,
    /// The model-issued call id, `None` for a script call.
    pub(super) call_id: Option<String>,
    /// The coordinates recorded at dispatch, so the result reports the
    /// model turn the call was made in even when the handler runs model
    /// rounds of its own.
    pub(super) report: ScriptReport,
    /// Whether the handler's own `tools.call` received a listing while
    /// this call was the frame's innermost open one, so its result is
    /// wrapped and reported untrusted.
    pub(super) listing: bool,
}

/// One section entry's owned frame within a run.
///
/// The frame is born at a section entry and dies at its teardown. One
/// section entry is one frame, regardless of arrival mode (fall-through,
/// jump, call); a jump ends the current frame and the driver builds a
/// fresh one for the target - only `var` crosses, as call data.
/// No derives: the VM and the trait-object handles support neither `Clone`
/// nor `Debug`.
pub(super) struct SectionContext {
    /// The frame's VM: the owned section VM, `Some` from construction
    /// until the frame's `Drop` takes it for the teardown boundary.
    /// `SectionVm` is `runvil-lua`'s type, tested in that crate.
    vm: Option<SectionVm>,
    /// Armed by [`SectionContext::mark_completed`] on the success path
    /// only, so `Drop` fires `SECTION_FINISHED` on completion (a jump or
    /// return included) and never on an error.
    completed: bool,
    /// The section's `sys`, whose `section_name` `Drop` reports the
    /// teardown boundary and the completion observation under. The
    /// first-call install sets its `model`.
    sys: SectionSys,
    /// A spawned chain's `item` seed (a fanout arm's collection member) for
    /// `{{ item }}` substitution; `None` on every other entry.
    item: Option<serde_json::Value>,
    /// The per-section tool-call counts, set once a tool call, a script's
    /// or a model's, has fully installed the section's first-call state,
    /// and bumped on the driver thread.
    counts: Option<ToolCallCounts>,
    /// The adhoc tool calls whose handlers are running, innermost last: a
    /// handler that calls another adhoc tool opens a nested entry, and
    /// each `adhoc_tool_done` closes the innermost.
    adhoc_calls: Vec<AdhocCall>,
    /// The frame's reporting handles: the chain's own, so every report
    /// the frame makes - the teardown boundary, the completion - is
    /// stamped with the chain's task.
    reporting: ReportingHandles,
}

impl SectionContext {
    /// Reads the section's final `var` out of the VM, so the walk rolls it
    /// forward. Must run while the frame is live, before its drop: the read
    /// goes through the live VM.
    ///
    /// # Errors
    /// Returns [`Error::Lua`] if the VM's `var` cannot be
    /// converted back to JSON (the write guard keeps this conversion from
    /// failing in practice), or [`Error::Internal`] if the VM is gone.
    pub(super) fn read_var(&self) -> Result<serde_json::Value> {
        Ok(self.vm()?.var()?)
    }

    /// Reads the H1 pass's `argv` back as JSON while the frame is live: the
    /// value the walk's sections inherit frozen - the derived parse, or H1's
    /// repair. `None` reads as nil.
    ///
    /// # Errors
    /// Returns [`Error::Lua`] when H1 left `argv` as
    /// non-JSON data, or [`Error::Internal`] if the VM is gone.
    pub(super) fn read_argv(&self) -> Result<Option<serde_json::Value>> {
        self.vm()?.argv_json().map_err(Error::from)
    }

    /// Arms the completion flag: the block walk completed (a jump or
    /// return included) and the final `var` is read back, so the frame's
    /// drop fires `SECTION_FINISHED` after the teardown pair. No error
    /// path arms it, so an error never fires `SECTION_FINISHED`.
    pub(super) fn mark_completed(&mut self) {
        self.completed = true;
    }

    /// Borrows the frame's VM for the scheduler's coroutine driving (block
    /// start, yield validation, answer resume) and model resolution.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] if the VM is gone, which only the frame's
    /// own drop does - a live frame always holds it.
    pub(super) fn vm(&self) -> Result<&SectionVm> {
        self.vm.as_ref().ok_or(Error::internal(
            "the section frame's VM lives until the frame's own drop",
        ))
    }

    /// The section entry's id: the value its Lua reads as `sys.id`, which
    /// the walk assigned when it entered the section.
    pub(super) fn entry_id(&self) -> &str {
        &self.sys.id
    }

    /// Installs the pending Markdown buffer as the VM's fresh read-only
    /// lazy `prose` template before one Lua coroutine starts. The first
    /// runtime read snapshots the section state, renders every `{{ }}`
    /// substitution once, and memoizes the result; the buffer is the
    /// scheduler's pending prose, empty when no Markdown accumulated.
    ///
    /// # Errors
    /// Returns [`Error::Lua`] if the guard cannot be installed, or
    /// [`Error::Internal`] if the VM is gone.
    pub(super) fn install_lazy_prose(&self, ctx: &RunState, template: &str) -> Result<()> {
        let template = template.to_owned();
        let render = prose_renderer(ctx, self.item.clone());
        self.vm()?
            .install_lazy_prose(move |state: ProseState<'_>| render(&template, state))?;
        Ok(())
    }

    /// The frame's effective reporting handles for the `chat` rounds the
    /// scheduler applies on this chain (the `models.loop` shim's among
    /// them), seeded out of the run context (a spawned task's fork) at
    /// construction.
    pub(super) fn reporting_handles(&self) -> &ReportingHandles {
        &self.reporting
    }

    /// The frame's tool-call counts for a tool call, a script's or a
    /// model's: the section's first call runs the first-call install (the
    /// counts and the Lua `tools.stats` table, the model freeze, the
    /// `sys.model` enrichment), a later call runs it again when it failed,
    /// and every call then seeds any of `ids` not seeded yet. `ids` is
    /// what Lua can see in the section's two sets, from its live offers
    /// before it freezes them and from its frozen sets after, with the
    /// discovery tools' ids once they are rendered. The returned handle
    /// shares the installed counts, which the caller bumps on the driver
    /// thread.
    ///
    /// # Errors
    /// Returns the [`Error`] of the first-call install or the
    /// id seeding.
    pub(super) fn tool_call_counts(
        &mut self,
        ctx: &RunState,
        ids: &[ToolId],
    ) -> Result<ToolCallCounts> {
        let Self {
            vm, sys, counts, ..
        } = self;
        let Some(vm) = vm.as_ref() else {
            return Err(Error::internal(
                "the section frame's VM lives until the frame's own drop",
            ));
        };
        let counts = match counts {
            Some(counts) => counts,
            None => counts.insert(install_first_call_state(vm, ctx, sys, ids)?),
        };
        for id in ids {
            counts.ensure(&id.to_string())?;
        }
        Ok(counts.clone())
    }

    /// Opens an adhoc tool call whose handler the shim is about to run.
    pub(super) fn push_adhoc_call(&mut self, call: AdhocCall) {
        self.adhoc_calls.push(call);
    }

    /// Closes the innermost open adhoc tool call; `None` when none is open.
    pub(super) fn pop_adhoc_call(&mut self) -> Option<AdhocCall> {
        self.adhoc_calls.pop()
    }

    /// Marks the innermost open adhoc tool call as having received a
    /// listing; a frame with no open call marks nothing.
    pub(super) fn mark_listing(&mut self) {
        if let Some(call) = self.adhoc_calls.last_mut() {
            call.listing = true;
        }
    }
}

/// The `{{ }}` renderer for one frame's prose templates, its own blocks'
/// and the libraries `require` loads alike: each render substitutes from
/// the run's `args` and `argv`, shared with the run rather than copied,
/// the frame's `item`, and the section state the first read snapshots.
fn prose_renderer(
    ctx: &RunState,
    item: Option<serde_json::Value>,
) -> impl Fn(&str, ProseState<'_>) -> mlua::Result<String> + Send + Sync + 'static {
    let raw_args = Arc::clone(ctx.args());
    let argv = ctx.argv().cloned();
    move |template: &str, state: ProseState<'_>| -> mlua::Result<String> {
        let globals = |name: &str| (state.globals)(name).map_err(Error::from);
        let sources = subst::Sources {
            args: &raw_args,
            argv: argv.as_deref(),
            item: item.as_ref(),
            var: &state.var,
            sys: &state.sys,
            globals: &globals,
        };
        subst::substitute(template, &sources).map_err(mlua::Error::external)
    }
}

/// Installs what a section's first tool call fixes: the counts behind
/// `tools.stats` and the model binding (`sys.model`).
///
/// The counts install backs the Lua `tools.stats` table, seeded with
/// `ids`, and the model resolution freezes the section's binding and
/// sets `sys.model` to the bound model's name. The caller stores the
/// returned counts, so its slot is set only once every step has
/// succeeded, and a failed install runs again on the section's next tool
/// call. `sys` changes only once the VM's re-seal succeeds.
///
/// # Errors
/// Returns the [`Error`] of the counts install, the model resolution, or
/// the `sys` re-seal.
fn install_first_call_state(
    vm: &SectionVm,
    ctx: &RunState,
    sys: &mut SectionSys,
    ids: &[ToolId],
) -> Result<ToolCallCounts> {
    let counts = vm.install_tool_call_counts(ids)?;
    let resolved_model = crate::lua::resolve_model_binding(ctx.models(), vm.model_runtime())?;
    if let Some(binding) = resolved_model.as_ref() {
        let mut enriched = sys.clone();
        enriched.model = Some(binding.id().name().to_owned());
        vm.re_seal_sys(&enriched.to_json())?;
        *sys = enriched;
    }
    Ok(counts)
}

impl Drop for SectionContext {
    fn drop(&mut self) {
        // The single teardown boundary: every exit path - success, error,
        // or early return - drops the frame, so the VM tears down exactly
        // once here. `SECTION_FINISHED` follows only on the armed
        // (completed) path; an error reports the teardown pair alone. The
        // `let-else` is defensive: `Drop` runs once, so the VM is always
        // here, and the destructor stays infallible.
        let Some(vm) = self.vm.take() else {
            return;
        };
        // The VM's teardown pair reports through the emitter's observer
        // seam, so it lands in the buffer ahead of the completion below.
        vm.teardown(self.reporting.emitter.as_ref(), &self.sys.section_name);
        if self.completed {
            self.reporting
                .emitter
                .report(&self.sys.section_name, lifecycle::SECTION_FINISHED);
        }
    }
}

#[cfg(test)]
#[path = "section_context-tests.rs"]
mod tests;
