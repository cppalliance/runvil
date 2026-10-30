//! Task admission: the slots a task chain takes at its owner and every
//! enclosing ancestor before it runs, and gives back while it waits.
//!
//! Admission bounds the run: every task chain takes a slot at its owner
//! and at every enclosing ancestor, held until the task ends - except
//! while it, or a `call` chain it is blocked on, is parked on a task
//! wait, which gives the slot back so its descendants can run - and a
//! chain admits at most its effective limit at once. Each chain's limit
//! is its [`Chain::concurrency`](super::Chain::concurrency). Queue
//! order is spawn order, with resumptions first. The task's start event
//! fires at admission, when the task first runs.
//!
//! [`Admission`] owns the admission state and enforces these rules. It
//! reads the chain tree only through [`ChainTree`], a read-only view of
//! the chain arena and the task arena, and writes neither: the scheduler
//! applies each admission it plans - the chain's parked state, the ready
//! queue, and the start event.

use std::collections::{HashMap, HashSet, VecDeque};
use std::num::NonZeroUsize;
use std::sync::Arc;

use runvil_types::event::Event;

use crate::lua::Answer;
use crate::{Error, Result};

use super::chain_record::{ChainArena, ChainKind, Parked};
use super::tasks::TaskArena;
use super::{ChainIndex, Scheduler};

/// A queued task chain's spawn record, held from spawn until its
/// admission: the input override and the spawning section the start event
/// reports. The `var` snapshot is not in the record: it moves out of the
/// yield into the chain's `var` slot at spawn, the start event reports a
/// copy of it at admission, and the chain's first section entry seeds from
/// the slot.
#[derive(Debug)]
pub(super) struct SpawnRecord {
    /// The `opts.input` override, reported in the start event.
    pub(super) input: Option<String>,
    /// The spawner's section at spawn, reported as the start event's
    /// section: a task admitted after its spawner moved on still reports
    /// where it was spawned, not where the spawner is now.
    pub(super) section: String,
}

/// One chain an admission pass admitted, for the scheduler to apply.
#[derive(Debug)]
enum Admit {
    /// A chain parked on a task wait took its slots back: it resumes with
    /// the answer its wake delivered.
    Resumed(ChainIndex),
    /// A queued task chain took its slots for the first time, with the
    /// spawn record its start event reports.
    Started(ChainIndex, SpawnRecord),
}

/// A read-only view of the chain tree, as admission reads it: each chain's
/// kind and limit from the chain arena, and each task chain's owner from
/// its task entry, so nothing mirrors the owner. Built inline from the
/// scheduler's two fields, so admission can be mutated while they're
/// borrowed.
#[derive(Clone, Copy)]
pub(super) struct ChainTree<'a> {
    /// The chain arena.
    pub(super) chains: &'a ChainArena,
    /// The task arena, which records each task chain's owner.
    pub(super) tasks: &'a TaskArena,
}

impl ChainTree<'_> {
    /// What kind of chain `id` is.
    fn kind(&self, id: ChainIndex) -> &ChainKind {
        &self.chains[id].kind
    }

    /// The most tasks `id` may have admitted at once.
    pub(super) fn limit(&self, id: ChainIndex) -> NonZeroUsize {
        self.chains[id].concurrency
    }

    /// The call parent blocked on `id`, when it is a `call` child.
    fn call_parent(&self, id: ChainIndex) -> Option<ChainIndex> {
        self.chains[id].kind.parent()
    }

    /// The chain enclosing `id` in the task tree: its owner, read from its
    /// task entry, for a task chain, its call parent for a call chain, and
    /// `None` for the root walk and the H1 pass.
    pub(super) fn enclosing(&self, id: ChainIndex) -> Option<ChainIndex> {
        let chain = &self.chains[id];
        match chain.kind {
            ChainKind::Task => self.tasks.owner_of(&chain.task),
            ChainKind::Call { parent } => Some(parent),
            ChainKind::Root | ChainKind::H1 => None,
        }
    }

    /// The chains a slot the task chain `id` holds counts against: its
    /// owner and every enclosing ancestor.
    fn ancestors(&self, id: ChainIndex) -> impl Iterator<Item = ChainIndex> + '_ {
        std::iter::successors(self.enclosing(id), |at| self.enclosing(*at))
    }
}

/// The run's admission state: the chains waiting for slots and the chains
/// holding them.
///
/// Slot counts are computed from the holders, never stored, so no count
/// can drift from them.
#[derive(Debug, Default)]
pub(super) struct Admission {
    /// Chains parked on a task wait that gave their admission slots back,
    /// waiting to take them again: a task chain, or a call chain that gave
    /// back its nearest holding ancestor's slots. Each keeps its Lua VM and
    /// holds its delivered answer. Admitted ahead of every fresh spawn.
    resuming: VecDeque<ChainIndex>,
    /// Freshly spawned task chains waiting for admission, in spawn order,
    /// each with its spawn record; admitted after every resuming chain. A
    /// queued spawn holds no Lua VM yet: admission is what starts it.
    spawned: VecDeque<(ChainIndex, SpawnRecord)>,
    /// The task chains holding their admission slots now: from admission
    /// until they end or park on a task wait. A task blocked on a `call`
    /// child gives them back while that child parks on a task wait.
    holding: HashSet<ChainIndex>,
    /// Each call chain parked on a task wait that gave back its nearest
    /// holding ancestor's slots, with that ancestor: the map of chains that
    /// gave slots back, not of the slots, so the chain's wake takes the
    /// ancestor's slots back before the chain continues.
    gave_back: HashMap<ChainIndex, ChainIndex>,
}

impl Admission {
    /// Queues the freshly spawned task chain `id` with its spawn record.
    pub(super) fn enqueue_spawn(&mut self, id: ChainIndex, record: SpawnRecord) {
        self.spawned.push_back((id, record));
    }

    /// Whether `id` is a spawn waiting for its first admission. A task
    /// chain that isn't queued has been admitted, so its start event has
    /// fired and its terminal event may fire too.
    pub(super) fn is_queued(&self, id: ChainIndex) -> bool {
        self.spawned.iter().any(|(queued, _)| *queued == id)
    }

    /// Plans one admission pass over every queued chain that can take a
    /// slot now: resumptions first (a task resumed from a join takes its
    /// slots back ahead of tasks that have not started, so a resume cannot
    /// starve behind a long queue), then fresh spawns in spawn order. The
    /// slots in use are counted once per pass, and each admission adds its
    /// own. Every admitted chain's slot holder holds its slots from here;
    /// the scheduler applies the rest. This is where both ordering and
    /// counting live, which is what makes an admission count against the
    /// next admission in the same pass.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when a resumed chain's slots belong to
    /// no admitted task, or a queued spawn is not a task chain. A failed
    /// pass leaves admission as it found it: both queues, the holders, and
    /// the given-back slots.
    fn plan(&mut self, tree: &ChainTree<'_>) -> Result<Vec<Admit>> {
        let mut admitted = Vec::new();
        if self.resuming.is_empty() && self.spawned.is_empty() {
            return Ok(admitted);
        }
        let mut holding = self.holding.clone();
        let mut gave_back = self.gave_back.clone();
        let mut used = slots_in_use(&holding, tree);
        let mut resuming = VecDeque::new();
        for &id in &self.resuming {
            let holder = slot_holder(&gave_back, id);
            if !can_admit(&used, tree, holder) {
                resuming.push_back(id);
                continue;
            }
            if !matches!(tree.kind(holder), ChainKind::Task) || self.is_queued(holder) {
                return Err(Error::internal(
                    "a resumed chain's slots belong to an admitted task",
                ));
            }
            holding.insert(holder);
            gave_back.remove(&id);
            count_slot(&mut used, tree, holder);
            admitted.push(Admit::Resumed(id));
        }
        let mut starts = Vec::with_capacity(self.spawned.len());
        for &(id, _) in &self.spawned {
            if !can_admit(&used, tree, id) {
                starts.push(false);
                continue;
            }
            if !matches!(tree.kind(id), ChainKind::Task) {
                return Err(Error::internal("a queued chain backs a task"));
            }
            holding.insert(id);
            count_slot(&mut used, tree, id);
            starts.push(true);
        }
        let mut spawned = VecDeque::new();
        for ((id, record), start) in std::mem::take(&mut self.spawned).into_iter().zip(starts) {
            if start {
                admitted.push(Admit::Started(id, record));
            } else {
                spawned.push_back((id, record));
            }
        }
        self.resuming = resuming;
        self.spawned = spawned;
        self.holding = holding;
        self.gave_back = gave_back;
        Ok(admitted)
    }

    /// Parks `id` on a task wait. A wait that holds real tasks gives
    /// admission slots back - a timer-only wait keeps them, since nothing
    /// it waits for needs a slot - so the chain's descendants can run
    /// under its limit. A task chain gives back its own. A call chain
    /// holds no slots of its own, so it gives back those of its nearest
    /// holding ancestor through its call parents (the task it runs inside,
    /// blocked on the call) and records which chain gave them. A call
    /// chain in the main walk finds no holding ancestor, and the main walk
    /// holds no slots to give.
    pub(super) fn park(&mut self, id: ChainIndex, tree: &ChainTree<'_>, waits_on_tasks: bool) {
        if !waits_on_tasks {
            return;
        }
        if self.holding.remove(&id) {
            return;
        }
        let mut at = tree.call_parent(id);
        while let Some(ancestor) = at {
            if self.holding.remove(&ancestor) {
                self.gave_back.insert(id, ancestor);
                return;
            }
            at = tree.call_parent(ancestor);
        }
    }

    /// Wakes `id` from a task wait, and reports whether it was queued to
    /// take its slots back: a task chain that gave its slots back, and a
    /// call chain that gave back its ancestor's, are admitted again ahead
    /// of fresh starts, while the main walk and a chain that kept its
    /// slots (a timer-only wait) resume at once.
    fn wake(&mut self, id: ChainIndex, tree: &ChainTree<'_>) -> bool {
        let requeue = match tree.kind(id) {
            ChainKind::Call { .. } => self.gave_back.contains_key(&id),
            ChainKind::Task => !self.holding.contains(&id),
            ChainKind::Root | ChainKind::H1 => false,
        };
        if requeue {
            self.resuming.push_back(id);
        }
        requeue
    }

    /// Releases `id`'s slots at its chain end, so a queued sibling or a
    /// resumption can have them. A chain that holds none, or whose slots
    /// are given back while it is parked, changes nothing.
    pub(super) fn release(&mut self, id: ChainIndex) {
        self.holding.remove(&id);
    }

    /// Drops an aborted chain: it leaves both queues - no admission can
    /// start it, and no wake can requeue it - and the slots it still holds
    /// are released. The chain's own `gave_back` record goes too, since
    /// no resumption will read it.
    pub(super) fn drop_aborted(&mut self, id: ChainIndex) {
        self.resuming.retain(|queued| *queued != id);
        self.spawned.retain(|(queued, _)| *queued != id);
        self.holding.remove(&id);
        self.gave_back.remove(&id);
    }

    /// Drops every queued chain and every held slot: the run's teardown.
    pub(super) fn clear(&mut self) {
        self.resuming.clear();
        self.spawned.clear();
        self.holding.clear();
        self.gave_back.clear();
    }

    /// Marks the task chain `id` holding its slots, so a test can fill a
    /// chain's slots with tasks that never run.
    #[cfg(test)]
    pub(super) fn hold_for_test(&mut self, id: ChainIndex) {
        self.holding.insert(id);
    }
}

/// The chain whose slots `id`'s resumption takes back: the holding
/// ancestor a parked call chain gave back, as `gave_back` records it, or
/// the resumed task itself.
fn slot_holder(gave_back: &HashMap<ChainIndex, ChainIndex>, id: ChainIndex) -> ChainIndex {
    gave_back.get(&id).copied().unwrap_or(id)
}

/// The admission slots in use at each chain, by arena index, counted
/// from the task chains in `holding`: a chain's count is its own
/// children's and, transitively, every descendant's.
fn slots_in_use(holding: &HashSet<ChainIndex>, tree: &ChainTree<'_>) -> Vec<usize> {
    let mut used = vec![0; tree.chains.len()];
    for holder in holding {
        count_slot(&mut used, tree, *holder);
    }
    used
}

/// Counts one slot the task chain `id` holds into `used`, at its owner
/// and every enclosing ancestor.
fn count_slot(used: &mut [usize], tree: &ChainTree<'_>, id: ChainIndex) {
    for ancestor in tree.ancestors(id) {
        used[ancestor.index()] += 1;
    }
}

/// Whether `id` can take its slots with the slots `used`: a free slot at
/// its owner and at every enclosing ancestor, so the task counts against
/// each chain's limit on the way up.
fn can_admit(used: &[usize], tree: &ChainTree<'_>, id: ChainIndex) -> bool {
    tree.ancestors(id)
        .all(|ancestor| used[ancestor.index()] < tree.limit(ancestor).get())
}

impl Scheduler {
    /// Admits every queued task that can take a slot now, applying each
    /// admission [`Admission::plan`] made, in its order.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the plan does, or when a started
    /// task has no entry.
    pub(super) fn admit(&mut self) -> Result<()> {
        let tree = ChainTree {
            chains: &self.chains,
            tasks: &self.tasks,
        };
        for admit in self.admission.plan(&tree)? {
            match admit {
                Admit::Resumed(id) => {
                    self.chains[id].parked = None;
                    self.ready.push_back(id);
                }
                Admit::Started(id, record) => self.admit_chain(id, record)?,
            }
        }
        Ok(())
    }

    /// Starts one admitted task chain: enqueues it and fires its start
    /// event with the spawn record on its owner's sequence (its spawner, or
    /// the root walk for a task the H1 pass spawned) under the spawning
    /// section (the spawn-time capture, so a task admitted after its
    /// spawner moved on still reports where it was spawned).
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when its task has no entry: a spawn
    /// registers the entry before it queues the chain.
    fn admit_chain(&mut self, id: ChainIndex, record: SpawnRecord) -> Result<()> {
        let chain = &mut self.chains[id];
        chain.parked = None;
        let task = chain.task.clone();
        let item = chain.seed.item.clone();
        let index = chain.seed.index;
        let var = chain.var.clone();
        let (Some(owner), Some(target)) = (self.tasks.owner_of(&task), self.tasks.target_of(&task))
        else {
            return Err(Error::internal("a task chain has an entry"));
        };
        let target = target.to_owned();
        let emitter = Arc::clone(self.chains[owner].ctx.emitter());
        // The start event fires at admission, on its owner's sequence:
        // the task first runs now, and the payload's seeds are enough to
        // start the same chain again under the same id.
        emitter.emit(&record.section, |execution, section, provenance| {
            Event::TaskStarted {
                execution,
                section,
                provenance,
                task,
                target,
                input: record.input,
                item,
                index,
                var,
            }
        });
        self.ready.push_back(id);
        Ok(())
    }

    /// Delivers its answer to a chain parked on a task wait: the chain is
    /// parked on [`Parked::Queued`] when [`Admission::wake`] requeues it for
    /// its slots, else pushed onto the ready queue at once.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the chain has no suspended block
    /// to take the answer, or one that already holds an answer.
    pub(super) fn wake_from_wait(
        &mut self,
        id: ChainIndex,
        answer: Result<Answer<Error>>,
    ) -> Result<()> {
        self.chains[id].deliver(answer)?;
        let tree = ChainTree {
            chains: &self.chains,
            tasks: &self.tasks,
        };
        if self.admission.wake(id, &tree) {
            self.chains[id].parked = Some(Parked::Queued);
        } else {
            self.ready.push_back(id);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "admission-tests.rs"]
mod tests;
