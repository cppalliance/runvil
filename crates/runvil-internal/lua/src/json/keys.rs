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

/// The shape a table's sorted `entries` give it, checking the rules in
/// order: a marked list's key types, then an unmarked table's key types,
/// then a mix of list items and named keys, then a missing list item.
pub(super) fn shape(entries: &[(Value, Value)], marked: bool) -> Shape {
    let is_item = |key: &Value| matches!(key, Value::Integer(_));
    let items = entries.iter().filter(|(key, _)| is_item(key)).count();
    if marked {
        if let Some((key, _)) = entries.iter().find(|(key, _)| !is_item(key)) {
            return Shape::Broken(Rule::MarkedKey(key.type_name()));
        }
    } else {
        if let Some((key, _)) = entries
            .iter()
            .find(|(key, _)| !matches!(key, Value::Integer(_) | Value::String(_)))
        {
            return Shape::Broken(Rule::KeyType(key.type_name()));
        }
        if items == 0 {
            return Shape::Object;
        }
        if items < entries.len() {
            return Shape::Broken(Rule::Mixed);
        }
    }
    match missing_item(entries, items) {
        Some(item) => Shape::Broken(Rule::Missing(item)),
        None => Shape::List,
    }
}

/// The first of 1 to `count` absent from the integer keys of `entries`,
/// which are in ascending order.
fn missing_item(entries: &[(Value, Value)], count: usize) -> Option<i64> {
    let mut expected: i64 = 1;
    for (key, _) in entries {
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

/// A table's entries in visiting order: booleans, then numbers by value,
/// then strings bytewise, then every other key ranked by type name, in no
/// fixed order within one type.
pub(super) fn entries(table: &Table) -> mlua::Result<Vec<(Value, Value)>> {
    let mut entries = table
        .pairs::<Value, Value>()
        .map(|pair| pair.map(|(key, value)| (position(&key), key, value)))
        .collect::<mlua::Result<Vec<_>>>()?;
    entries.sort_by(|left, right| compare(&left.0, &right.0));
    Ok(entries
        .into_iter()
        .map(|(_, key, value)| (key, value))
        .collect())
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
