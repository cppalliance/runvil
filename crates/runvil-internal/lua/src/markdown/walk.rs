//! Reading a list of blocks from a Lua value.
//!
//! A list of blocks is whatever the author passes: what `markdown.parse`
//! returned, one section's `blocks`, a list read back from `var`, or
//! tables built by hand. [`Walk`] reads one the way `json.encode` reads a
//! value, never calling into Lua: it reads through sealed views, takes a
//! list as items 1, 2, and so on up to the first nil, refuses a list on
//! its own path and one nested too deep, and counts every block it visits
//! against a budget, since a table shared by many parents is visited once
//! for each.
//!
//! A failure names the function the walk serves and the place it stopped,
//! such as `blocks[2].blocks[1]`.

use std::ffi::c_void;
use std::fmt;

use mlua::{Lua, LuaString, Table, Value};

use crate::error::author_refusal;
use crate::proxy::{Unsealed, unseal};
use crate::walk_budget::WalkBudget;

/// The most lists deep that blocks may nest, the list given being level 1:
/// the depth `json.encode` accepts.
const DEPTH_LIMIT: usize = 127;

/// One walk over a list of blocks, for one call of a `markdown` function.
pub(super) struct Walk {
    /// The function the walk serves, which starts every failure's text.
    function: &'static str,
    /// What is left of the work the walk may do.
    budget: WalkBudget,
    /// The data table of each list open on the path, outermost first.
    open: Vec<*const c_void>,
    /// The position of the block being read in each list on the path.
    at: Vec<usize>,
}

/// One block of a list.
pub(super) struct Entry {
    /// The table as it sits in the list: what a lookup returns.
    pub(super) found: Table,
    /// The table its fields are read from, with any view read through.
    data: Table,
}

impl Entry {
    /// The field `name`, read without calling into Lua.
    pub(super) fn field(&self, name: &str) -> mlua::Result<Value> {
        self.data.raw_get(name)
    }

    /// The field `name` when it is a string, and `None` otherwise.
    pub(super) fn string(&self, name: &str) -> mlua::Result<Option<LuaString>> {
        Ok(match self.field(name)? {
            Value::String(text) => Some(text),
            _ => None,
        })
    }

    /// Whether the field `name` is the string `text`.
    pub(super) fn is(&self, name: &str, text: &str) -> mlua::Result<bool> {
        Ok(self
            .string(name)?
            .is_some_and(|field| field.as_bytes() == text.as_bytes()))
    }

    /// The block's `line`, when it holds a whole number.
    pub(super) fn line(&self) -> mlua::Result<Option<i64>> {
        Ok(whole(&self.field("line")?))
    }
}

/// The integer `value` holds, when it is a whole number an `i64` can hold.
pub(super) fn whole(value: &Value) -> Option<i64> {
    /// 2^63, one past `i64::MAX`.
    const ABOVE_MAX: f64 = 9_223_372_036_854_775_808.0;
    match value {
        Value::Integer(integer) => Some(*integer),
        Value::Number(number)
            if number.fract() == 0.0 && (-ABOVE_MAX..ABOVE_MAX).contains(number) =>
        {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the float is a whole number inside i64's range"
            )]
            let whole = *number as i64;
            Some(whole)
        }
        _ => None,
    }
}

impl Walk {
    /// A walk for one call of `function` on `lua`.
    pub(super) fn new(lua: &Lua, function: &'static str) -> Self {
        Self {
            function,
            budget: WalkBudget::for_vm(lua),
            open: Vec::new(),
            at: Vec::new(),
        }
    }

    /// The failure `<function>: <message>`.
    pub(super) fn refuse(&self, message: impl fmt::Display) -> mlua::Error {
        author_refusal(format!("{}: {message}", self.function))
    }

    /// Spends one visit of the budget.
    fn charge(&mut self) -> mlua::Result<()> {
        self.budget
            .charge_node()
            .map_err(|_| self.refuse("blocks are too large to walk"))
    }

    /// Spends `count` bytes of text of the budget.
    fn charge_bytes(&mut self, count: usize) -> mlua::Result<()> {
        self.budget
            .charge_bytes(count)
            .map_err(|_| self.refuse("blocks are too large to walk"))
    }

    /// The name of the block being read, such as `blocks[2].blocks[1]`.
    /// It is empty when no block is being read.
    fn block_name(&self) -> String {
        self.at
            .iter()
            .map(|index| format!("blocks[{index}]"))
            .collect::<Vec<_>>()
            .join(".")
    }

    /// The name of the list a block holds, such as `blocks[2].blocks`, or
    /// `blocks` for the list the walk was given.
    fn list_name(&self) -> String {
        if self.at.is_empty() {
            "blocks".to_owned()
        } else {
            format!("{}.blocks", self.block_name())
        }
    }

    /// The table `table` stands for, with any view read through, for the
    /// value `name` names.
    fn data(&mut self, table: &Table, name: &dyn Fn(&Self) -> String) -> mlua::Result<Table> {
        let mut hops = 0_usize;
        let unsealed = unseal(table, || {
            hops += 1;
            Ok(())
        })?;
        for _ in 0..hops {
            self.charge()?;
        }
        match unsealed {
            Unsealed::Data(data) => Ok(data),
            Unsealed::Unreadable(Some(label)) => Err(self.refuse(format!(
                "{} cannot be read ({label}); copy the fields you need into a plain table",
                name(self)
            ))),
            Unsealed::Unreadable(None) => Err(self.refuse(format!(
                "{} cannot be read; copy the fields you need into a plain table",
                name(self)
            ))),
            Unsealed::Loop => Err(self.refuse(format!("{} contains itself", name(self)))),
        }
    }

    /// Opens the list `value` and returns the table its items are read
    /// from. [`close`](Self::close) ends it.
    ///
    /// # Errors
    /// Returns the refusal of a value that is not a table, of a block
    /// given where a list belongs, of a list that cannot be read, of one
    /// that is on its own path, and of one nested past the depth limit.
    pub(super) fn open(&mut self, value: &Value) -> mlua::Result<Table> {
        let Value::Table(table) = value else {
            return Err(self.refuse(format!(
                "{} must be a list of blocks, got {}",
                self.list_name(),
                value.type_name()
            )));
        };
        let data = self.data(table, &Self::list_name)?;
        if let Value::String(kind) = data.raw_get::<Value>("kind")? {
            let kind = kind.to_string_lossy();
            let got = if kind == "section" {
                "a section; pass its blocks field".to_owned()
            } else {
                format!("a {kind} block")
            };
            return Err(self.refuse(format!(
                "{} must be a list of blocks, got {got}",
                self.list_name()
            )));
        }
        let pointer = data.to_pointer();
        if self.open.contains(&pointer) {
            return Err(self.refuse(format!("{} contains itself", self.list_name())));
        }
        if self.open.len() >= DEPTH_LIMIT {
            return Err(self.refuse(format!(
                "{} is nested more than {DEPTH_LIMIT} levels deep",
                self.list_name()
            )));
        }
        self.open.push(pointer);
        Ok(data)
    }

    /// Ends the list the last [`open`](Self::open) started.
    pub(super) fn close(&mut self) {
        self.open.pop();
    }

    /// Visits each block of `list`, an open list, in order: items 1, 2,
    /// and so on up to the first nil.
    ///
    /// # Errors
    /// Returns the refusal of an item that is not a table or that cannot
    /// be read, and the failure of `visit`.
    pub(super) fn each(
        &mut self,
        list: &Table,
        visit: &mut dyn FnMut(&mut Self, &Entry) -> mlua::Result<()>,
    ) -> mlua::Result<()> {
        for index in 1.. {
            let item: Value = list.raw_get(index)?;
            self.at.push(index);
            let found = match item {
                Value::Nil => {
                    self.at.pop();
                    break;
                }
                Value::Table(found) => found,
                other => {
                    return Err(self.refuse(format!(
                        "{} must be a table, got {}",
                        self.block_name(),
                        other.type_name()
                    )));
                }
            };
            self.charge()?;
            let data = self.data(&found, &Self::block_name)?;
            visit(self, &Entry { found, data })?;
            self.at.pop();
        }
        Ok(())
    }

    /// Reads `value`, the field `field` of the block being read, as text.
    ///
    /// # Errors
    /// Returns the refusal of a value that is not a string, and of a
    /// string that is not valid UTF-8.
    pub(super) fn text(&mut self, value: &Value, field: &str) -> mlua::Result<String> {
        let Value::String(text) = value else {
            return Err(self.refuse(format!(
                "{}.{field} must be a string, got {}",
                self.block_name(),
                value.type_name()
            )));
        };
        self.charge_bytes(text.as_bytes().len())?;
        match text.to_str() {
            Ok(text) => Ok(text.to_owned()),
            Err(_) => Err(self.refuse(format!("{}.{field} is not valid UTF-8", self.block_name()))),
        }
    }
}
