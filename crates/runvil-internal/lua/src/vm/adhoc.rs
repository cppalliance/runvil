//! The adhoc tools a section VM defines: each one's binding, looked up by
//! wire name or by id.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::bindings::ToolBinding;
use crate::error::{Error, Result};

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
    /// Returns [`Error::Internal`] if the entries lock was poisoned.
    fn entries(&self) -> Result<MutexGuard<'_, Vec<ToolBinding>>> {
        self.entries
            .lock()
            .map_err(|_| Error::internal("adhoc tools registry was poisoned"))
    }

    /// Registers the adhoc tool `binding` binds.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] if the entries lock was poisoned.
    pub(crate) fn register(&self, binding: ToolBinding) -> Result<()> {
        self.entries()?.push(binding);
        Ok(())
    }

    /// Returns the binding of the adhoc tool whose id is `name` when it
    /// holds a `/`, else whose wire name is `name`.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] if the entries lock was poisoned.
    pub fn binding(&self, name: &str) -> Result<Option<ToolBinding>> {
        let by_id = name.contains('/');
        Ok(self
            .entries()?
            .iter()
            .find(|binding| {
                if by_id {
                    binding.id().to_string() == name
                } else {
                    binding.alias() == name
                }
            })
            .cloned())
    }

    /// Returns every adhoc tool's binding, in id order.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] if the entries lock was poisoned.
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
