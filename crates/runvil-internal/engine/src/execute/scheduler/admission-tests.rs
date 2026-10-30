//! Tests for admission alone over a small chain tree, one rule each:
//! resumed chains are admitted before fresh spawns, one pass counts each
//! admission against the next, a task holds a slot at its owner and every
//! ancestor, a task wait gives those slots back and a timer-only wait keeps
//! them, a call chain gives them back through its nearest holding ancestor,
//! an aborted chain leaves admission, and a pass that fails leaves
//! admission as it found it.

use std::num::NonZeroUsize;

use runvil_types::ids::ChainId;
use runvil_vfs::{Origin, VfsRef};

use super::super::chain_record::{Chain, ChainArena, ChainIndex, ChainKind};
use super::super::tasks::{TaskArena, TaskBacking};
use super::{Admission, Admit, ChainTree, SpawnRecord};
use crate::Error;

/// The arenas a [`ChainTree`] reads, built up one chain at a time.
struct Tree {
    chains: ChainArena,
    tasks: TaskArena,
}

impl Tree {
    fn new() -> Tree {
        Tree {
            chains: ChainArena::new(),
            tasks: TaskArena::default(),
        }
    }

    fn view(&self) -> ChainTree<'_> {
        ChainTree {
            chains: &self.chains,
            tasks: &self.tasks,
        }
    }

    /// Pushes a chain of `kind` that admits at most `limit` tasks at once.
    fn push(&mut self, kind: ChainKind, limit: usize) -> ChainIndex {
        let index = u32::try_from(self.chains.len()).expect("the test tree is small");
        let limit = NonZeroUsize::new(limit).expect("a limit is positive");
        let chain = Chain::for_test(ChainId::root().child(index), kind, limit);
        self.chains.push(chain).expect("the arena has room")
    }

    fn root(&mut self, limit: usize) -> ChainIndex {
        self.push(ChainKind::Root, limit)
    }

    /// Pushes a task chain that `owner` owns, with its entry.
    fn task(&mut self, owner: ChainIndex, limit: usize) -> ChainIndex {
        let id = self.push(ChainKind::Task, limit);
        let access = VfsRef::default()
            .acquire(Origin::new("admission-test"))
            .expect("the default backend acquires");
        let backing = TaskBacking::Chain {
            chain: id,
            exec: runvil_vfs::detail::access_id(&access),
        };
        let task = self.chains[id].task.clone();
        self.tasks.insert(task, backing, owner, "worker".to_owned());
        id
    }

    /// Pushes a `call` child that `parent` is blocked on.
    fn call(&mut self, parent: ChainIndex) -> ChainIndex {
        self.push(ChainKind::Call { parent }, 1)
    }
}

/// Spawns a task `owner` owns and queues it for admission.
fn spawn(admission: &mut Admission, tree: &mut Tree, owner: ChainIndex) -> ChainIndex {
    let id = tree.task(owner, 1);
    let record = SpawnRecord {
        input: None,
        section: format!("spawned {}", id.index()),
    };
    admission.enqueue_spawn(id, record);
    id
}

/// What one admission pass admitted, in order.
#[derive(Debug, PartialEq, Eq)]
enum Admitted {
    Resumed(ChainIndex),
    Started(ChainIndex),
}

fn plan(admission: &mut Admission, tree: &Tree) -> Vec<Admitted> {
    admission
        .plan(&tree.view())
        .expect("every queued chain is a task")
        .into_iter()
        .map(|admit| match admit {
            Admit::Resumed(id) => Admitted::Resumed(id),
            Admit::Started(id, record) => {
                assert_eq!(
                    record.section,
                    format!("spawned {}", id.index()),
                    "a start carries its own spawn record"
                );
                Admitted::Started(id)
            }
        })
        .collect()
}

#[test]
fn resumed_chains_are_admitted_before_fresh_spawns() {
    let mut tree = Tree::new();
    let mut admission = Admission::default();
    let root = tree.root(1);
    let waiter = spawn(&mut admission, &mut tree, root);
    assert_eq!(plan(&mut admission, &tree), [Admitted::Started(waiter)]);
    admission.park(waiter, &tree.view(), true);
    let fresh = spawn(&mut admission, &mut tree, root);
    assert!(admission.wake(waiter, &tree.view()));
    assert_eq!(
        plan(&mut admission, &tree),
        [Admitted::Resumed(waiter)],
        "the resumption takes the one slot though the spawn queued first"
    );
    assert!(admission.is_queued(fresh));
}

#[test]
fn one_pass_counts_each_admission_against_the_next() {
    let mut tree = Tree::new();
    let mut admission = Admission::default();
    let root = tree.root(1);
    let first = spawn(&mut admission, &mut tree, root);
    let second = spawn(&mut admission, &mut tree, root);
    assert_eq!(plan(&mut admission, &tree), [Admitted::Started(first)]);
    assert!(!admission.is_queued(first));
    assert!(admission.is_queued(second));
}

#[test]
fn a_task_holds_a_slot_at_its_owner_and_every_ancestor() {
    let mut tree = Tree::new();
    let mut admission = Admission::default();
    let root = tree.root(2);
    let child = spawn(&mut admission, &mut tree, root);
    let grandchild = spawn(&mut admission, &mut tree, child);
    assert_eq!(
        plan(&mut admission, &tree),
        [Admitted::Started(child), Admitted::Started(grandchild)]
    );
    let sibling = spawn(&mut admission, &mut tree, root);
    assert_eq!(
        plan(&mut admission, &tree),
        [],
        "the grandchild's slot counts against the root"
    );
    admission.release(grandchild);
    assert_eq!(plan(&mut admission, &tree), [Admitted::Started(sibling)]);
}

#[test]
fn a_task_wait_gives_its_slots_back_until_the_wake_takes_them_again() {
    let mut tree = Tree::new();
    let mut admission = Admission::default();
    let root = tree.root(1);
    let waiter = spawn(&mut admission, &mut tree, root);
    assert_eq!(plan(&mut admission, &tree), [Admitted::Started(waiter)]);
    let queued = spawn(&mut admission, &mut tree, root);
    assert_eq!(plan(&mut admission, &tree), []);
    admission.park(waiter, &tree.view(), true);
    assert_eq!(plan(&mut admission, &tree), [Admitted::Started(queued)]);
    assert!(
        admission.wake(waiter, &tree.view()),
        "a task that gave its slots back waits to take them again"
    );
    assert_eq!(
        plan(&mut admission, &tree),
        [],
        "the slot is the queued task's now"
    );
    admission.release(queued);
    assert_eq!(plan(&mut admission, &tree), [Admitted::Resumed(waiter)]);
}

#[test]
fn a_timer_only_wait_keeps_its_slots_and_its_wake_resumes_inline() {
    let mut tree = Tree::new();
    let mut admission = Admission::default();
    let root = tree.root(1);
    let waiter = spawn(&mut admission, &mut tree, root);
    assert_eq!(plan(&mut admission, &tree), [Admitted::Started(waiter)]);
    spawn(&mut admission, &mut tree, root);
    admission.park(waiter, &tree.view(), false);
    assert_eq!(plan(&mut admission, &tree), []);
    assert!(!admission.wake(waiter, &tree.view()));
}

#[test]
fn a_call_chain_gives_slots_back_through_its_nearest_holding_ancestor() {
    let mut tree = Tree::new();
    let mut admission = Admission::default();
    let root = tree.root(1);
    let task = spawn(&mut admission, &mut tree, root);
    assert_eq!(plan(&mut admission, &tree), [Admitted::Started(task)]);
    let call = tree.call(task);
    let nested = tree.call(call);
    let queued = spawn(&mut admission, &mut tree, root);
    admission.park(nested, &tree.view(), true);
    assert_eq!(
        plan(&mut admission, &tree),
        [Admitted::Started(queued)],
        "the task the nested call runs inside gave its slot back"
    );
    assert!(admission.wake(nested, &tree.view()));
    admission.release(queued);
    assert_eq!(plan(&mut admission, &tree), [Admitted::Resumed(nested)]);
    spawn(&mut admission, &mut tree, root);
    assert_eq!(
        plan(&mut admission, &tree),
        [],
        "the resumption took the task's slot back"
    );
}

#[test]
fn a_call_chain_in_the_main_walk_has_no_slots_to_give_back() {
    let mut tree = Tree::new();
    let mut admission = Admission::default();
    let root = tree.root(1);
    let call = tree.call(root);
    admission.park(call, &tree.view(), true);
    assert!(!admission.wake(call, &tree.view()));
}

#[test]
fn an_aborted_chain_leaves_its_queue_and_its_slots() {
    let mut tree = Tree::new();
    let mut admission = Admission::default();
    let root = tree.root(1);
    let running = spawn(&mut admission, &mut tree, root);
    assert_eq!(plan(&mut admission, &tree), [Admitted::Started(running)]);
    let aborted = spawn(&mut admission, &mut tree, root);
    admission.drop_aborted(aborted);
    assert!(!admission.is_queued(aborted));
    admission.drop_aborted(running);
    let next = spawn(&mut admission, &mut tree, root);
    assert_eq!(plan(&mut admission, &tree), [Admitted::Started(next)]);
}

#[test]
fn a_resumption_whose_slots_belong_to_no_admitted_task_is_an_internal_error() {
    let mut tree = Tree::new();
    let mut admission = Admission::default();
    let root = tree.root(1);
    let queued = spawn(&mut admission, &mut tree, root);
    assert!(admission.wake(queued, &tree.view()));
    assert!(matches!(
        admission.plan(&tree.view()),
        Err(Error::Internal { .. })
    ));
    assert!(admission.resuming.contains(&queued));
}

#[test]
fn a_pass_that_fails_on_an_internal_error_leaves_admission_as_it_found_it() {
    let mut tree = Tree::new();
    let mut admission = Admission::default();
    let root = tree.root(2);
    let task = spawn(&mut admission, &mut tree, root);
    let call = tree.call(root);
    admission.enqueue_spawn(
        call,
        SpawnRecord {
            input: None,
            section: "not a task".to_owned(),
        },
    );
    assert!(matches!(
        admission.plan(&tree.view()),
        Err(Error::Internal { .. })
    ));
    assert!(admission.is_queued(task));
    assert!(!admission.holding.contains(&task));
}

#[test]
fn a_pass_that_fails_after_admitting_a_resumption_leaves_its_slots_given_back() {
    let mut tree = Tree::new();
    let mut admission = Admission::default();
    let root = tree.root(2);
    let task = spawn(&mut admission, &mut tree, root);
    assert_eq!(plan(&mut admission, &tree), [Admitted::Started(task)]);
    let call = tree.call(task);
    admission.park(call, &tree.view(), true);
    assert!(admission.wake(call, &tree.view()));
    let stray = tree.call(root);
    admission.enqueue_spawn(
        stray,
        SpawnRecord {
            input: None,
            section: "not a task".to_owned(),
        },
    );
    assert!(matches!(
        admission.plan(&tree.view()),
        Err(Error::Internal { .. })
    ));
    assert_eq!(admission.gave_back.get(&call), Some(&task));
    assert!(admission.resuming.contains(&call));
    assert!(!admission.holding.contains(&task));
    admission.drop_aborted(stray);
    assert_eq!(
        plan(&mut admission, &tree),
        [Admitted::Resumed(call)],
        "the failed pass reached the stray spawn after admitting the resumption"
    );
}
