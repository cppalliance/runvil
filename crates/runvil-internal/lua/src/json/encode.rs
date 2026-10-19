//! `json.encode` and `json.encode_lines`: Lua values to compact JSON text.
//!
//! The walk never calls back into Lua. It reads entries with
//! `Table::pairs`, which skips `__pairs`, metatables with
//! `Table::metatable`, which ignores `__metatable`, and fields of a
//! metatable with `raw_get`, and it compares tables by pointer. It builds a
//! `serde_json` tree on the Rust heap, whose objects sort their keys, and
//! writes it in one pass. In debug mode each failure inside the value
//! becomes a placeholder string, so any value encodes, except a value too
//! large to walk: the walk counts every value it visits, shared tables and
//! repeated strings once per occurrence, and refuses past a budget sized
//! from the Lua heap. It reads a table's keys first and each value when its
//! turn comes, and refuses a path that holds too many keys as Lua
//! references at once.

use std::collections::HashSet;
use std::ffi::c_void;

use mlua::{Lua, LuaSerdeExt, LuaString, Table, Value};
use serde_json::{Map, Number, Value as Json};

use super::keys::{Entries, Rule, Shape, entries, shape};
use super::refusal;
use crate::proxy::{key_path, key_text};
use crate::walk_budget::{HELD_KEY_LIMIT, TooLarge, WalkBudget};

/// The most tables deep a value may nest, the top-level table being level
/// 1: the depth `json.decode` accepts.
const DEPTH_LIMIT: usize = 127;

/// `json.encode(value [, options])`: `value` as one line of compact JSON.
pub(super) fn encode(lua: &Lua, (value, options): (Value, Value)) -> mlua::Result<LuaString> {
    const FUNCTION: &str = "json.encode";
    let debug = debug_option(&options, FUNCTION)?;
    let json = Encoder::new(lua, FUNCTION, debug).value(&value, 1)?;
    let text =
        serde_json::to_string(&json).map_err(|error| refusal(format!("{FUNCTION}: {error}")))?;
    lua.create_string(text)
}

/// `json.encode_lines(list [, options])`: each item of `list` encoded by
/// the `json.encode` rules and followed by a newline.
pub(super) fn encode_lines(lua: &Lua, (list, options): (Value, Value)) -> mlua::Result<LuaString> {
    const FUNCTION: &str = "json.encode_lines";
    let debug = debug_option(&options, FUNCTION)?;
    let Value::Table(list) = list else {
        return Err(refusal(format!(
            "{FUNCTION} expects a list, got {}",
            list.type_name()
        )));
    };
    let mut text = String::new();
    for item in Encoder::new(lua, FUNCTION, debug).items(&list)? {
        let line = serde_json::to_string(&item)
            .map_err(|error| refusal(format!("{FUNCTION}: {error}")))?;
        text.push_str(&line);
        text.push('\n');
    }
    lua.create_string(text)
}

/// Reads the options argument, nil or a table whose only key is `debug`, a
/// boolean, and returns whether debug mode is on. Keys are checked in
/// visiting order, so the first offending key is the one reported.
fn debug_option(options: &Value, function: &str) -> mlua::Result<bool> {
    let table = match options {
        Value::Nil => return Ok(false),
        Value::Table(table) => table,
        other => {
            return Err(refusal(format!(
                "{function}: options must be a table, got {}",
                other.type_name()
            )));
        }
    };
    let Some(options) = entries(table, HELD_KEY_LIMIT)? else {
        return Err(too_large(function));
    };
    let mut debug = false;
    for key in options.keys {
        if !matches!(&key, Value::String(name) if name == "debug") {
            return Err(refusal(format!(
                "{function}: unknown option {}",
                key_text(&key)
            )));
        }
        let value: Value = table.raw_get(key)?;
        let Value::Boolean(flag) = value else {
            return Err(refusal(format!(
                "{function}: option debug must be a boolean, got {}",
                value.type_name()
            )));
        };
        debug = flag;
    }
    Ok(debug)
}

/// The refusal for a value too large to walk: past the value budget, the
/// byte budget, or the held-key limit. It has no path.
fn too_large(function: &str) -> mlua::Error {
    refusal(format!("{function}: value is too large to encode"))
}

/// Why a value cannot be encoded, before the function name and path are
/// added.
enum Failure {
    NotData(&'static str),
    NotUtf8,
    NotFinite,
    Unreadable(Option<String>),
    Keys(Rule),
    Cycle,
    TooDeep,
}

impl Failure {
    /// The message's text after the path.
    fn text(&self) -> String {
        match self {
            Failure::NotData(kind) => format!("must be JSON data, got {kind}"),
            Failure::NotUtf8 => "is not valid UTF-8".to_owned(),
            Failure::NotFinite => "is not a finite number".to_owned(),
            Failure::Unreadable(Some(label)) => {
                format!("cannot be read ({label}); copy the fields you need into a plain table")
            }
            Failure::Unreadable(None) => {
                "cannot be read; copy the fields you need into a plain table".to_owned()
            }
            Failure::Keys(rule) => rule.text(),
            Failure::Cycle => "contains itself".to_owned(),
            Failure::TooDeep => format!("is nested more than {DEPTH_LIMIT} levels deep"),
        }
    }
}

/// What a table stands for once sealed views are read through.
enum Resolved {
    Data(Table),
    Unreadable(Option<String>),
    /// A chain of views that leads back to one of its own views.
    Loop,
}

/// One walk over a value: the mode, the path of keys to the current value
/// for error messages, the tables on that path for cycle detection, what
/// is left of the budget for values and text, and how many keys the tables
/// on the path hold as Lua references.
struct Encoder {
    function: &'static str,
    debug: bool,
    marker: Table,
    keys: Vec<Value>,
    tables: Vec<*const c_void>,
    budget: WalkBudget,
    held: usize,
}

impl Encoder {
    fn new(lua: &Lua, function: &'static str, debug: bool) -> Self {
        Self {
            function,
            debug,
            marker: lua.array_metatable(),
            keys: Vec::new(),
            tables: Vec::new(),
            budget: WalkBudget::for_vm(lua),
            held: 0,
        }
    }

    /// The refusal for a walk past its budget or the held-key limit. It has
    /// no path, and it never becomes a placeholder: one placeholder per
    /// remaining value would not bound the work.
    fn too_large(&self) -> mlua::Error {
        too_large(self.function)
    }

    /// Spends one value of the budget.
    fn charge_node(&mut self) -> mlua::Result<()> {
        self.budget
            .charge_node()
            .map_err(|TooLarge| self.too_large())
    }

    /// Spends `count` bytes of text of the budget.
    fn charge_bytes(&mut self, count: usize) -> mlua::Result<()> {
        self.budget
            .charge_bytes(count)
            .map_err(|TooLarge| self.too_large())
    }

    /// The error for `failure` at the current path.
    fn error(&self, failure: &Failure) -> mlua::Error {
        let path = self
            .keys
            .iter()
            .fold("value".to_owned(), |path, key| key_path(&path, key));
        refusal(format!("{}: {path} {}", self.function, failure.text()))
    }

    /// `placeholder` in debug mode, or the error for `failure`.
    fn fail(&self, failure: &Failure, placeholder: String) -> mlua::Result<Json> {
        if self.debug {
            Ok(Json::String(placeholder))
        } else {
            Err(self.error(failure))
        }
    }

    /// Encodes `value`, which is at `level` if it is a table.
    fn value(&mut self, value: &Value, level: usize) -> mlua::Result<Json> {
        self.charge_node()?;
        match value {
            Value::Nil => Ok(Json::Null),
            Value::LightUserData(pointer) if pointer.0.is_null() => Ok(Json::Null),
            Value::Boolean(flag) => Ok(Json::Bool(*flag)),
            Value::Integer(integer) => Ok(Json::from(*integer)),
            Value::Number(number) => match finite(*number) {
                Some(json) => Ok(json),
                None if self.debug => Ok(Json::String(non_finite_text(*number))),
                None => Err(self.error(&Failure::NotFinite)),
            },
            Value::String(text) => {
                self.charge_bytes(text.as_bytes().len())?;
                match text.to_str() {
                    Ok(text) => Ok(Json::String(text.to_owned())),
                    Err(_) if self.debug => Ok(Json::String(text.to_string_lossy())),
                    Err(_) => Err(self.error(&Failure::NotUtf8)),
                }
            }
            Value::Table(table) => self.table(table, level),
            other => {
                let kind = other.type_name();
                let placeholder = match other {
                    Value::Function(_) | Value::Thread(_) => format!("[{kind}]"),
                    _ => "[userdata]".to_owned(),
                };
                self.fail(&Failure::NotData(kind), placeholder)
            }
        }
    }

    /// Encodes `table` at `level`, reading through a sealed view to its
    /// data table, which counts for the cycle and depth checks instead.
    fn table(&mut self, table: &Table, level: usize) -> mlua::Result<Json> {
        let data = match self.resolve(table)? {
            Resolved::Data(data) => data,
            Resolved::Unreadable(label) => {
                let placeholder = match &label {
                    Some(label) => format!("[cannot read: {label}]"),
                    None => "[cannot read]".to_owned(),
                };
                return self.fail(&Failure::Unreadable(label), placeholder);
            }
            Resolved::Loop => return self.fail(&Failure::Cycle, "[cycle]".to_owned()),
        };
        let pointer = data.to_pointer();
        if self.tables.contains(&pointer) {
            return self.fail(&Failure::Cycle, "[cycle]".to_owned());
        }
        if level > DEPTH_LIMIT {
            return self.fail(&Failure::TooDeep, "[too deep]".to_owned());
        }
        let Entries { keys, held } = self.keys_of(&data)?;
        let shape = shape(&keys, self.is_marked(&data));
        self.tables.push(pointer);
        self.held += held;
        let encoded = match shape {
            Shape::List => self.list(&data, keys, level).map(Json::Array),
            Shape::Object => self.object(&data, keys, level),
            Shape::Broken(_) if self.debug => self.fallback(&data, keys, level),
            Shape::Broken(rule) => Err(self.error(&Failure::Keys(rule))),
        };
        self.held -= held;
        self.tables.pop();
        encoded
    }

    /// The keys of `data` in visiting order. A table whose reference-holding
    /// keys, added to those the tables on the path already hold, would pass
    /// `HELD_KEY_LIMIT` is refused, in debug mode too.
    fn keys_of(&self, data: &Table) -> mlua::Result<Entries> {
        entries(data, HELD_KEY_LIMIT.saturating_sub(self.held))?.ok_or_else(|| self.too_large())
    }

    fn is_marked(&self, table: &Table) -> bool {
        table
            .metatable()
            .is_some_and(|metatable| metatable == self.marker)
    }

    /// Encodes the values of a list's `keys`, 1 to n in order. Each value is
    /// read from `data` when its turn comes, so only one holds a reference
    /// at a time.
    fn list(&mut self, data: &Table, keys: Vec<Value>, level: usize) -> mlua::Result<Vec<Json>> {
        let mut items = Vec::with_capacity(keys.len());
        for key in keys {
            let value: Value = data.raw_get(key.clone())?;
            self.keys.push(key);
            let item = self.value(&value, level + 1);
            self.keys.pop();
            items.push(item?);
        }
        Ok(items)
    }

    /// Encodes an object whose keys are all strings, checking every key's
    /// text before reading or encoding any value.
    fn object(&mut self, data: &Table, keys: Vec<Value>, level: usize) -> mlua::Result<Json> {
        let mut names = Vec::with_capacity(keys.len());
        for key in &keys {
            let Value::String(name) = key else {
                continue;
            };
            self.charge_bytes(name.as_bytes().len())?;
            match name.to_str() {
                Ok(text) => names.push(text.to_owned()),
                Err(_) if self.debug => names.push(name.to_string_lossy()),
                Err(_) => {
                    self.keys.push(key.clone());
                    return Err(self.error(&Failure::NotUtf8));
                }
            }
        }
        let mut object = Map::new();
        for (name, key) in names.into_iter().zip(keys) {
            let value: Value = data.raw_get(key.clone())?;
            self.keys.push(key);
            let encoded = self.value(&value, level + 1);
            self.keys.pop();
            object.insert(name, encoded?);
        }
        Ok(Json::Object(object))
    }

    /// Debug mode's object for a table that breaks the key rules: each
    /// boolean, number, and string key as text, later keys in visiting
    /// order replacing earlier ones with the same text, and every other
    /// key left out.
    fn fallback(&mut self, data: &Table, keys: Vec<Value>, level: usize) -> mlua::Result<Json> {
        let mut object = Map::new();
        for key in keys {
            let name = match &key {
                Value::Boolean(flag) => flag.to_string(),
                Value::Integer(integer) => integer.to_string(),
                Value::Number(number) => finite(*number)
                    .map_or_else(|| non_finite_text(*number), |json| json.to_string()),
                Value::String(name) => name.to_string_lossy(),
                _ => continue,
            };
            self.charge_bytes(name.len())?;
            let value: Value = data.raw_get(key)?;
            let encoded = self.value(&value, level + 1)?;
            object.insert(name, encoded);
        }
        Ok(Json::Object(object))
    }

    /// The items of `json.encode_lines`'s list argument, each encoded at
    /// level 1 while the list stays on the cycle path.
    fn items(&mut self, list: &Table) -> mlua::Result<Vec<Json>> {
        let data = match self.resolve(list)? {
            Resolved::Data(data) => data,
            Resolved::Unreadable(label) => return Err(self.error(&Failure::Unreadable(label))),
            Resolved::Loop => return Err(self.error(&Failure::Cycle)),
        };
        // Only integer keys reach the list below, and they hold no
        // reference, so `held` has nothing to add for this table.
        let keys = self.keys_of(&data)?.keys;
        let marked = self.is_marked(&data);
        let has_items = keys.iter().any(|key| matches!(key, Value::Integer(_)));
        if let (false, false, Some(key)) = (marked, has_items, keys.first()) {
            return Err(refusal(format!(
                "{} expects a list, got a table with a {} key",
                self.function,
                key.type_name()
            )));
        }
        if let Shape::Broken(rule) = shape(&keys, marked) {
            return Err(self.error(&Failure::Keys(rule)));
        }
        self.tables.push(data.to_pointer());
        let items = self.list(&data, keys, 0);
        self.tables.pop();
        items
    }
}

impl Encoder {
    /// Reads through sealed views. A sealed view is a table with no entries
    /// of its own whose metatable has both `__index` and `__metatable`: a
    /// table `__index` is its data, read by these same rules, and a
    /// function `__index` cannot be read, labelled by a string
    /// `__metatable`. Any other table is its own data. Each hop to another
    /// view spends one value of the budget, and the views already passed
    /// sit in a set, so a long chain costs time in proportion to its
    /// length.
    fn resolve(&mut self, table: &Table) -> mlua::Result<Resolved> {
        let mut current = table.clone();
        let mut views = HashSet::new();
        loop {
            let Some(metatable) = current.metatable() else {
                return Ok(Resolved::Data(current));
            };
            let label: Value = metatable.raw_get("__metatable")?;
            if label.is_nil()
                || current
                    .pairs::<Value, Value>()
                    .next()
                    .transpose()?
                    .is_some()
            {
                return Ok(Resolved::Data(current));
            }
            match metatable.raw_get::<Value>("__index")? {
                Value::Table(data) => {
                    self.charge_node()?;
                    views.insert(current.to_pointer());
                    if views.contains(&data.to_pointer()) {
                        return Ok(Resolved::Loop);
                    }
                    current = data;
                }
                Value::Function(_) => {
                    let label = match label {
                        Value::String(label) => Some(label.to_string_lossy()),
                        _ => None,
                    };
                    return Ok(Resolved::Unreadable(label));
                }
                _ => return Ok(Resolved::Data(current)),
            }
        }
    }
}

/// The JSON number `number` encodes as: an integer when it has no
/// fractional part and lies in `i64`'s range, so `-0.0` is `0`, and
/// otherwise the shortest text that reads back exactly. `None` for NaN
/// and the infinities.
fn finite(number: f64) -> Option<Json> {
    /// 2^63, one past `i64::MAX`.
    const ABOVE_MAX: f64 = 9_223_372_036_854_775_808.0;
    if number.fract() == 0.0 && (-ABOVE_MAX..ABOVE_MAX).contains(&number) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the float is a whole number inside i64's range"
        )]
        let whole = number as i64;
        return Some(Json::from(whole));
    }
    Number::from_f64(number).map(Json::Number)
}

/// Debug mode's text for a number with no JSON form.
fn non_finite_text(number: f64) -> String {
    if number.is_nan() {
        "NaN".to_owned()
    } else if number > 0.0 {
        "Infinity".to_owned()
    } else {
        "-Infinity".to_owned()
    }
}
