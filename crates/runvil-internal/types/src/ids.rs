//! Hierarchical, deterministic identity for the Engine's chains and tasks.
//!
//! Every chain (the main walk, a `call` child, a spawned task) is named by
//! a path: its parent chain's id extended by the parent's local child
//! counter, which `call` children and spawned tasks share. The main walk
//! is the root chain `0`. A task's id is its chain's id. A section entry's
//! id (`sys.id` in Lua) is its chain's id extended by the chain's local
//! entry counter. Two runs of the same prompt with the same inputs
//! allocate the same ids regardless of how their chains interleave,
//! because every counter is local to the chain that advances it.
//!
//! The encoding is a dot-separated path of decimal components (`0`,
//! `0.2`, `0.2.0`), chosen over a packed integer because the depth and the
//! width of a run are both unbounded (call nesting, fanout arm count) and
//! because the path reads as the hierarchy it names in a log or a UI.
//!
//! [`AbandonReason`] names how a task's owner ended while the task was
//! still live, for the `abandoned` terminal state. [`Provenance`] extends a
//! task's id with a per-task sequence number: the replay key stamped on
//! every effect and event the Engine emits. [`RoundId`] numbers the run's
//! model rounds, so a round's effect and its content events name it alike.
//!
//! Ids order as paths: a chain before its descendants, siblings by index.
//! The tasks one chain owns are its direct children, so sorting their ids
//! recovers spawn order.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::Error;
use crate::names::make_invalid_id_error;

/// The hierarchical id of one chain: a path of child indices from the root
/// chain.
///
/// It displays and serializes as decimal components joined by dots, such
/// as `0.2.0`. Ids order as paths: a chain before its descendants, and
/// siblings by child index.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChainId(Vec<u32>);

impl ChainId {
    /// The root chain: the main walk, whose id is `0`.
    #[must_use]
    pub fn root() -> Self {
        Self(vec![0])
    }

    /// The id of this chain's child chain number `index`. A `call` child
    /// and a spawned task are both child chains, and they share the
    /// parent's counter.
    #[must_use]
    pub fn child(&self, index: u32) -> Self {
        let mut components = Vec::with_capacity(self.0.len() + 1);
        components.extend_from_slice(&self.0);
        components.push(index);
        Self(components)
    }

    /// The id of this chain's `index`-th section entry, rendered as a
    /// path: the value a section reads as `sys.id`. A section id names an
    /// entry inside a chain, so it is returned as text.
    #[must_use]
    pub fn entry(&self, index: u32) -> String {
        format!("{self}.{index}")
    }
}

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (position, component) in self.0.iter().enumerate() {
            if position > 0 {
                f.write_str(".")?;
            }
            write!(f, "{component}")?;
        }
        Ok(())
    }
}

impl FromStr for ChainId {
    type Err = Error;

    /// Parses a path: one or more components joined by dots, such as
    /// `0.2.0`, each plain decimal digits that fit in a `u32`.
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let reject = || {
            make_invalid_id_error(
                format!(
                    "invalid chain id `{input}`: required a dot-separated path of decimal \
                     components"
                ),
                input,
            )
        };
        if input.is_empty() {
            return Err(reject());
        }
        input
            .split('.')
            .map(|component| {
                // A component is plain decimal digits: no sign, no blank,
                // no leading `+`, which `u32::from_str` would otherwise
                // accept.
                if component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(reject());
                }
                component.parse::<u32>().map_err(|_| reject())
            })
            .collect::<Result<Vec<u32>, Error>>()
            .map(Self)
    }
}

impl Serialize for ChainId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ChainId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// The id of one task, which is the id of the chain that runs it.
///
/// A task and its chain are one thing named from two sides, so both ids are
/// the same path. The separate type makes a table keyed by task accept
/// only task ids, so a chain id enters it by explicit conversion. Orders
/// as its chain id does.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(ChainId);

impl From<ChainId> for TaskId {
    fn from(chain: ChainId) -> Self {
        Self(chain)
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for TaskId {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input.parse().map(Self)
    }
}

impl Serialize for TaskId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TaskId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ChainId::deserialize(deserializer).map(Self)
    }
}

/// The replay key of one effect or event: its nearest enclosing task and its
/// position within that task.
///
/// `task` is the task whose chain emitted the item. The main walk is task
/// `0`. A `call` child reports its parent's task. This is well defined
/// because a `call` blocks its parent, so the two run one at a time. `seq`
/// is a counter local to that task. The task's effects and events share
/// it, so the two kinds order against each other within one task.
///
/// Two runs of the same prompt with the same inputs and answers stamp the
/// same provenance on the same items, however their chains interleave.
/// That lets a log slice by task, order items within a task, and later
/// replay a run against its record. The `EffectId` of an in-flight effect
/// is a separate, opaque handle counted across the whole run, and it can
/// differ between runs.
///
/// Orders by task path, then by sequence.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Provenance {
    /// The nearest enclosing task.
    pub task: TaskId,
    /// The item's position among the task's effects and events.
    pub seq: u32,
}

impl Provenance {
    /// Builds the replay key of the item at position `seq` in `task`.
    #[must_use]
    pub fn new(task: TaskId, seq: u32) -> Provenance {
        Provenance { task, seq }
    }
}

/// Numbers one model round by its position in the run's dispatch order.
///
/// Rounds count from 0. One counter for the whole run numbers every round,
/// both a section's chat rounds and its nested `models.infer` rounds. A
/// round's `Chat` effect holds its id, and so do the thinking, reply, and
/// tool-call events reported from the round's answer. The caller can use
/// the id to pair the partial output it shows while a round runs with the
/// events that report the round's answer. When tasks run concurrently, the
/// order in which the caller answers effects can change which round gets
/// which number. Serializes as a bare number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RoundId(u64);

impl RoundId {
    /// Returns the round numbered `id`.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the round's number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Why a live task was abandoned: how its owner chain ended while the task
/// was still running.
///
/// A task ends with its owner. The `abandoned` terminal state is separate
/// from `canceled`, because losing an owner and being stopped on purpose
/// are different facts for a log and a UI. The reason says which kind of
/// owner end it was, so a report can say more than "abandoned".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AbandonReason {
    /// The owner ended normally, by returning a scalar or finishing its
    /// walk, and left the task live. The owner's outcome becomes the
    /// `make_tasks_live_error` error unless the task was idle after a
    /// `tasks.reply`.
    OwnerReturned,
    /// The owner failed.
    OwnerFailed,
    /// The owner's model tool loop ran past its round cap. This is a
    /// failure, reported apart from [`OwnerFailed`](Self::OwnerFailed)
    /// because a report can then name the cap as the cause.
    ToolLoopExhausted,
    /// The owner was aborted from outside, either by fail-fast after a
    /// sibling failed fatally or because the owner's own owner ended first.
    OwnerAborted,
    /// The run itself ended while the task was live, because the caller
    /// canceled the run or an answer was fatal. The Engine ended the task
    /// with the run.
    RunTerminated,
}

impl AbandonReason {
    /// The phrase the `TaskAbandoned` trace line renders for the reason:
    /// `the section ended`, `the owner failed`, `the tool loop was
    /// exhausted`, `the owner was aborted`, or `the run ended`.
    #[must_use]
    pub fn why(self) -> &'static str {
        match self {
            AbandonReason::OwnerReturned => "the section ended",
            AbandonReason::OwnerFailed => "the owner failed",
            AbandonReason::ToolLoopExhausted => "the tool loop was exhausted",
            AbandonReason::OwnerAborted => "the owner was aborted",
            AbandonReason::RunTerminated => "the run ended",
        }
    }
}

#[cfg(test)]
mod tests {
    //! Tests for `ChainId`, `TaskId`, and `Provenance` rendering, parsing, and
    //! ordering, and the phrase each `AbandonReason` renders.

    use super::{AbandonReason, ChainId, Provenance, TaskId};
    use crate::test_support::assert_wire;

    #[test]
    fn the_root_chain_is_zero_and_children_extend_it_by_index() {
        let root = ChainId::root();
        assert_eq!(root.to_string(), "0");
        assert_eq!(root.child(0).to_string(), "0.0");
        assert_eq!(root.child(2).child(1).to_string(), "0.2.1");
    }

    #[test]
    fn a_section_entry_is_the_chain_id_extended_by_the_entry_index() {
        let chain = ChainId::root().child(3);
        assert_eq!(chain.entry(0), "0.3.0");
        assert_eq!(chain.entry(7), "0.3.7");
        // A parent and its child chain never share an entry id: the paths
        // differ in length.
        assert_ne!(ChainId::root().entry(3), chain.entry(0));
    }

    #[test]
    fn a_rendered_id_parses_back_to_the_same_value() {
        let chain = ChainId::root().child(12).child(0);
        let parsed: ChainId = chain.to_string().parse().expect("a rendered id parses");
        assert_eq!(parsed, chain);
        let task: TaskId = "0.12.0".parse().expect("a task id parses");
        assert_eq!(task, TaskId::from(chain));
    }

    #[test]
    fn malformed_paths_are_rejected() {
        for input in [
            "",
            ".",
            "0.",
            ".0",
            "0..1",
            "a",
            "0.-1",
            "0.+1",
            "0. 1",
            "99999999999",
        ] {
            let error = input
                .parse::<ChainId>()
                .expect_err("a malformed path is rejected");
            assert_eq!(
                error.to_string(),
                format!(
                    "invalid chain id `{input}`: required a dot-separated path of decimal components"
                )
            );
        }
    }

    #[test]
    fn ids_serialize_as_their_path_text() {
        let chain = ChainId::root().child(2);
        assert_wire(&chain, "\"0.2\"");
        let task: TaskId = serde_json::from_str("\"0.2\"").expect("a task id deserializes");
        assert_eq!(task, TaskId::from(chain));
        assert!(
            serde_json::from_str::<ChainId>("\"0.x\"").is_err(),
            "a malformed path fails to deserialize"
        );
    }

    #[test]
    fn provenance_orders_by_task_then_sequence_and_round_trips() {
        let task: TaskId = "0.2".parse().expect("a task id parses");
        let first = Provenance::new(task.clone(), 0);
        let later = Provenance::new(task, 7);
        let other_task = Provenance::new("0.3".parse().expect("a task id parses"), 0);
        assert!(first < later, "within one task the sequence orders");
        assert!(
            later < other_task,
            "the task path orders before the sequence"
        );
        assert_wire(&later, r#"{"task":"0.2","seq":7}"#);
    }

    #[test]
    fn every_abandon_reason_renders_its_own_phrase() {
        let phrases = [
            (AbandonReason::OwnerReturned, "the section ended"),
            (AbandonReason::OwnerFailed, "the owner failed"),
            (
                AbandonReason::ToolLoopExhausted,
                "the tool loop was exhausted",
            ),
            (AbandonReason::OwnerAborted, "the owner was aborted"),
            (AbandonReason::RunTerminated, "the run ended"),
        ];
        for (reason, phrase) in phrases {
            assert_eq!(reason.why(), phrase, "{reason:?}");
        }
    }
}
