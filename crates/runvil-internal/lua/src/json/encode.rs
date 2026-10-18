//! `json.encode` and `json.encode_lines`: Lua values to compact JSON text.
//!
//! The walk never calls back into Lua. It reads entries with
//! `Table::pairs`, which skips `__pairs`, metatables with
//! `Table::metatable`, which ignores `__metatable`, and fields of a
//! metatable with `raw_get`, and it compares tables by pointer. It builds a
//! `serde_json` tree on the Rust heap, whose objects sort their keys, and
//! writes it in one pass. In debug mode each failure inside the value
//! becomes a placeholder string, so any value encodes.

use std::ffi::c_void;

use mlua::{Lua, LuaSerdeExt, LuaString, Table, Value};
use serde_json::{Map, Number, Value as Json};

use super::keys::{Rule, Shape, entries, shape};
use super::refusal;
use crate::proxy::{key_path, key_text};

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
    let mut debug = false;
    for (key, value) in entries(table)? {
        if !matches!(&key, Value::String(name) if name == "debug") {
            return Err(refusal(format!(
                "{function}: unknown option {}",
                key_text(&key)
            )));
        }
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
/// for error messages, and the tables on that path for cycle detection.
struct Encoder {
    function: &'static str,
    debug: bool,
    marker: Table,
    keys: Vec<Value>,
    tables: Vec<*const c_void>,
}

impl Encoder {
    fn new(lua: &Lua, function: &'static str, debug: bool) -> Self {
        Self {
            function,
            debug,
            marker: lua.array_metatable(),
            keys: Vec::new(),
            tables: Vec::new(),
        }
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
            Value::String(text) => match text.to_str() {
                Ok(text) => Ok(Json::String(text.to_owned())),
                Err(_) if self.debug => Ok(Json::String(text.to_string_lossy())),
                Err(_) => Err(self.error(&Failure::NotUtf8)),
            },
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
        let data = match resolve(table)? {
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
        let entries = entries(&data)?;
        let shape = shape(&entries, self.is_marked(&data));
        self.tables.push(pointer);
        let encoded = match shape {
            Shape::List => self.list(entries, level).map(Json::Array),
            Shape::Object => self.object(entries, level),
            Shape::Broken(_) if self.debug => self.fallback(entries, level),
            Shape::Broken(rule) => Err(self.error(&Failure::Keys(rule))),
        };
        self.tables.pop();
        encoded
    }

    fn is_marked(&self, table: &Table) -> bool {
        table
            .metatable()
            .is_some_and(|metatable| metatable == self.marker)
    }

    /// Encodes the values of a list's entries, keys 1 to n in order.
    fn list(&mut self, entries: Vec<(Value, Value)>, level: usize) -> mlua::Result<Vec<Json>> {
        let mut items = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            self.keys.push(key);
            let item = self.value(&value, level + 1);
            self.keys.pop();
            items.push(item?);
        }
        Ok(items)
    }

    /// Encodes an object whose keys are all strings, checking every key's
    /// text before encoding any value.
    fn object(&mut self, entries: Vec<(Value, Value)>, level: usize) -> mlua::Result<Json> {
        let mut names = Vec::with_capacity(entries.len());
        for (key, _) in &entries {
            let Value::String(name) = key else {
                continue;
            };
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
        for (name, (key, value)) in names.into_iter().zip(entries) {
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
    fn fallback(&mut self, entries: Vec<(Value, Value)>, level: usize) -> mlua::Result<Json> {
        let mut object = Map::new();
        for (key, value) in entries {
            let name = match &key {
                Value::Boolean(flag) => flag.to_string(),
                Value::Integer(integer) => integer.to_string(),
                Value::Number(number) => finite(*number)
                    .map_or_else(|| non_finite_text(*number), |json| json.to_string()),
                Value::String(name) => name.to_string_lossy(),
                _ => continue,
            };
            let encoded = self.value(&value, level + 1)?;
            object.insert(name, encoded);
        }
        Ok(Json::Object(object))
    }

    /// The items of `json.encode_lines`'s list argument, each encoded at
    /// level 1 while the list stays on the cycle path.
    fn items(&mut self, list: &Table) -> mlua::Result<Vec<Json>> {
        let data = match resolve(list)? {
            Resolved::Data(data) => data,
            Resolved::Unreadable(label) => return Err(self.error(&Failure::Unreadable(label))),
            Resolved::Loop => return Err(self.error(&Failure::Cycle)),
        };
        let entries = entries(&data)?;
        let marked = self.is_marked(&data);
        let has_items = entries
            .iter()
            .any(|(key, _)| matches!(key, Value::Integer(_)));
        if let (false, false, Some((key, _))) = (marked, has_items, entries.first()) {
            return Err(refusal(format!(
                "{} expects a list, got a table with a {} key",
                self.function,
                key.type_name()
            )));
        }
        if let Shape::Broken(rule) = shape(&entries, marked) {
            return Err(self.error(&Failure::Keys(rule)));
        }
        self.tables.push(data.to_pointer());
        let items = self.list(entries, 0);
        self.tables.pop();
        items
    }
}

/// Reads through sealed views. A sealed view is a table with no entries of
/// its own whose metatable has both `__index` and `__metatable`: a table
/// `__index` is its data, read by these same rules, and a function
/// `__index` cannot be read, labelled by a string `__metatable`. Any other
/// table is its own data.
fn resolve(table: &Table) -> mlua::Result<Resolved> {
    let mut current = table.clone();
    let mut views = Vec::new();
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
                views.push(current.to_pointer());
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
