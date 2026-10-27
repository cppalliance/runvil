//! The frame's two constructors: one per arrival kind. Each absorbs the VM
//! construction and setup preamble for its kind of section entry - a
//! walked section (a spawned task's first entry included, seeded with its
//! `item` and `sys.index`), the H1 pass (section 0) - and hands back
//! a live [`SectionContext`] whose `Drop` is the teardown boundary. The
//! setup half ([`section_vm::setup_section_vm`]) is shared; only the seed,
//! the `sys` extras, the visible set behind the `prompt` reads, and the
//! origin `require` resolves from differ.

use std::sync::Arc;

use runvil_types::event::lifecycle;
use runvil_types::ids::TaskId;
use runvil_vfs::Access;

use crate::execute::context::RunState;
use crate::execute::section_vm::{self, VmSeed};
use crate::execute::walk_target::{self, RequireOrigin};
use crate::heading_address::{SectionPos, Visible};
use crate::lua::{Library, LibraryKey, SectionVm};
use crate::parser::Prompt;
use crate::{Error, Result};

use super::{SectionContext, TaskSeed};

/// The resolver behind a frame's `require`: each heading resolves in
/// `prompt` from the position a loading library's key names, or from
/// `origin` when no library is loading.
fn library_resolver(
    prompt: Arc<Prompt>,
    origin: RequireOrigin,
) -> impl Fn(String, Option<LibraryKey>) -> Result<Library> + Send + Sync + 'static {
    move |heading: String, key: Option<LibraryKey>| {
        walk_target::resolve_library(&prompt, &origin, &heading, key.as_ref())
    }
}

impl SectionContext {
    /// Constructs the frame for one walked section and runs its setup
    /// preamble: the section's `sys`, the section-started observation, VM
    /// construction and limits, and the shared setup half
    /// ([`section_vm::setup_section_vm`]), with the `prompt` reads resolved
    /// over the section's visible set.
    ///
    /// `pos` is the section's place in the caller's walk. Each `prompt`
    /// read resolves its sibling slice from the shared prompt and reads
    /// the section's visible set (its siblings minus itself, plus its
    /// direct children) in place, and `require` resolves from the same
    /// position over its wider reach. `section_id`
    /// is the section's `sys.id`: the entering chain's hierarchical id extended
    /// by its local entry counter, allocated by the scheduler; `task_id`
    /// is the entering chain's `sys.taskid`. `var` is the walk's current
    /// `var` table, seeded into the section's VM. `seed` holds a spawned
    /// chain's `item` and `sys.index` on its first entry and is empty on
    /// every other entry.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when `pos` is past its slice, and
    /// otherwise the [`Error`] of whichever step failed. A VM
    /// construction or limits failure propagates bare, before any teardown
    /// observation exists; a setup failure tears the fresh VM down first, so
    /// the teardown boundary still fires exactly once on that path.
    pub(crate) fn new(
        ctx: &RunState,
        access: &Arc<Access>,
        pos: &SectionPos,
        section_id: &str,
        task_id: &TaskId,
        var: &serde_json::Value,
        seed: TaskSeed,
    ) -> Result<Self> {
        let section = pos.section(ctx.prompt()).ok_or(Error::internal(
            "an entered section is absent from its slice",
        ))?;
        let mut sys = ctx.section_sys(section_id, task_id, section.name());
        // A spawned chain's `sys.index` is the spawn's own value, verbatim;
        // absent otherwise, so a walked section reading `sys.index` raises
        // the sealed-sys unknown-field error.
        sys.index = seed.index;
        ctx.emitter()
            .report(section.name(), lifecycle::SECTION_STARTED);
        let vm = SectionVm::new_for_section(
            ctx.nonce(),
            ctx.tool_set(),
            &ctx.model_set(),
            ctx.emitter(),
            section.name(),
        )?;
        // The run's cancel flag reaches every block coroutine's hook.
        vm.set_cancel(ctx.cancel().clone());
        // A limits failure propagates bare: no teardown runs here, so no
        // LUA_TEARDOWN_* observation fires on this path.
        vm.apply_lua_limits(
            ctx.limits().lua_memory_bytes.get(),
            ctx.limits().lua_log_events.get(),
        )?;
        let resolve = library_resolver(
            Arc::clone(ctx.prompt()),
            RequireOrigin::Section(pos.clone()),
        );
        let render = super::prose_renderer(ctx, seed.item.clone());
        // The `prompt` reads resolve over the section's visible set; the
        // suspending calls (`call`, `tasks.fanout`, `models.infer`) are the
        // yield shims the setup half installs.
        let prompt = Arc::clone(ctx.prompt());
        let pos = pos.clone();
        let read = move |heading: String| {
            walk_target::read_visible(&heading, Visible::of(pos.siblings(&prompt), pos.index))
        };
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
            section.name(),
        );
        // Setup runs on the bare VM and tears it down itself on failure:
        // the frame does not exist yet, so its `Drop` cannot own this path.
        let vm = section_vm::setup_section_vm(vm, &setup, read, resolve, render)?;
        Ok(Self {
            vm: Some(vm),
            completed: false,
            sys,
            item: seed.item,
            counts: None,
            adhoc_calls: Vec::new(),
            reporting: ctx.reporting().clone(),
        })
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
    /// observation (the pass is not a walked section), an empty `var` seed
    /// (it runs first and is never re-entered), `argv` installed writable
    /// so the repair pattern can assign it, [`walk_target::h1_visible_sections`]
    /// as the visible set behind the `prompt` reads, and
    /// [`RequireOrigin::H1`], which reaches every top-level section, behind
    /// `require`.
    ///
    /// # Errors
    /// Returns the [`Error`] of whichever step failed. A VM
    /// construction or limits failure propagates bare, before any teardown
    /// observation exists; a setup failure tears the fresh VM down first, so
    /// the teardown boundary still fires exactly once on that path.
    pub(crate) fn new_h1(
        ctx: &RunState,
        access: &Arc<Access>,
        section_id: &str,
        task_id: &TaskId,
    ) -> Result<Self> {
        let title = ctx.prompt().title();
        let sys = ctx.section_sys(section_id, task_id, title);
        let vm = SectionVm::new_for_section(
            ctx.nonce(),
            ctx.tool_set(),
            &ctx.model_set(),
            ctx.emitter(),
            title,
        )?;
        vm.set_cancel(ctx.cancel().clone());
        // A limits failure propagates bare: no teardown runs here, so no
        // LUA_TEARDOWN_* observation fires on this path.
        vm.apply_lua_limits(
            ctx.limits().lua_memory_bytes.get(),
            ctx.limits().lua_log_events.get(),
        )?;
        let resolve = library_resolver(Arc::clone(ctx.prompt()), RequireOrigin::H1);
        let render = super::prose_renderer(ctx, None);
        let prompt = Arc::clone(ctx.prompt());
        let read = move |heading: String| {
            walk_target::read_visible(
                &heading,
                Visible::all(walk_target::h1_visible_sections(&prompt)),
            )
        };
        // H1's one privilege: `argv` installs writable, so the repair
        // pattern can assign it; the executor reads the value back at the
        // freeze (see the scheduler's H1-to-walk handoff).
        let sys_json = sys.to_json();
        let mut setup = ctx.vm_setup(&sys_json, VmSeed::default(), access, title);
        setup.argv_writable = true;
        // Setup runs on the bare VM and tears it down itself on failure:
        // the frame does not exist yet, so its `Drop` cannot own this path.
        let vm = section_vm::setup_section_vm(vm, &setup, read, resolve, render)?;
        Ok(Self {
            vm: Some(vm),
            completed: false,
            sys,
            item: None,
            counts: None,
            adhoc_calls: Vec::new(),
            reporting: ctx.reporting().clone(),
        })
    }
}
