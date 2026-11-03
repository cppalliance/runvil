//! Tests for reading a section's current model through the VM and the shared model set.

use std::num::NonZeroU32;
use std::sync::Arc;

use super::*;
use crate::lua::{SectionVm, ToolSet};
use crate::test_support::fresh_access;
use crate::test_support::recording::null_emitter;
use crate::test_support::scenario::TEST_SEED;
use crate::{Result, error};
use runvil_model_client::model::ModelInvocation;
use runvil_types::emitter::Emitter;
use runvil_types::untrusted::GuardNonce;
use serde_json::json;

/// Compiles one Lua chunk for a section VM drive.
#[track_caller]
fn chunk(source: &str) -> runvil_lua::LuaProgram {
    runvil_lua::LuaProgram::compile(
        source,
        "chunk",
        NonZeroU32::new(1).expect("compile source line is non-zero"),
        &null_emitter(),
        "Section",
    )
    .expect("test Lua must compile")
}

fn ctx(window: u32) -> NonZeroU32 {
    NonZeroU32::new(window).expect("test context window is non-zero")
}

fn gateway_id(name: &str) -> ModelId {
    ModelId::gateway(name).expect("test model alias is valid")
}

/// A bound role as the run's fill records it: label, description, resolved
/// identity, and the thinking switch as the frozen invocation.
fn bound_role(
    label: &str,
    description: &str,
    model: &str,
    window: u32,
    thinking: Option<bool>,
) -> ModelBinding {
    ModelBinding::new(
        label,
        description,
        gateway_id(model),
        ModelInvocation::new().with_thinking(thinking),
        ctx(window),
    )
}

/// Shares a model set the way the run shares its own.
fn shared_models(bindings: Vec<ModelBinding>) -> Arc<ModelSet> {
    let mut set = ModelSet::default();
    for binding in bindings {
        set.push(binding);
    }
    Arc::new(set)
}

/// Shares an empty tool set.
fn shared_tools() -> Arc<ToolSet> {
    Arc::new(ToolSet::default())
}

/// A section VM over `models` with empty Engine values injected.
fn section_vm_with_models(
    models: &Arc<ModelSet>,
    emitter: &Emitter,
    section: &str,
) -> Result<SectionVm> {
    SectionVm::new_for_section(
        &GuardNonce::from_seed(&TEST_SEED),
        &shared_tools(),
        models,
        emitter,
        section,
    )
    .map_err(error::into_universal)?
    .inject_values("", &json!({}), &fresh_access())
    .map_err(|failure| error::into_universal(failure.teardown(emitter, section)))
}

/// Reads the section's current model the way the Engine's read path does.
fn resolve_section_model(vm: &SectionVm) -> Result<Option<ModelBinding>> {
    vm.current_model().map_err(error::into_universal)
}

#[path = "tests-always.rs"]
mod always;
#[path = "tests-integration.rs"]
mod integration;
