//! The `spawn` arm. A task is a chain the scheduler runs beside its
//! spawner instead of in place of it: `tasks.spawn` (and the `fanout`
//! shim, once per arm) starts the chain over the target's slice - under
//! `call`'s target resolution and depth cap, refusing a list section as
//! the target - registers an entry for it keyed by the chain's own
//! hierarchical id, and resumes the spawner at once with the id. The
//! spawner runs first; the child waits in the admission queue, holding no
//! Lua VM, until the run's concurrency limits admit it.

use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use runvil_types::ids::TaskId;
use runvil_vfs::detail;

use crate::error::DepthVia;
use crate::execute::section_context::TaskSeed;
use crate::lua::{Answer, SpawnVia};
use crate::{Error, Result};

use super::super::admission::SpawnRecord;
use super::super::chain::ChainStart;
use super::super::chain_record::{ChainKind, Parked};
use super::super::{ChainIndex, Counters, Scheduler, prompt_origin};
use super::TaskBacking;

impl Scheduler {
    /// Dispatches a `spawn` request: constructs the task's chain, registers
    /// its entry, and resumes the spawner with the task's id; the child is
    /// enqueued behind the spawner, so `spawn` returns before the child
    /// runs. Its preparation fails on the depth cap, target resolution,
    /// the worker check, or chain construction. `via` names the call that
    /// started the task: a `fanout` arm's depth-cap refusal is named after
    /// `tasks.fanout`.
    pub(in crate::execute::scheduler) fn dispatch_spawn(
        &mut self,
        id: ChainIndex,
        target: &str,
        input: Option<&str>,
        seed: TaskSeed,
        var: serde_json::Value,
        via: SpawnVia,
    ) -> Result<()> {
        match self.prepare_spawn(id, target, input, seed, var, via) {
            Ok((task, child, record)) => {
                // The child waits for admission, queued behind the
                // spawner; the drain admits it when a slot frees up at
                // its owner and every ancestor.
                self.admission.enqueue_spawn(child, record);
                self.answer_inline(id, Ok(Answer::Spawn(task)))
            }
            Err(error) => self.answer_inline(id, Err(error)),
        }
    }

    /// The fallible half of spawn dispatch: `call`'s depth cap against the
    /// spawner's call-depth field (the refusal named `tasks.fanout` for an
    /// arm, and `call` for a `call` or a `tasks.spawn`), `call`'s target
    /// resolution over the spawner's
    /// visible set, the worker-template check (a list section is not a
    /// target), then the task chain one level deeper under the spawn's
    /// `args` and `var` snapshot, with its own access capability (a
    /// concurrent thread of execution, forked from the spawner's so the
    /// spawn is the happens-before edge; every delivery of the task joins
    /// it back) and a fresh turn counter. The child leaves this call
    /// holding no Lua VM yet, with its `var` snapshot moved into the chain,
    /// and its spawn record comes back for the admission queue; the drain
    /// admits it when an admission slot frees up.
    fn prepare_spawn(
        &mut self,
        id: ChainIndex,
        target: &str,
        input: Option<&str>,
        seed: TaskSeed,
        var: serde_json::Value,
        via: SpawnVia,
    ) -> Result<(TaskId, ChainIndex, SpawnRecord)> {
        let chain = &self.chains[id];
        let depth = chain.call_depth.deeper(match via {
            SpawnVia::Fanout => DepthVia::Fanout,
            SpawnVia::Spawn => DepthVia::Call,
        })?;
        // The spawning section, captured now: the start event reports it
        // at admission, which may come after the spawner's walk moved to
        // another section.
        let spawner_section = chain.section_name().to_owned();
        // An explicit input forks the chain's args (and `argv` re-derives
        // from them), as a `call` with input does; otherwise the chain
        // inherits the spawner's context whole, but for the turn counter,
        // which `with_task` replaces below.
        let child_ctx = match input {
            Some(input) => chain.ctx.with_args(input),
            None => chain.ctx.clone(),
        };
        // A spawned chain's effective limit starts with its spawner's:
        // its tasks run within the spawner's share and every ancestor's.
        let child_concurrency = chain.concurrency;
        let spawner_access = Arc::clone(chain.access()?);
        // `chain`'s arena borrow ends here; the resolution names the
        // target's slice by path, resolved against the shared tree.
        let prompt = self.prompt();
        let resolved = self.with_chain(id, |ops| ops.resolve_chain_target(target))?;
        let worker = resolved.pos.section(&prompt).ok_or(Error::internal(
            "a resolved worker is absent from its slice",
        ))?;
        if worker.prologue().is_none() && worker.epilog().is_none() && !worker.items().is_empty() {
            return Err(Error::lua_message(format!(
                "section `{}` is a list section, not a worker template",
                worker.name()
            )));
        }
        // The access spawns before the chain exists: a store refusal here
        // is the last fallible step that can leave nothing behind, so it
        // runs ahead of the id allocation and the arena push rather than
        // orphaning a started chain that is neither enqueued nor entered
        // in the arena.
        let access = detail::access_spawn(
            &spawner_access,
            prompt_origin(&prompt, worker.name(), worker.blocks()),
        )
        .map_err(Error::store)?;
        // The task's happens-before identity, recorded before the access
        // moves into the chain: every delivery joins it, and the chain
        // end joins it last.
        let exec = detail::access_id(&access);
        // The task's id is the spawner's next child id; the task is its
        // chain, named from the other side.
        let lineage = self.chains[id].next_child_id()?;
        let task = TaskId::from(lineage.clone());
        // The task's context reports under its own task id with its own
        // turn counter, so its events report its provenance and its turn
        // numbers, and `tasks.status`'s `turns`, count only its own rounds.
        let child_ctx = child_ctx.with_task(task.clone(), Arc::new(AtomicU32::new(0)));
        let child = self.start_chain(ChainStart {
            lineage,
            counters: Counters::default(),
            ctx: child_ctx,
            access: Arc::new(access),
            start: resolved.pos,
            kind: ChainKind::Task,
            var,
            call_depth: depth,
            concurrency: child_concurrency,
        })?;
        let spawned = &mut self.chains[child];
        spawned.seed = seed;
        // `blocked` reads `queued` in `tasks.status` while the entry's
        // state is still `Running`.
        spawned.parked = Some(Parked::Queued);
        self.tasks.insert(
            task.clone(),
            TaskBacking::Chain { chain: child, exec },
            id,
            worker.name().to_owned(),
        );
        // The task waits for admission from here, and the spawn record
        // holds what the start event reports once the task first runs.
        let record = SpawnRecord {
            input: input.map(str::to_owned),
            section: spawner_section,
        };
        Ok((task, child, record))
    }
}
