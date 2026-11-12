//! The per-section frame: one section entry's owned state within a run.
//!
//! [`SectionContext`] is born at a section entry and dies at its teardown.
//! It owns the section VM plus the state the block walk reads and writes -
//! the section's [`SectionSys`], a spawned chain's `item`, the tool-call
//! counts, the stack of open [`AdhocCall`]s, and the completion flag its
//! `Drop` reads - and it holds the frame's effective reporting handles
//! (the task-scoped event emitter and the turn counter) seeded out of the
//! run context; a task chain's context is the spawn's fork, so the handles
//! reach the frame and the task's nested chains through the one value.
//! Each section entry is one construct-run-teardown cycle: a constructor
//! (one for a walked section, one for the H1 pass) builds the frame, the
//! scheduler's chain steps run the blocks, and the frame's [`Drop`] impl is
//! the single teardown boundary.
//!
//! The run-scoped inputs (bindings, models, limits, the shared tools)
//! arrive through the [`ChainState`].

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use runvil_types::event_lifecycle;
use runvil_types::ids::TaskId;
use runvil_types::tools::ToolId;
use runvil_vfs::{Access, ScopeHandle};

use crate::lua::{Library, ProseState, ScriptReport, SectionVm, ToolCallCounts, Uninjected};
use crate::model::ModelBinding;
use crate::prompt::Prompt;
use crate::section_path::Origin;
use crate::section_pos::SectionPos;
use crate::{Result, error, subst};

use super::context::{ChainState, ReportingHandles};
use super::section_vm::{self, VmSeed};
use super::walk_target;

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
    /// The section entry's hierarchical id: the entering chain's id
    /// extended by its local entry counter, rendered as a dot-separated
    /// path.
    pub(super) id: String,
    /// The nearest enclosing task: the main walk is task `0`, and a `call`
    /// child reports its caller's task. A chain passes it to `tasks.*` to
    /// speak about itself.
    pub(super) taskid: TaskId,
    /// The section's name, or the prompt's title for the H1 pass. Fixed at
    /// construction, and the name the frame's `Drop` reports the teardown
    /// boundary and the completion observation under.
    pub(super) section_name: String,
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
/// Not `Clone`: the frame owns the VM and its one teardown.
pub(super) struct SectionContext {
    /// The frame's VM; [`live_vm`] states when the slot holds it.
    vm: Option<SectionVm>,
    /// Whether [`SectionContext::mark_completed`] armed the frame's
    /// `SECTION_FINISHED`.
    completed: bool,
    /// The section's `sys`. The first-call install sets its `model`.
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
    /// Constructs the frame for one walked section and runs its setup
    /// preamble: the section's `sys`, the section-started observation, VM
    /// construction and limits, and the shared setup half
    /// ([`section_vm::setup_section_vm`]), with the `prompt` reads resolved
    /// from the section.
    ///
    /// `pos` is the section's place in the caller's walk. Each `prompt`
    /// read resolves its path against the shared prompt: a relative path
    /// from `pos`, which names a sibling or a direct child, and a path
    /// from the top from the top-level sections. `require` takes only a
    /// path from the top, so it needs no position. `section_id`
    /// is the section's `sys.id`: the entering chain's hierarchical id extended
    /// by its local entry counter, allocated by the scheduler; `task_id`
    /// is the entering chain's `sys.taskid`. `var` is the walk's current
    /// `var` table, seeded into the section's VM. `seed` holds a spawned
    /// chain's `item` and `sys.index` on its first entry and is empty on
    /// every other entry. `scope` is the handle on the run's scope, which
    /// derives the `store` table's view of `access`.
    ///
    /// # Errors
    /// Returns an `Internal` failure when `pos` is past its slice, and
    /// otherwise the failure of whichever step failed (see [`section_vm`]
    /// for which failures tear the VM down).
    #[expect(
        clippy::too_many_arguments,
        reason = "the frame takes the two chain-owned handles (access and selection) and the run's scope handle beside the section entry's own deltas, and each stays a separate borrow"
    )]
    pub(crate) fn new(
        ctx: &ChainState,
        access: &Arc<Access>,
        scope: &ScopeHandle,
        selection: &Arc<Mutex<Option<ModelBinding>>>,
        pos: &SectionPos,
        section_id: &str,
        task_id: &TaskId,
        var: &serde_json::Value,
        seed: TaskSeed,
    ) -> Result<Self> {
        let section = pos.section(ctx.prompt()).ok_or(error::make_internal_error(
            "an entered section is absent from its slice",
        ))?;
        let mut sys = ctx.section_sys(section_id, task_id, section.name());
        // A spawned chain's `sys.index` is the spawn's own value, verbatim;
        // absent otherwise, so a walked section reading `sys.index` raises
        // the sealed-sys unknown-field error.
        sys.index = seed.index;
        ctx.emitter()
            .report(section.name(), event_lifecycle::SECTION_STARTED);
        let vm = limited_vm(ctx, section.name())?;
        let resolve = library_resolver(Arc::clone(ctx.prompt()));
        let render = prose_renderer(ctx, seed.item.clone());
        // The `prompt` reads resolve from the section; the suspending
        // calls (`call`, `tasks.fanout`, `models.infer`) are the yield
        // shims the setup half installs.
        let prompt = Arc::clone(ctx.prompt());
        let pos = pos.clone();
        let read =
            move |path: String| walk_target::read_section(&path, &prompt, Origin::Section(&pos));
        // The shared setup half ([`section_vm::setup_section_vm`]) follows;
        // only the seed, the `sys` extras, and the callbacks' position are
        // the walk's own.
        let sys_json = sys.to_json();
        let setup = ctx.vm_setup(
            &sys_json,
            VmSeed {
                var: Some(var),
                item: seed.item.as_ref(),
            },
            access,
            scope,
            selection,
            section.name(),
            false,
        );
        // Setup runs on the bare VM and tears it down itself on failure:
        // the frame does not exist yet, so its `Drop` cannot own this path.
        let vm = section_vm::setup_section_vm(vm, &setup, read, resolve, render)?;
        Ok(Self::assemble(vm, sys, seed.item, ctx))
    }

    /// Constructs the frame for the H1 pass - section 0 - through the same
    /// install path as any walked section: the section's `sys`
    /// (`section_id`, the root chain's entry 0, and `task_id`, the root
    /// chain's task, under the prompt's title, with the run's `when` like
    /// every section after it), VM construction over the run's shared
    /// sets, limits, and the shared setup half
    /// ([`section_vm::setup_section_vm`]).
    ///
    /// H1's only deltas from a walked section: no `SECTION_STARTED`
    /// observation, and no `SECTION_FINISHED` since the scheduler never arms
    /// the pass's completion (the pass is not a walked section), an empty
    /// `var` seed (it runs first and is never re-entered), `argv` installed
    /// writable so the repair pattern can assign it, and [`Origin::H1`]
    /// behind the `prompt` reads, so a relative path names a top-level
    /// section.
    ///
    /// # Errors
    /// Returns the failure of whichever step failed (see [`section_vm`]
    /// for which failures tear the VM down).
    pub(crate) fn new_h1(
        ctx: &ChainState,
        access: &Arc<Access>,
        scope: &ScopeHandle,
        selection: &Arc<Mutex<Option<ModelBinding>>>,
        section_id: &str,
        task_id: &TaskId,
    ) -> Result<Self> {
        let title = ctx.prompt().title();
        let sys = ctx.section_sys(section_id, task_id, title);
        let vm = limited_vm(ctx, title)?;
        let resolve = library_resolver(Arc::clone(ctx.prompt()));
        let render = prose_renderer(ctx, None);
        let prompt = Arc::clone(ctx.prompt());
        let read = move |path: String| walk_target::read_section(&path, &prompt, Origin::H1);
        // `argv` installs writable, the H1 delta `SectionVmSetup::argv`
        // describes; the executor reads the value back at the freeze (see
        // the scheduler's H1-to-walk handoff).
        let sys_json = sys.to_json();
        let setup = ctx.vm_setup(
            &sys_json,
            VmSeed::default(),
            access,
            scope,
            selection,
            title,
            true,
        );
        // Setup runs on the bare VM and tears it down itself on failure:
        // the frame does not exist yet, so its `Drop` cannot own this path.
        let vm = section_vm::setup_section_vm(vm, &setup, read, resolve, render)?;
        Ok(Self::assemble(vm, sys, None, ctx))
    }

    /// The live frame over a set-up `vm`: not yet completed, with no
    /// tool-call counts and no open adhoc call.
    fn assemble(
        vm: SectionVm,
        sys: SectionSys,
        item: Option<serde_json::Value>,
        ctx: &ChainState,
    ) -> Self {
        Self {
            vm: Some(vm),
            completed: false,
            sys,
            item,
            counts: None,
            adhoc_calls: Vec::new(),
            reporting: ctx.reporting().clone(),
        }
    }

    /// Reads the section's final `var` out of the VM, so the walk rolls it
    /// forward. Must run while the frame is live, before its drop: the read
    /// goes through the live VM.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure when the section reassigned the `var`
    /// global, which the walk relies on to fail the run, or when `var`
    /// cannot convert back to JSON (the write guard keeps that second case
    /// from happening in practice).
    pub(super) fn read_var(&self) -> Result<serde_json::Value> {
        self.vm().var()
    }

    /// Reads the H1 pass's `argv` back as JSON while the frame is live: the
    /// value the walk's sections inherit frozen - the derived parse, or H1's
    /// repair. `None` reads as nil.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure when H1 left `argv` as non-JSON data.
    pub(super) fn read_argv(&self) -> Result<Option<serde_json::Value>> {
        self.vm().argv_json()
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
    /// Infallible; [`live_vm`] states why.
    pub(super) fn vm(&self) -> &SectionVm {
        live_vm(self.vm.as_ref())
    }

    /// The section entry's id: the value its Lua reads as `sys.id`, which
    /// the walk assigned when it entered the section.
    pub(super) fn entry_id(&self) -> &str {
        &self.sys.id
    }

    /// Installs the scheduler's pending Markdown buffer, empty when no
    /// Markdown accumulated, as the VM's lazy `prose` template before one
    /// Lua coroutine starts, rendered through this frame's
    /// [`prose_renderer`] with `line`, the prompt line the buffer starts
    /// on; [`SectionVm::install_lazy_prose`] states when it renders.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure if the guard cannot be installed.
    pub(super) fn install_lazy_prose(
        &self,
        ctx: &ChainState,
        template: &str,
        line: Option<NonZeroU32>,
    ) -> Result<()> {
        let template = template.to_owned();
        let render = prose_renderer(ctx, self.item.clone());
        self.vm()
            .install_lazy_prose(move |state: ProseState<'_>| render(&template, line, state))
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
    /// Returns the failure of the first-call install or the
    /// id seeding.
    pub(super) fn tool_call_counts(&mut self, ids: &[ToolId]) -> Result<ToolCallCounts> {
        let Self {
            vm, sys, counts, ..
        } = self;
        let vm = live_vm(vm.as_ref());
        let counts = match counts {
            Some(counts) => counts,
            None => counts.insert(install_first_call_state(vm, sys, ids)?),
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

/// The live VM a frame's slot holds: `Some` from construction until the
/// frame's own [`Drop`] takes it for the teardown boundary, which is the
/// only place the slot clears. Every accessor reads the slot through here,
/// so the invariant is stated once.
#[expect(
    clippy::expect_used,
    reason = "the slot clears only in this frame's own Drop, so a live frame always holds its VM"
)]
fn live_vm(vm: Option<&SectionVm>) -> &SectionVm {
    vm.expect("a live frame holds its VM until drop")
}

/// The resolver behind a frame's `require`: each path resolves from the
/// top of `prompt`, so every frame's resolver is the same.
fn library_resolver(
    prompt: Arc<Prompt>,
) -> impl Fn(String) -> Result<Library> + Send + Sync + 'static {
    move |path: String| walk_target::resolve_library(&prompt, &path)
}

/// Builds the section VM for `name` over the run's shared sets, with the
/// run's cancel flag and Lua limits applied.
///
/// # Errors
/// Returns the failure of the VM construction or the limits install.
fn limited_vm(ctx: &ChainState, name: &str) -> Result<SectionVm<Uninjected>> {
    let vm = SectionVm::new_for_section(
        ctx.nonce(),
        ctx.tool_set(),
        &ctx.model_set(),
        ctx.emitter(),
        name,
    )?;
    // The run's cancel flag reaches every block coroutine's hook.
    vm.set_cancel(ctx.cancel().clone());
    // A limits failure propagates bare: no teardown runs here, so no
    // LUA_TEARDOWN_* observation fires on this path.
    vm.apply_lua_limits(
        ctx.limits().lua_memory_bytes.get(),
        ctx.limits().lua_log_events.get(),
    )?;
    Ok(vm)
}

/// The `{{ }}` renderer for one frame's prose templates, its own blocks'
/// and the libraries `require` loads alike: each render substitutes from
/// the run's `args` and `argv`, shared with the run rather than copied,
/// the frame's `item`, and the section state the first read snapshots,
/// and places a failure in the run's prompt file from the line the
/// template starts on.
fn prose_renderer(
    ctx: &ChainState,
    item: Option<serde_json::Value>,
) -> impl Fn(&str, Option<NonZeroU32>, ProseState<'_>) -> mlua::Result<String> + Send + Sync + 'static
{
    let raw_args = Arc::clone(ctx.args());
    let argv = ctx.argv().cloned();
    let prompt = Arc::clone(ctx.prompt());
    move |template: &str, line: Option<NonZeroU32>, state: ProseState<'_>| -> mlua::Result<String> {
        let sources = subst::Sources {
            args: &raw_args,
            argv: argv.as_deref(),
            item: item.as_ref(),
            var: &state.var,
            sys: &state.sys,
            globals: state.globals,
            file: prompt.file(),
            line,
        };
        subst::substitute(template, &sources).map_err(mlua::Error::external)
    }
}

/// Installs what a section's first tool call fixes: the counts behind
/// `tools.stats` and the current model (`sys.model`).
///
/// The counts install backs the Lua `tools.stats` table, seeded with
/// `ids`, and the model read takes the section's current model and
/// sets `sys.model` to its name. The caller stores the
/// returned counts, so its slot is set only once every step has
/// succeeded, and a failed install runs again on the section's next tool
/// call. `sys` changes only once the VM's re-seal succeeds.
///
/// # Errors
/// Returns the failure of the counts install, the model resolution, or
/// the `sys` re-seal.
fn install_first_call_state(
    vm: &SectionVm,
    sys: &mut SectionSys,
    ids: &[ToolId],
) -> Result<ToolCallCounts> {
    let counts = vm.install_tool_call_counts(ids)?;
    let resolved_model = vm.current_model()?;
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
        // once here. It uses `let-else` rather than `live_vm`, so the
        // destructor never panics.
        let Some(vm) = self.vm.take() else {
            return;
        };
        // The VM's teardown pair reports through the emitter's observer
        // seam, so it lands in the buffer ahead of the completion below.
        vm.teardown(self.reporting.emitter.as_ref(), &self.sys.section_name);
        if self.completed {
            self.reporting
                .emitter
                .report(&self.sys.section_name, event_lifecycle::SECTION_FINISHED);
        }
    }
}

#[cfg(test)]
mod tests {
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
            section_count: 3,
            index: None,
            model: None,
        }
    }

    #[test]
    fn sys_json_holds_the_five_shared_fields_with_a_string_task_id() {
        assert_eq!(
            sys().to_json(),
            json!({
                "when": "2026-10-26T18:00:00+00:00",
                "id": "0.2.1",
                "taskid": "0.2",
                "section_name": "Draft",
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
                "section_count": 3,
                "index": 4,
                "model": "qwen3",
            })
        );
    }
}
