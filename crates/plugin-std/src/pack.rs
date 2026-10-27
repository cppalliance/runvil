//! The crate's Plugin, [`install`], the `std` Pack's `PackInfo`, the
//! `construct` that builds its one shared object, and the stub every tool
//! answers with in this build.

use std::sync::Arc;

use runvil_plugin::{
    HostServices, InstallError, Pack, PackFuture, PackId, PackInfo, PackRegistry, ToolContext,
    ToolError, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};

use crate::spec::{self, Definition};

/// What a call whose arguments match the schema answers in this build.
///
/// The text says the implementation is missing rather than that the call
/// was wrong, because a model that reads it should stop calling the tool.
/// It also says a repeat call is safe, so a model that retries anyway
/// learns the outcome is the same rather than assuming the call was
/// malformed. `<name>` is the tool's wire name.
pub const NOT_IMPLEMENTED: &str = "The <name> tool is not implemented in this build. The \
    implementation is missing and nothing in this session will change that. Calling <name> again \
    is safe, but it will return this same result every time, whatever the arguments. Carry on \
    without it.";

/// The Pack's `PackInfo`.
///
/// Its FQN is `runvil/std`, so its Pack id is `std`. It has no prelude,
/// needs no per-run service, and takes no configuration but `null` or
/// `{}`.
const STD: PackInfo =
    PackInfo::new("runvil/std", construct).description("Read, write, search, and delete files");

/// The Plugin: inserts the `std` Pack into `packs` with `config["std"]`,
/// which is `null` when `config` has no `std` key.
///
/// # Errors
/// Returns the registry's [`InstallError`], such as
/// [`InstallError::DuplicatePack`] when `packs` holds `runvil/std`
/// already.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the Plugin signature fixes the argument types"
)]
pub fn install(packs: &mut PackRegistry, config: Value) -> Result<(), InstallError> {
    packs.insert(STD, config["std"].clone())
}

/// The one object every run shares: its tool list.
#[derive(Debug)]
struct Std {
    tools: Vec<ToolInfo>,
}

/// Builds the Pack under the Pack id `name`: refuses any configuration but
/// `null` or `{}` and names each tool `<name>/<segment>`.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the PackInfo construct signature fixes the argument types"
)]
fn construct(
    name: &PackId,
    config: Value,
    _services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    if !(config.is_null() || config == json!({})) {
        return Err(ToolError::message("std takes no configuration"));
    }
    let tools = spec::all()?
        .iter()
        .map(|definition| {
            let id = ToolId::parse(&format!("{name}/{}", definition.segment)).map_err(|e| {
                ToolError::message(format!(
                    "std could not name its {} tool",
                    definition.segment
                ))
                .with_source(e)
            })?;
            let info = ToolInfo::new(
                id,
                definition.description.clone(),
                definition.parameters.clone(),
            )
            .wire_name(definition.wire_name.clone());
            Ok(info)
        })
        .collect::<Result<Vec<_>, ToolError>>()?;
    Ok(Arc::new(Std { tools }))
}

/// The stub text for the tool the model calls `wire_name`.
fn stub(wire_name: &str) -> String {
    NOT_IMPLEMENTED.replace("<name>", wire_name)
}

/// The definition of the tool whose id ends in `segment`.
fn definition(segment: &str) -> Option<&'static Definition> {
    spec::all()
        .ok()?
        .iter()
        .find(|definition| definition.segment == segment)
}

impl Pack for Std {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, ToolError>> {
        Box::pin(async move {
            let name = cx.tool().name();
            let Some(definition) = definition(name) else {
                return Err(ToolError::message(format!("std has no tool named {name}")));
            };
            crate::check::check(&definition.wire_name, &definition.parameters, &args)?;
            Ok(ToolOutput::trusted(stub(&definition.wire_name)))
        })
    }
}

#[cfg(test)]
#[path = "pack-tests.rs"]
mod tests;
