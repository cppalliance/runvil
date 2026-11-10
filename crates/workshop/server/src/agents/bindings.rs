//! The Host state a conversation's run carries in its setup: the menu's
//! selected model and the workspace's granted roots, read through the
//! registry as the run starts, which the run's `ui()` and model
//! resolution read for the whole run.

use runvil::RunSetup;
use workshop_menu::MenuHandles;
use workshop_registry::{Registry, WorkspaceRoots};

/// `setup` with its selected model and workspace roots read from
/// `registry`: `selected_model` from the menu's retained workbench state
/// and the granted workspace roots from the registry's roots slot, so this
/// crate reads the workspace through the slot the workspace subsystem
/// registered. An unregistered subsystem's part reads as no selection or
/// no roots.
pub(super) fn with_host_state(registry: &Registry, setup: RunSetup) -> RunSetup {
    let selected_model = registry
        .state::<MenuHandles>()
        .and_then(|handles| handles.menu().latest())
        .and_then(|snapshot| snapshot.selected_model);
    let workspace_roots = registry
        .state::<dyn WorkspaceRoots>()
        .map_or_else(Vec::new, |roots| roots.granted_roots());
    RunSetup {
        selected_model,
        workspace_roots,
        ..setup
    }
}

#[cfg(test)]
#[path = "bindings-tests.rs"]
mod tests;
