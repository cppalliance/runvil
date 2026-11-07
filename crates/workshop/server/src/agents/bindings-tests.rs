//! Host binding tests: a run's setup takes the selection and the granted
//! roots.

use std::sync::Arc;

use harness::record::MemoryRecorder;
use tokio::sync::watch;
use workshop_menu::{CatalogBus, MenuBus};
use workshop_registry::{WorkspaceRoots, WorkspaceRootsAdapter};

use super::*;
use crate::agents::tests::OfflineBroker;

/// A setup with nothing selected and no roots.
fn empty_setup() -> RunSetup {
    RunSetup::new(Arc::new(MemoryRecorder::new()), Arc::new(OfflineBroker))
}

#[test]
fn a_runs_setup_takes_the_selection_and_the_granted_roots() {
    let registry = Registry::new();
    let empty = with_host_state(&registry, empty_setup());
    assert_eq!(
        empty.selected_model, None,
        "an unregistered menu serves no selection"
    );
    assert!(
        empty.workspace_roots.is_empty(),
        "an unregistered workspace serves no roots"
    );

    let catalog = CatalogBus::new();
    let menu = MenuBus::new(catalog.clone(), None);
    let _menu_guards = workshop_menu::register(&registry, &catalog, &menu);
    let registered = with_host_state(&registry, empty_setup());
    assert_eq!(
        registered.selected_model, None,
        "a registered menu with nothing selected still serves no selection"
    );

    catalog.publish(vec![serde_json::json!({ "id": "test-model" })]);
    menu.set_selected("test-model")
        .expect("the id is in the catalog");
    let dir = tempfile::TempDir::new().expect("tempdir");
    let granted = dir.path().to_path_buf();
    let _roots =
        registry.register_state::<dyn WorkspaceRoots>(Arc::new(WorkspaceRootsAdapter::new(
            {
                let granted = granted.clone();
                move || vec![granted.clone()]
            },
            || watch::channel(0).1,
        )));

    let setup = with_host_state(&registry, empty_setup());
    assert_eq!(setup.selected_model.as_deref(), Some("test-model"));
    assert_eq!(
        setup.workspace_roots,
        vec![granted],
        "the roots are read through the registry's WorkspaceRoots slot"
    );
}
