//! The adhoc tools a section VM defines: each one's binding, looked up by
//! wire name or by id.
//!
//! An adhoc tool, defined with `tools.adhoc`, runs inside the calling
//! block's coroutine, never as an Engine function. The VM keeps each
//! tool's binding; the handler sits in the VM's handler table. A call
//! takes two yields: the shim's `tool_call`, which the scheduler answers
//! with the handler itself, and the shim's `adhoc_tool_done` after the
//! handler ran, which carries its result for the scheduler to report.
//! Every suspending call the handler makes in between is an ordinary
//! yield of the same coroutine.

use std::sync::{Arc, Mutex, MutexGuard};

use runvil_types::tools::ToolId;

use crate::bindings::ToolBinding;
use crate::error::{self, Result};

/// The adhoc tools `tools.adhoc` defined on a section VM, in definition
/// order.
///
/// Each entry is the tool's binding, under its wire name, with no vendor,
/// and never describing tools. The tool's function sits in the VM's handler
/// table under the wire name. The entries are shared with the `tools.adhoc`
/// Lua callback, which must be `Send`, hence the `Mutex`; the VM is
/// single-threaded, so the lock never contends.
#[derive(Debug, Default, Clone)]
pub struct AdhocTools {
    entries: Arc<Mutex<Vec<ToolBinding>>>,
}

impl AdhocTools {
    /// Locks the registry.
    ///
    /// # Errors
    /// Returns an `Internal` failure if the entries lock was poisoned.
    fn entries(&self) -> Result<MutexGuard<'_, Vec<ToolBinding>>> {
        self.entries
            .lock()
            .map_err(|_| error::internal("adhoc tools registry was poisoned"))
    }

    /// Registers the adhoc tool `binding` binds.
    ///
    /// # Errors
    /// Returns an `Internal` failure if the entries lock was poisoned.
    pub(crate) fn register(&self, binding: ToolBinding) -> Result<()> {
        self.entries()?.push(binding);
        Ok(())
    }

    /// Returns the binding of the adhoc tool whose id is `id`.
    ///
    /// # Errors
    /// Returns an `Internal` failure if the entries lock was poisoned.
    pub fn by_id(&self, id: &ToolId) -> Result<Option<ToolBinding>> {
        Ok(self
            .entries()?
            .iter()
            .find(|binding| binding.id() == id)
            .cloned())
    }

    /// Returns the binding of the adhoc tool whose wire name is `name`.
    ///
    /// # Errors
    /// Returns an `Internal` failure if the entries lock was poisoned.
    pub fn by_wire_name(&self, name: &str) -> Result<Option<ToolBinding>> {
        Ok(self
            .entries()?
            .iter()
            .find(|binding| binding.alias() == name)
            .cloned())
    }

    /// Returns every adhoc tool's binding, in id order.
    ///
    /// # Errors
    /// Returns an `Internal` failure if the entries lock was poisoned.
    pub(crate) fn bindings(&self) -> Result<Vec<ToolBinding>> {
        let mut bindings = self.entries()?.clone();
        bindings.sort_by(|left, right| left.id().cmp(right.id()));
        Ok(bindings)
    }

    #[cfg(test)]
    pub(crate) fn entries_handle(&self) -> Arc<Mutex<Vec<ToolBinding>>> {
        Arc::clone(&self.entries)
    }
}
