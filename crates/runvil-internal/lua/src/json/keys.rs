//! The keys of a table `json.encode` reads: the order it visits them in
//! and the shape they give the table.

use std::cmp::Ordering;

use mlua::{Table, Value};

use crate::collection::{KeyError, SortKey, sort_key};

/// What a table's keys make it.
pub(super) enum Shape {
    /// Keys exactly 1 to n.
    List,
    /// Every key a string, or an unmarked table with no keys.
    Object,
    /// Keys that break the rules, with the first rule broken.
    Broken(Rule),
}

/// A key rule a table breaks.
pub(super) enum Rule {
    MarkedKey(&'static str),
    KeyType(&'static str),
    Mixed,
    Missing(i64),
}

impl Rule {
    /// The message's text after the path.
    pub(super) fn text(&self) -> String {
        match self {
            Rule::MarkedKey(kind) => format!("is marked as a list but has a {kind} key"),
            Rule::KeyType(kind) => format!("has a {kind} key; object keys must be strings"),
            Rule::Mixed => "mixes list items and named keys".to_owned(),
            Rule::Missing(item) => format!("is missing list item [{item}]"),
        }
    }
}

/// The shape a table's sorted `keys` give it, checking the rules in order:
/// a marked list's key types, then an unmarked table's key types, then a
/// mix of list items and named keys, then a missing list item.
pub(super) fn shape(keys: &[Value], marked: bool) -> Shape {
    let is_item = |key: &Value| matches!(key, Value::Integer(_));
    let items = keys.iter().filter(|key| is_item(key)).count();
    if marked {
        if let Some(key) = keys.iter().find(|key| !is_item(key)) {
            return Shape::Broken(Rule::MarkedKey(key.type_name()));
        }
    } else {
        if let Some(key) = keys
            .iter()
            .find(|key| !matches!(key, Value::Integer(_) | Value::String(_)))
        {
            return Shape::Broken(Rule::KeyType(key.type_name()));
        }
        if items == 0 {
            return Shape::Object;
        }
        if items < keys.len() {
            return Shape::Broken(Rule::Mixed);
        }
    }
    match missing_item(keys, items) {
        Some(item) => Shape::Broken(Rule::Missing(item)),
        None => Shape::List,
    }
}

/// The first of 1 to `count` absent from the integer `keys`, which are in
/// ascending order.
fn missing_item(keys: &[Value], count: usize) -> Option<i64> {
    let mut expected: i64 = 1;
    for key in keys {
        let Value::Integer(key) = key else {
            continue;
        };
        match key.cmp(&expected) {
            Ordering::Less => {}
            Ordering::Equal => expected += 1,
            Ordering::Greater => break,
        }
    }
    let count = i64::try_from(count).unwrap_or(i64::MAX);
    (expected <= count).then_some(expected)
}

/// A table's keys in visiting order, and how many of them hold a Lua
/// reference.
pub(super) struct Entries {
    pub(super) keys: Vec<Value>,
    pub(super) held: usize,
}

/// A table's keys in visiting order: booleans, then numbers by value, then
/// strings bytewise, then every other key ranked by type name, in no fixed
/// order within one type. Each value is dropped as soon as its pair is
/// read, which frees the reference slot it holds, and a caller reads a
/// value with `raw_get` when its turn comes. Gives `None` as soon as more
/// than `room` keys hold references, so the key vector never grows past the
/// cap.
pub(super) fn entries(table: &Table, room: usize) -> mlua::Result<Option<Entries>> {
    let mut found = Vec::new();
    let mut held = 0;
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair?;
        if holds_reference(&key) {
            held += 1;
            if held > room {
                return Ok(None);
            }
        }
        found.push((position(&key), key));
    }
    found.sort_by(|left, right| compare(&left.0, &right.0));
    Ok(Some(Entries {
        keys: found.into_iter().map(|(_, key)| key).collect(),
        held,
    }))
}

/// Whether `key` takes a slot on the auxiliary reference stack while it is
/// alive: integers, numbers, booleans, and light userdata are copied by
/// value.
fn holds_reference(key: &Value) -> bool {
    matches!(
        key,
        Value::String(_)
            | Value::Table(_)
            | Value::Function(_)
            | Value::Thread(_)
            | Value::UserData(_)
            | Value::Other(_)
    )
}

/// A key's place in visiting order.
enum Position {
    Sorted(SortKey),
    Unsorted(&'static str),
}

/// Places `key` by `sort_key`, also placing a string that is not valid
/// UTF-8 by its bytes and an infinite number by its value.
fn position(key: &Value) -> Position {
    match (sort_key(key), key) {
        (Ok((position, _)), _) => Position::Sorted(position),
        (Err(KeyError::NotUtf8(_)), Value::String(text)) => {
            Position::Sorted(SortKey::Text(text.as_bytes().to_vec()))
        }
        (Err(KeyError::NotFinite), Value::Number(number)) => {
            Position::Sorted(SortKey::Float(*number))
        }
        (Err(_), other) => Position::Unsorted(other.type_name()),
    }
}

fn compare(left: &Position, right: &Position) -> Ordering {
    match (left, right) {
        (Position::Sorted(left), Position::Sorted(right)) => left.compare(right),
        (Position::Sorted(_), Position::Unsorted(_)) => Ordering::Less,
        (Position::Unsorted(_), Position::Sorted(_)) => Ordering::Greater,
        (Position::Unsorted(left), Position::Unsorted(right)) => left.cmp(right),
    }
}
