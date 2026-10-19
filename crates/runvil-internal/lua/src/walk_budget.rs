//! The work one conversion of a Lua value may do before it is refused.
//!
//! A walk that reads a shared table once per reference, or copies a
//! repeated string once per occurrence, can build a result far larger than
//! the Lua heap that holds its input, and a single Rust callback runs
//! without the instruction hook ever firing. The budget is sized from the
//! heap ceiling so that only expansion beyond the heap is refused.

use std::ffi::c_void;

use mlua::{Lua, Value};

use crate::vm::DEFAULT_LUA_MEMORY_BYTES;

/// The smallest cost of one value in the Lua heap, rounded down: an array
/// slot takes at least about 9 bytes, so an unshared value that fits the
/// heap has at most `ceiling / 9` values, and `ceiling / 8` leaves room
/// above that.
const BYTES_PER_NODE: usize = 8;

/// The most table levels `check_value` descends, the top-level table being
/// level 1. A table nested past it is not counted and converts as it does
/// today.
const CHECK_DEPTH_LIMIT: usize = 200;

/// The most keys one encode path may hold as Lua references at once:
/// string, table, function, thread, and userdata keys, each of which takes
/// a slot on the auxiliary reference stack until it is dropped. Integer,
/// number, and boolean keys hold no slot, so lists are exempt. A Lua stack
/// caps at 1,000,000 slots and mlua panics when a reference cannot get
/// one, so 10 times headroom below that cap covers the references the
/// path itself keeps alive.
pub(crate) const HELD_KEY_LIMIT: usize = 100_000;

/// The walk went past its budget.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TooLarge;

/// The VM's Lua heap limit, kept as Lua app data because mlua has no getter
/// for the memory limit.
pub(crate) struct HeapCeiling(pub usize);

/// What one walk has left to spend: values visited and bytes of text read.
pub(crate) struct WalkBudget {
    nodes: usize,
    bytes: usize,
}

impl WalkBudget {
    /// The budget for a walk on `lua`: `ceiling / 8` values and `ceiling`
    /// bytes of text, where the ceiling is the VM's Lua heap limit. At the
    /// default 64 MiB that is 8,388,608 values, which covers the most
    /// values an 8 MiB `json.decode` input can produce, so decoding and
    /// then encoding stays whole. A VM with no recorded ceiling gets the
    /// default, so no caller runs unbounded.
    pub(crate) fn for_vm(lua: &Lua) -> Self {
        let ceiling = lua
            .app_data_ref::<HeapCeiling>()
            .map_or(DEFAULT_LUA_MEMORY_BYTES, |ceiling| ceiling.0);
        Self {
            nodes: ceiling / BYTES_PER_NODE,
            bytes: ceiling,
        }
    }

    /// Spends one value, failing once none are left.
    pub(crate) fn charge_node(&mut self) -> Result<(), TooLarge> {
        self.nodes = self.nodes.checked_sub(1).ok_or(TooLarge)?;
        Ok(())
    }

    /// Spends `n` bytes of text, failing when fewer than `n` are left.
    pub(crate) fn charge_bytes(&mut self, n: usize) -> Result<(), TooLarge> {
        self.bytes = self.bytes.checked_sub(n).ok_or(TooLarge)?;
        Ok(())
    }

    /// Spends the budget on every value `value` holds, as a pre-walk for a
    /// caller that converts the value with `from_value`, which reads a
    /// shared table once per reference and builds the result on the Rust
    /// heap before any Lua allocation. The walk reads each table lazily
    /// with `Table::pairs`, so it never holds more than one entry of a
    /// table, and it stops at the first value past the budget. It counts
    /// every value it visits, repeats of a shared table included, and the
    /// bytes of every string value and string key. It does not descend into
    /// a table already on the path to it, or one at a level past
    /// `CHECK_DEPTH_LIMIT`; the conversion that follows keeps its own
    /// behavior for a cyclic or very deep value.
    pub(crate) fn check_value(&mut self, value: &Value) -> Result<(), TooLarge> {
        self.check_at(value, &mut Vec::new())
    }

    /// Spends the budget on `value` and, for a table, on its entries.
    /// `path` holds the tables that lead to `value`.
    fn check_at(&mut self, value: &Value, path: &mut Vec<*const c_void>) -> Result<(), TooLarge> {
        self.charge_node()?;
        match value {
            Value::String(text) => self.charge_bytes(text.as_bytes().len()),
            Value::Table(table) => {
                let pointer = table.to_pointer();
                if path.len() >= CHECK_DEPTH_LIMIT || path.contains(&pointer) {
                    return Ok(());
                }
                path.push(pointer);
                // An entry that cannot be read ends the walk of this table,
                // and the conversion that follows reports it as it does today.
                for (key, value) in table.pairs::<Value, Value>().map_while(Result::ok) {
                    if let Value::String(name) = &key {
                        self.charge_bytes(name.as_bytes().len())?;
                    }
                    self.check_at(&value, path)?;
                }
                path.pop();
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
