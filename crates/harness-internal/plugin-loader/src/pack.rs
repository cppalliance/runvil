//! The Packs a Plugin's entry point inserts: the factory the registry
//! holds for each one, one run's instance of it, the functions an
//! RV_PACK_NEW's `RvPackNewArgs` carries, and the senders of RV_PACK_NEW
//! and RV_PACK_DELETE.

use std::ffi::c_void;
use std::mem;
use std::sync::Arc;

use runvil_plugin::{
    BoxFuture, Error, ErrorCode, Pack, PackFactory, PackId, ToolContext, ToolId, ToolInfo,
    ToolOutput, make_tool_error,
};
use runvil_plugin_sys::{
    RV_OK, RV_PACK_DELETE, RV_PACK_NEW, RvCall, RvPackNewArgs, RvStatus, RvStr, RvToolInfo,
};
use serde_json::Value;

use crate::call::Steps;
use crate::host::{is_segment, new_call};
use crate::plugin::Plugin;
use crate::session::{
    self, AddedTool, Field, PACK_NEW, RESERVED, Shape, Verb, answer, field, rv_str, shape,
    size_field,
};

#[cfg(test)]
#[path = "pack-tests.rs"]
mod tests;

const TOOL_INFO: Shape = shape!("add_tool", "info", "RvToolInfo");
const TOOL_NAME: Field = field!("the tool's name", 64);
const WIRE_NAME: Field = field!("the tool's wire name", 64);
const DESCRIPTION: Field = field!("the tool's description", 65536);
const PARAMETERS: Field = field!("the tool's parameter schema", 65536);

/// Makes each run's instance of one Pack that a Plugin inserted through
/// its entry point. It holds an `Arc` of the Plugin, so the Plugin stays
/// loaded while the registry, or the `Packs` built from it, holds the
/// Pack.
pub(crate) struct EntryFactory {
    plugin: Arc<Plugin>,
    /// The Pack's name, as the Plugin gave it to `insert_pack`.
    pack: String,
}

impl EntryFactory {
    /// The factory of the Pack `plugin` inserted under the name `pack`.
    pub(crate) fn new(plugin: Arc<Plugin>, pack: String) -> EntryFactory {
        EntryFactory { plugin, pack }
    }
}

impl PackFactory for EntryFactory {
    /// Sends RV_PACK_NEW and makes the run's instance from the tools the
    /// Plugin added.
    ///
    /// # Errors
    /// Returns the Plugin's refusal with its reason as the message, or a
    /// failure that names how the Plugin broke the contract or the status
    /// RV_PACK_NEW returned.
    fn new_pack(&self, id: &PackId) -> Result<Arc<dyn Pack>, Error> {
        let (instance, added) = send_pack_new(&self.plugin, &self.pack)?;
        // The instance owns `instance` from here, so a tool that cannot be
        // named below still gets its RV_PACK_DELETE.
        let mut pack = EntryPack {
            plugin: Arc::clone(&self.plugin),
            instance,
            tools: Vec::new(),
        };
        pack.tools = added
            .into_iter()
            .map(|tool| tool_info(id, tool))
            .collect::<Result<_, _>>()?;
        Ok(Arc::new(pack))
    }
}

/// The catalog entry of the tool `added` under the Pack id `id`.
fn tool_info(id: &PackId, added: AddedTool) -> Result<ToolInfo, Error> {
    let tool_id = ToolId::parse(&format!("{id}/{}", added.name)).map_err(|error| {
        make_tool_error(
            ErrorCode::ToolOther,
            format!(
                "the tool {} of the Pack {id} could not be named",
                added.name
            ),
        )
        .with_cause(&error)
    })?;
    let info = ToolInfo::new(tool_id, added.description, added.parameters);
    Ok(match added.wire_name {
        Some(wire_name) => info.wire_name(wire_name),
        None => info,
    })
}

/// One run's instance of a Pack that a Plugin's entry point made. It
/// holds an `Arc` of the Plugin, so the Plugin outlives it, and dropping
/// it sends RV_PACK_DELETE once.
pub(crate) struct EntryPack {
    plugin: Arc<Plugin>,
    /// The pointer RV_PACK_NEW set, which each of the instance's calls
    /// passes back and the loader never reads through.
    instance: *mut c_void,
    tools: Vec<ToolInfo>,
}

impl EntryPack {
    /// An `RvCall` that sends `verb` to this instance.
    pub(crate) fn new_call(&self, verb: u32) -> RvCall {
        RvCall {
            pack_run: self.instance,
            ..new_call(verb, self.plugin.instance().plugin())
        }
    }

    /// Sends `call` through the Plugin's entry point.
    ///
    /// # Safety
    /// `call` must follow the C contract for its verb, with this
    /// instance's pointers, and its `args` must stay valid for the call.
    pub(crate) unsafe fn send(&self, call: &mut RvCall) -> RvStatus {
        // SAFETY: inserting the Plugin required its entry point to follow
        // the contract and not unwind, and the caller vouches for `call`.
        unsafe { (self.plugin.instance().entry())(call) }
    }
}

impl Drop for EntryPack {
    fn drop(&mut self) {
        let mut call = self.new_call(RV_PACK_DELETE);
        // SAFETY: `instance` is the pointer a successful RV_PACK_NEW set,
        // and this drop is its only delete. Each tool call of the instance
        // borrows it, so every one of them has ended.
        let _status = session::closed(|| unsafe { self.send(&mut call) });
    }
}

// SAFETY: the `# Threads and lifetime` section of `runvil-plugin-sys` lets
// RV_PACK_DELETE and each tool call come on any thread, and the loader
// never reads through `instance`.
unsafe impl Send for EntryPack {}
// SAFETY: the same section lets tool calls of one Pack instance run at the
// same time, and `&self` only hands out the pointer those calls pass back.
unsafe impl Sync for EntryPack {}

impl Pack for EntryPack {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(Steps::new(self, cx.tool().name(), &args))
    }
}

/// Records the Plugin's refusal to make a Pack instance, validated;
/// returns RV_ERR_REFUSED, or RV_ERR_MISUSE for a `ctx` it doesn't answer
/// or a second refusal.
pub(crate) unsafe extern "C" fn refuse(ctx: *mut c_void, reason: RvStr) -> RvStatus {
    session::refuse(ctx, &PACK_NEW, reason)
}

/// Records one tool the instance adds, validated; returns RV_OK or
/// RV_ERR_MISUSE.
pub(crate) unsafe extern "C" fn add_tool(ctx: *mut c_void, info: *const RvToolInfo) -> RvStatus {
    answer(ctx, &PACK_NEW, |session| {
        // SAFETY: the contract makes `info` point at a readable
        // `RvToolInfo` during this call.
        let info = unsafe { TOOL_INFO.read(info) }?;
        if info.reserved != 0 {
            return Err(RESERVED);
        }
        // SAFETY: the contract makes each string valid for its length.
        let name = unsafe { TOOL_NAME.read(info.name) }?;
        if !is_segment(&name) {
            return Err("the tool's name is not one name segment, such as `fetch`");
        }
        if session.tools.iter().any(|tool| tool.name == name) {
            return Err("two tools of one Pack instance share a name");
        }
        // SAFETY: as above.
        let wire_name = unsafe { WIRE_NAME.read(info.wire_name) }?;
        if !wire_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err("the tool's wire name is not ASCII letters, digits, `_`, and `-`");
        }
        // SAFETY: as above.
        let description = unsafe { DESCRIPTION.read(info.description) }?;
        if description.is_empty() {
            return Err("the tool's description is empty");
        }
        // SAFETY: as above.
        let parameters = unsafe { PARAMETERS.read(info.parameters) }?;
        let parameters = serde_json::from_str::<Value>(&parameters)
            .ok()
            .filter(Value::is_object)
            .ok_or("the tool's parameter schema is not the JSON text of an object")?;
        session.tools.push(AddedTool {
            name,
            wire_name: (!wire_name.is_empty()).then_some(wire_name),
            description,
            parameters,
        });
        Ok(RV_OK)
    })
}

/// Sends RV_PACK_NEW for the Pack `plugin` inserted under the name `pack`,
/// and returns the instance pointer the Plugin set and the tools it
/// added. A call this rejects gets no RV_PACK_DELETE.
///
/// The message of each failure is the reason the run goes without the
/// Pack: the Plugin's own reason for a refusal, how the Plugin broke the
/// contract, or the status it returned.
fn send_pack_new(plugin: &Plugin, pack: &str) -> Result<(*mut c_void, Vec<AddedTool>), Error> {
    let mut call = new_call(RV_PACK_NEW, plugin.instance().plugin());
    let (status, mut session) = session::open(Verb::PackNew, |ctx| {
        let args = RvPackNewArgs {
            size: size_field::<RvPackNewArgs>(),
            reserved: 0,
            ctx,
            pack: rv_str(pack),
            refuse,
            add_tool,
        };
        call.args = (&raw const args).cast();
        // SAFETY: inserting the Plugin required its entry point to follow
        // the contract and not unwind, `call` is a live `RvCall` that
        // nothing else uses, and its `args` is live for the call.
        unsafe { (plugin.instance().entry())(&raw mut call) }
    });
    let unavailable = |message: String| make_tool_error(ErrorCode::ToolOther, message);
    if let Some(reason) = session.refusal.take() {
        return Err(unavailable(reason));
    }
    if let Some(misuse) = session.misuse.take() {
        return Err(unavailable(format!(
            "the Plugin {} misused the Pack functions: {misuse}",
            plugin.name()
        )));
    }
    if status != RV_OK {
        return Err(unavailable(format!(
            "the Plugin {} answered RV_PACK_NEW with status {status}",
            plugin.name()
        )));
    }
    Ok((call.pack_run, mem::take(&mut session.tools)))
}
