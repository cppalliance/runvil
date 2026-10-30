//! Tests for the task arena alone, one rule each: a stop ends only a live
//! task, a completed task's outcome is delivered once, entries are never
//! removed, `owned` admits only the owner, `visible` adds the chain
//! running inside the task, and `live_author_tasks` lists an owner's live
//! tasks in spawn order without internal timers.

use runvil_types::ids::{ChainId, TaskId};
use runvil_vfs::{Origin, VfsRef};

use super::super::ChainIndex;
use super::{Stop, TaskArena, TaskBacking, TaskState};
use crate::Error;
use crate::execute::run::EffectId;

fn chain(raw: u32) -> ChainIndex {
    ChainIndex::for_test(raw)
}

/// Child task `index` of the root walk.
fn task(index: u32) -> TaskId {
    TaskId::from(ChainId::root().child(index))
}

/// A backing chain at arena index `raw` under a fresh identity.
fn chain_backing(raw: u32) -> TaskBacking {
    let access = VfsRef::default()
        .acquire(Origin::new("tasks-test"))
        .expect("the default backend acquires");
    TaskBacking::Chain {
        chain: chain(raw),
        exec: runvil_vfs::detail::access_id(&access),
    }
}

/// Registers child task `index` of the root walk, owned by `owner` and
/// backed by the chain at arena index `100 + index`.
fn spawn(arena: &mut TaskArena, owner: ChainIndex, index: u32) -> (TaskId, TaskBacking) {
    let backing = chain_backing(100 + index);
    arena.insert(task(index), backing, owner, format!("worker {index}"));
    (task(index), backing)
}

/// Registers child task `index` of the root walk as an internal timer
/// owned by `owner`.
fn timer(arena: &mut TaskArena, owner: ChainIndex, index: u32) -> TaskId {
    let backing = TaskBacking::Effect(EffectId(u64::from(index)));
    arena.insert(task(index), backing, owner, "timer".to_owned());
    task(index)
}

#[test]
fn stopping_a_running_task_ends_it_and_returns_its_backing_and_target() {
    let mut arena = TaskArena::default();
    let (cancelled, cancelled_backing) = spawn(&mut arena, chain(0), 0);
    let (abandoned, abandoned_backing) = spawn(&mut arena, chain(0), 1);
    assert_eq!(
        arena.stop(&cancelled, Stop::Cancelled),
        Some((cancelled_backing, "worker 0".to_owned()))
    );
    assert_eq!(
        arena.stop(&abandoned, Stop::Abandoned),
        Some((abandoned_backing, "worker 1".to_owned()))
    );
    assert_eq!(arena.state_of(&cancelled), Some(TaskState::Cancelled));
    assert_eq!(arena.state_of(&abandoned), Some(TaskState::Abandoned));
}

#[test]
fn stopping_a_task_that_is_not_live_returns_none_and_changes_nothing() {
    let mut arena = TaskArena::default();
    let (done, _) = spawn(&mut arena, chain(0), 0);
    let (cancelled, _) = spawn(&mut arena, chain(0), 1);
    assert!(arena.complete(&done, Ok("text".to_owned())).is_some());
    assert!(arena.stop(&cancelled, Stop::Cancelled).is_some());
    for stop in [Stop::Cancelled, Stop::Abandoned] {
        assert_eq!(arena.stop(&done, stop), None);
        assert_eq!(arena.stop(&cancelled, stop), None);
        assert_eq!(arena.stop(&task(9), stop), None, "task 9 has no entry");
    }
    assert_eq!(arena.state_of(&done), Some(TaskState::Done { ok: true }));
    assert_eq!(arena.state_of(&cancelled), Some(TaskState::Cancelled));
    assert_eq!(arena.state_of(&task(9)), None);
}

#[test]
fn a_completed_task_is_done_and_its_outcome_is_taken_once() {
    let mut arena = TaskArena::default();
    let (task, _) = spawn(&mut arena, chain(0), 0);
    let completed = arena
        .complete(&task, Ok("text".to_owned()))
        .expect("a running task completes");
    assert_eq!(completed.owner, chain(0));
    assert_eq!(arena.state_of(&task), Some(TaskState::Done { ok: true }));
    assert_eq!(arena.take_delivery(&task).expect("the outcome"), "text");
    assert_eq!(
        arena.state_of(&task),
        Some(TaskState::Delivered { ok: true })
    );
    assert!(matches!(
        arena.take_delivery(&task),
        Err(Error::Internal { .. })
    ));
    assert_eq!(
        arena.state_of(&task),
        Some(TaskState::Delivered { ok: true })
    );
}

#[test]
fn a_refused_delivery_leaves_the_entry_unchanged() {
    let mut arena = TaskArena::default();
    let (cancelled, _) = spawn(&mut arena, chain(0), 0);
    let (abandoned, _) = spawn(&mut arena, chain(0), 1);
    let (running, _) = spawn(&mut arena, chain(0), 2);
    assert!(arena.stop(&cancelled, Stop::Cancelled).is_some());
    assert!(arena.stop(&abandoned, Stop::Abandoned).is_some());
    for (task, state) in [
        (&cancelled, TaskState::Cancelled),
        (&abandoned, TaskState::Abandoned),
        (&running, TaskState::Running),
    ] {
        assert!(arena.take_delivery(task).is_err(), "{task}");
        assert_eq!(arena.state_of(task), Some(state), "{task}");
    }
}

#[test]
fn completing_a_task_that_is_not_live_changes_nothing() {
    let mut arena = TaskArena::default();
    let (cancelled, _) = spawn(&mut arena, chain(0), 0);
    assert!(arena.stop(&cancelled, Stop::Cancelled).is_some());
    assert!(arena.complete(&cancelled, Ok("late".to_owned())).is_none());
    assert!(arena.complete(&task(9), Ok("unknown".to_owned())).is_none());
    assert_eq!(arena.state_of(&cancelled), Some(TaskState::Cancelled));
    assert_eq!(arena.state_of(&task(9)), None);
}

#[test]
fn an_ended_or_delivered_task_keeps_its_entry() {
    let mut arena = TaskArena::default();
    let (cancelled, _) = spawn(&mut arena, chain(0), 0);
    let (abandoned, _) = spawn(&mut arena, chain(0), 1);
    let (delivered, _) = spawn(&mut arena, chain(0), 2);
    assert!(arena.stop(&cancelled, Stop::Cancelled).is_some());
    assert!(arena.stop(&abandoned, Stop::Abandoned).is_some());
    assert!(
        arena
            .complete(&delivered, Err(Error::Interrupted))
            .is_some()
    );
    assert!(arena.take_delivery(&delivered).is_err());
    for task in [&cancelled, &abandoned, &delivered] {
        assert_eq!(arena.owner_of(task), Some(chain(0)), "{task}");
    }
    assert_eq!(
        arena.state_of(&delivered),
        Some(TaskState::Delivered { ok: false })
    );
}

#[test]
fn owned_admits_only_the_tasks_owner() {
    let mut arena = TaskArena::default();
    let (task, _) = spawn(&mut arena, chain(1), 0);
    let entry = arena.owned(chain(1), &task).expect("the owner reaches it");
    assert_eq!(entry.target, "worker 0");
    assert!(matches!(
        arena.owned(chain(2), &task),
        Err(Error::TaskNotOwned { task: refused }) if refused == task
    ));
    let unknown = TaskId::from(ChainId::root().child(9));
    assert!(matches!(
        arena.owned(chain(1), &unknown),
        Err(Error::TaskNotOwned { task: refused }) if refused == unknown
    ));
}

#[test]
fn visible_also_admits_a_caller_running_inside_the_task() {
    let mut arena = TaskArena::default();
    let (task, _) = spawn(&mut arena, chain(1), 0);
    let outside = TaskId::from(ChainId::root());
    assert!(arena.visible(chain(1), &outside, &task).is_ok());
    assert!(
        arena.visible(chain(7), &task, &task).is_ok(),
        "a call child of the task chain shares its sys.taskid"
    );
    assert!(matches!(
        arena.visible(chain(7), &outside, &task),
        Err(Error::TaskNotOwned { task: refused }) if refused == task
    ));
    let unknown = TaskId::from(ChainId::root().child(9));
    assert!(matches!(
        arena.visible(chain(7), &unknown, &unknown),
        Err(Error::TaskNotOwned { .. })
    ));
}

#[test]
fn live_author_tasks_lists_an_owners_live_tasks_in_spawn_order_without_timers() {
    let mut arena = TaskArena::default();
    let (tenth, _) = spawn(&mut arena, chain(0), 10);
    let (second, _) = spawn(&mut arena, chain(0), 2);
    timer(&mut arena, chain(0), 1);
    let (ended, _) = spawn(&mut arena, chain(0), 3);
    spawn(&mut arena, chain(5), 4);
    let (first, _) = spawn(&mut arena, chain(0), 0);
    assert!(arena.stop(&ended, Stop::Cancelled).is_some());
    assert_eq!(arena.live_author_tasks(chain(0)), [first, second, tenth]);
}
