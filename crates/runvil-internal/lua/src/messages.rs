//! The `messages` global: the section's one Rust-backed conversation.
//!
//! Every section VM owns one [`MessageList`], which injection installs as
//! the `messages` global, served from the `_G` guard's slot table so it
//! keeps its identity for the section's whole life and an assignment of
//! the global raises [`ASSIGNMENT_REFUSAL`]. Its chainable
//! `system`/`user`/`assistant`/`tool`/`append` builders and `replace` are
//! colon methods, and `replace` works only while a compactor runs. Each
//! builder takes an optional last `meta`, which the projection never
//! sends. Each edit validates its records as it adds
//! them, keeps the system records leading the list, and drops fields a
//! record does not hold, so a refused edit raises at the author's call and
//! leaves the list unchanged. `#messages` counts the records,
//! `messages[i]`, `pairs`, and `ipairs` read them as read-only views with
//! metamethods only, and assignment by index is refused. `pairs` stops at
//! the record count it started with, as the sandbox `pairs` does for a
//! table, while `ipairs` reads the live list.
//!
//! Author edits validate the records they add. Records the Engine adds by
//! calling [`MessageList::push`] itself - the loop's state machine - are
//! valid by construction and skip that validation; `push` checks only the
//! leading-system rule. A model round's request holds a clone of the same
//! list, so the round reads every record with no second validation.
//!
//! The list also holds the section's rendered tool list, which the Engine
//! attaches at the first loop round that builds its tools, so it lasts
//! exactly as long as the section. Once one is attached, every offer
//! function raises. A compaction edits the list in place and keeps a
//! snapshot of its records to put back if the compaction fails; neither
//! the snapshot nor `compactors.clear` touches the last send or the
//! rendered tool list.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mlua::{
    AnyUserData, Function, Lua, LuaSerdeExt, MetaMethod, UserData, UserDataMethods, Value, Variadic,
};
use runvil_model_client::client::{Message, Role};
use runvil_types::ids::RoundId;
use serde_json::{Map, Value as Json};

use crate::error::{Error, Result};
use crate::protocol::{MessageRecord, RecordRole, parse_record};
use crate::scope::RenderedTools;

#[path = "messages-snapshot.rs"]
mod snapshot;
#[path = "messages-view.rs"]
mod view;

pub(crate) use snapshot::Snapshot;

/// The refusal of an assignment of the `messages` global.
pub(crate) const ASSIGNMENT_REFUSAL: &str = "messages is the section's conversation and cannot \
                                             be reassigned; add records with append(record), \
                                             or rewrite history with \
                                             models.compact(compactor, reason)";

/// Installs `list` as the `messages` global, served from the guard's slot.
///
/// # Errors
/// Returns [`Error::Lua`] if the userdata cannot be built or the guard's
/// slot table cannot be written.
pub(crate) fn install_messages(lua: &Lua, list: &MessageList) -> Result<()> {
    let userdata = lua.create_userdata(list.clone()).map_err(Error::lua)?;
    crate::globals::install_messages(lua, userdata)
}

/// A section's conversation, which the `messages` global holds. The same
/// value is the Lua userdata and the handle a model round's request
/// holds; clones share one list.
#[derive(Clone, Debug, Default)]
pub struct MessageList(Arc<Mutex<ListState>>);

/// Why a [`MessageList`] refused an edit, which leaves the list unchanged.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ListEditError {
    /// A system record at the 1-based `index` would sit behind a
    /// non-system record.
    #[error(
        "messages[{index}] is a system message after a non-system message; system messages \
         lead the list, and only a compactor can change the leading block"
    )]
    LateSystem {
        /// The late record's 1-based position.
        index: usize,
    },
    /// A `replace(first, last)` range falls outside a list of `len` records.
    #[error(
        "replace({first}, {last}) is out of bounds on a list of {len} records: it needs \
         1 <= first <= last + 1 <= {}",
        len.saturating_add(1)
    )]
    OutOfBounds {
        /// The range's first position, as given.
        first: i64,
        /// The range's last position, as given.
        last: i64,
        /// The list's record count.
        len: usize,
    },
}

/// What a list holds.
#[derive(Debug, Default)]
struct ListState {
    /// The records, in order, shared with the views read from them.
    records: Vec<Arc<MessageRecord>>,
    /// The last issued send: its round and the wire request it sent.
    last: Option<(RoundId, Vec<Message>)>,
    /// Attached at the first successful build and never detached.
    rendered: Option<RenderedTools>,
    /// Set while a compactor runs over the list.
    compacting: bool,
}

impl MessageList {
    /// The list's state. Every edit swaps in its result whole, so a lock
    /// poisoned by a panic elsewhere still guards a valid list.
    fn state(&self) -> MutexGuard<'_, ListState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Returns a copy of the records in order, for a round's projection.
    #[must_use]
    pub fn to_records(&self) -> Vec<MessageRecord> {
        self.state()
            .records
            .iter()
            .map(|record| MessageRecord::clone(record))
            .collect()
    }

    /// How many records the list holds.
    fn len(&self) -> usize {
        self.state().records.len()
    }

    /// The record at the 1-based `index`, if the list holds one there.
    fn record(&self, index: usize) -> Option<Arc<MessageRecord>> {
        let position = index.checked_sub(1)?;
        self.state().records.get(position).cloned()
    }

    /// Appends `record`. A system record after any non-system record is
    /// refused.
    ///
    /// # Errors
    /// Returns [`ListEditError::LateSystem`] naming a late system record's
    /// position.
    pub(crate) fn push(&self, record: MessageRecord) -> std::result::Result<(), ListEditError> {
        let mut state = self.state();
        if record.role == RecordRole::System
            && state
                .records
                .iter()
                .any(|held| held.role != RecordRole::System)
        {
            return Err(ListEditError::LateSystem {
                index: state.records.len() + 1,
            });
        }
        state.records.push(Arc::new(record));
        Ok(())
    }

    /// Replaces the 1-based inclusive range `first..=last` with
    /// `records`, where `1 <= first <= last + 1 <= len + 1`, then checks
    /// the leading-system rule. A refused edit leaves the list unchanged.
    fn replace(
        &self,
        first: usize,
        last: usize,
        records: Vec<MessageRecord>,
    ) -> std::result::Result<(), ListEditError> {
        let mut state = self.state();
        let len = state.records.len();
        if !in_bounds(first, last, len) {
            return Err(out_of_bounds(first, last, len));
        }
        let edited: Vec<Arc<MessageRecord>> = state.records[..first - 1]
            .iter()
            .cloned()
            .chain(records.into_iter().map(Arc::new))
            .chain(state.records[last..].iter().cloned())
            .collect();
        check_leading_system(&edited)?;
        state.records = edited;
        Ok(())
    }

    /// Records `request` as the list's send in `round`, and returns the
    /// previous send's round and how many leading messages `request`
    /// shares with that send's request; `(None, 0)` on the first send.
    #[must_use]
    pub fn commit(&self, round: RoundId, request: &[Message]) -> (Option<RoundId>, u64) {
        let previous = self.state().last.replace((round, request.to_vec()));
        previous.map_or((None, 0), |(after, sent)| {
            let keep = sent
                .iter()
                .zip(request)
                .take_while(|(sent, now)| sent == now)
                .count();
            (Some(after), u64::try_from(keep).unwrap_or(u64::MAX))
        })
    }

    /// Whether a rendered tool list is attached; no list means false.
    #[must_use]
    pub(crate) fn rendered(&self) -> bool {
        self.state().rendered.is_some()
    }

    /// A copy of the attached rendered tool list, if one is attached.
    #[must_use]
    pub fn rendered_tools(&self) -> Option<RenderedTools> {
        self.state().rendered.clone()
    }

    /// Attaches `tools` as the section's rendered tool list. A list that
    /// already holds one keeps it.
    ///
    /// The Engine calls this only on a list with none attached.
    pub fn attach_rendered_tools(&self, tools: RenderedTools) {
        self.state().rendered.get_or_insert(tools);
    }
}

/// Whether `replace` can edit `first..=last` on a list of `len` records:
/// `1 <= first <= last + 1 <= len + 1`.
fn in_bounds(first: usize, last: usize, len: usize) -> bool {
    first >= 1 && last <= len && first <= last + 1
}

/// Returns the refusal for a `replace` range outside the list's bounds,
/// from the range's positions as given.
fn out_of_bounds(first: impl TryInto<i64>, last: impl TryInto<i64>, len: usize) -> ListEditError {
    ListEditError::OutOfBounds {
        first: first.try_into().unwrap_or(i64::MAX),
        last: last.try_into().unwrap_or(i64::MAX),
        len,
    }
}

/// Refuses records whose system records do not all lead them, naming the
/// first late one.
fn check_leading_system(records: &[Arc<MessageRecord>]) -> std::result::Result<(), ListEditError> {
    let lead = records
        .iter()
        .take_while(|record| record.role == RecordRole::System)
        .count();
    match records[lead..]
        .iter()
        .position(|record| record.role == RecordRole::System)
    {
        Some(offset) => Err(ListEditError::LateSystem {
            index: lead + offset + 1,
        }),
        None => Ok(()),
    }
}

/// Raises `message` as a list method's error, which an author `pcall`
/// catches.
fn refusal(message: String) -> mlua::Error {
    mlua::Error::external(Error::Lua(message))
}

/// The list a method was called on.
fn handle(this: &AnyUserData) -> mlua::Result<MessageList> {
    Ok(this.borrow::<MessageList>()?.clone())
}

/// One Lua value in its JSON form, or why it has none.
fn json(lua: &Lua, value: Value) -> std::result::Result<Json, String> {
    lua.from_value(value).map_err(|error| match error {
        mlua::Error::DeserializeError(reason) => reason,
        other => other.to_string(),
    })
}

/// A whole record given to `append` or `replace`, in its JSON form, for
/// the position `index` it takes.
fn record_json(lua: &Lua, index: usize, value: Value) -> mlua::Result<Json> {
    json(lua, value).map_err(|reason| {
        refusal(format!(
            "messages[{index}] must be a JSON-representable message table: {reason}"
        ))
    })
}

/// Validates `entry` as the record at `index`, the end of `list`, and
/// appends it.
fn add(list: &MessageList, index: usize, entry: &Json) -> mlua::Result<()> {
    let record = parse_record(index, entry).map_err(refusal)?;
    list.push(record)
        .map_err(|refused| refusal(refused.to_string()))
}

/// Appends the record a builder makes from `role` and `fields`, leaving
/// out each field given as nil, and returns the list for chaining.
fn build<const N: usize>(
    lua: &Lua,
    this: AnyUserData,
    role: Role,
    fields: [(&str, Value); N],
) -> mlua::Result<AnyUserData> {
    let list = handle(&this)?;
    let index = list.len() + 1;
    let mut entry = Map::new();
    entry.insert("role".to_owned(), Json::from(role.as_str()));
    for (name, value) in fields {
        if !value.is_nil() {
            let value = json(lua, value).map_err(|reason| {
                refusal(format!(
                    "messages[{index}] {name} must be JSON-representable: {reason}"
                ))
            })?;
            entry.insert(name.to_owned(), value);
        }
    }
    add(&list, index, &Json::Object(entry))?;
    Ok(this)
}

/// A Lua number under the integer rule: an integer, or a float with an
/// integral value.
fn integral(value: &Value) -> Option<i64> {
    match value {
        Value::Integer(integer) => Some(*integer),
        Value::Number(number) if number.fract() == 0.0 =>
        {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the value is integral, and a magnitude past i64 saturates to a value \
                          that is out of range either way"
            )]
            Some(*number as i64)
        }
        _ => None,
    }
}

/// The 1-based list index a Lua key names under the integer rule.
fn key_index(key: &Value) -> Option<usize> {
    integral(key).and_then(|index| usize::try_from(index).ok())
}

/// One `replace` bound under the integer rule.
fn bound(name: &str, value: &Value) -> mlua::Result<i64> {
    integral(value).ok_or_else(|| match value {
        Value::Number(bound) => refusal(format!("replace {name} must be an integer, got {bound}")),
        other => refusal(format!(
            "replace {name} must be an integer, got {}",
            other.type_name()
        )),
    })
}

/// The index after `previous` in a `pairs` walk, and a new view of the
/// live record there, or nils past `extent` or past the live list's end.
/// `extent` is the record count when the walk began, so a loop that
/// appends still ends, as the sandbox `pairs` fixes a table's keys at its
/// start.
fn next_record(
    lua: &Lua,
    list: &MessageList,
    previous: &Value,
    extent: usize,
) -> mlua::Result<(Option<usize>, Value)> {
    let index = match previous {
        Value::Nil => Some(1),
        key => key_index(key).and_then(|index| index.checked_add(1)),
    };
    match index
        .filter(|index| *index <= extent)
        .and_then(|index| Some((index, list.record(index)?)))
    {
        Some((index, record)) => Ok((Some(index), view::create(lua, record)?)),
        None => Ok((None, Value::Nil)),
    }
}

impl UserData for MessageList {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        for (name, role) in [("system", Role::System), ("user", Role::User)] {
            methods.add_function(
                name,
                move |lua, (this, content, meta): (AnyUserData, Value, Value)| {
                    build(lua, this, role, [("content", content), ("meta", meta)])
                },
            );
        }
        for (name, role, field) in [
            ("assistant", Role::Assistant, "tool_calls"),
            ("tool", Role::Tool, "tool_call_id"),
        ] {
            methods.add_function(
                name,
                move |lua, (this, content, value, meta): (AnyUserData, Value, Value, Value)| {
                    build(
                        lua,
                        this,
                        role,
                        [("content", content), (field, value), ("meta", meta)],
                    )
                },
            );
        }
        methods.add_function("append", |lua, (this, record): (AnyUserData, Value)| {
            let list = handle(&this)?;
            let index = list.len() + 1;
            add(&list, index, &record_json(lua, index, record)?)?;
            Ok(this)
        });
        methods.add_function(
            "replace",
            |lua, (this, first, last, records): (AnyUserData, Value, Value, Variadic<Value>)| {
                let target = handle(&this)?;
                if !target.compacting() {
                    return Err(refusal(
                        "replace works only inside a compactor; rewrite history with \
                         models.compact(compactor, reason)"
                            .to_owned(),
                    ));
                }
                let first = bound("first", &first)?;
                let last = bound("last", &last)?;
                let len = target.len();
                let (first, last) = match (usize::try_from(first), usize::try_from(last)) {
                    (Ok(first), Ok(last)) if in_bounds(first, last, len) => (first, last),
                    _ => return Err(refusal(out_of_bounds(first, last, len).to_string())),
                };
                let records = records
                    .into_iter()
                    .enumerate()
                    .map(|(offset, value)| {
                        let index = first + offset;
                        parse_record(index, &record_json(lua, index, value)?).map_err(refusal)
                    })
                    .collect::<mlua::Result<Vec<_>>>()?;
                target
                    .replace(first, last, records)
                    .map_err(|refused| refusal(refused.to_string()))?;
                Ok(this)
            },
        );
        methods.add_meta_method(MetaMethod::Len, |_, this, ()| Ok(this.len()));
        methods.add_meta_method(
            MetaMethod::Index,
            |lua, this, key: Value| match key_index(&key).and_then(|index| this.record(index)) {
                Some(record) => view::create(lua, record),
                None => Ok(Value::Nil),
            },
        );
        methods.add_meta_function(
            MetaMethod::Pairs,
            |lua, this: AnyUserData| -> mlua::Result<(Function, AnyUserData, Value)> {
                let list = handle(&this)?;
                let extent = list.len();
                let next = lua.create_function(move |lua, (_, previous): (Value, Value)| {
                    next_record(lua, &list, &previous, extent)
                })?;
                Ok((next, this, Value::Nil))
            },
        );
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, _: (Value, Value)| -> mlua::Result<()> {
                Err(refusal(
                    "messages cannot be assigned to by index; add records with append(record), \
                     or rewrite history with models.compact(compactor, reason)"
                        .to_owned(),
                ))
            },
        );
    }
}

#[cfg(test)]
#[path = "messages-tests.rs"]
mod tests;
