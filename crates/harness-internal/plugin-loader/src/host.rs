//! The `RvHost` table every call hands a Plugin, the functions each
//! RV_PLUGIN_LOAD's `RvLoadArgs` carries, and the senders of
//! RV_PLUGIN_LOAD and RV_PLUGIN_UNLOAD.

use std::ffi::c_void;
use std::{mem, ptr};

use runvil_plugin::{Error, PackId};
use runvil_plugin_sys::{
    PluginEntry, RV_API_MAJOR, RV_API_MINOR, RV_MAX_LINE, RV_MAX_NAME, RV_OK, RV_PLUGIN_LOAD,
    RV_PLUGIN_UNLOAD, RvCall, RvHost, RvLoadArgs, RvPackInfo, RvPluginInfo, RvStatus, RvStr,
    RvVerb,
};

use crate::load::make_load_error;
use crate::session::{
    self, Described, Field, InsertedPack, LOAD, RESERVED, Session, Shape, Verb, answer, field,
    shape, size_field,
};

#[cfg(test)]
#[path = "host-tests.rs"]
mod tests;

/// The `RvHost` table every call hands the Plugin.
pub(crate) static HOST: RvHost = RvHost {
    size: size_field::<RvHost>(),
    api_major: RV_API_MAJOR,
    api_minor: RV_API_MINOR,
    reserved: 0,
};

/// The bound on one name segment, in bytes.
pub(crate) const MAX_SEGMENT: usize = 64;

const PLUGIN_INFO: Shape = shape!("describe_plugin", "info", "RvPluginInfo");
const NAME: Field = field!("the name", 64, RV_MAX_NAME);
const VERSION: Field = field!("the version", 64, RV_MAX_NAME);

const PACK_INFO: Shape = shape!("insert_pack", "info", "RvPackInfo");
/// Two segments of 64 bytes and the `/` between them.
const PACK_NAME: Field = field!("the Pack's name", 129, 2 * RV_MAX_NAME + 1);
const PACK_DESCRIPTION: Field = field!("the Pack's description", 1024, RV_MAX_LINE);

/// An `RvCall` that sends `verb` to the Plugin instance `plugin`, with the
/// loader's `RvHost` table and nothing else set.
pub(crate) fn new_call(verb: RvVerb, plugin: *mut c_void) -> RvCall {
    RvCall {
        size: size_field::<RvCall>(),
        verb,
        host: &raw const HOST,
        plugin,
        pack_run: ptr::null_mut(),
        args: ptr::null(),
        tool_call: ptr::null_mut(),
    }
}

/// Whether `name` is one name segment: a Pack id segment of at most 64
/// bytes.
pub(crate) fn is_segment(name: &str) -> bool {
    name.len() <= MAX_SEGMENT && PackId::parse(name).is_ok()
}

/// Records the Plugin's refusal to load, validated; returns
/// RV_ERR_REFUSED, or RV_ERR_MISUSE for a `ctx` it doesn't answer or a
/// second refusal.
pub(crate) unsafe extern "C" fn refuse(ctx: *mut c_void, reason: RvStr) -> RvStatus {
    session::refuse(ctx, &LOAD, reason)
}

/// Records the Plugin's description once, validated; returns RV_OK or
/// RV_ERR_MISUSE.
pub(crate) unsafe extern "C" fn describe_plugin(
    ctx: *mut c_void,
    info: *const RvPluginInfo,
) -> RvStatus {
    answer(ctx, &LOAD, |session| {
        if session.described.is_some() {
            return Err("describe_plugin was called twice");
        }
        // SAFETY: the contract makes `info` point at a readable
        // `RvPluginInfo` during this call.
        let info = unsafe { PLUGIN_INFO.read(info) }?;
        if info.reserved != 0 {
            return Err(RESERVED);
        }
        // SAFETY: the contract makes each string valid for its length.
        let name = unsafe { NAME.read(info.name) }?;
        if !is_segment(&name) {
            return Err("the name is not one name segment, such as `acme-url`");
        }
        // SAFETY: as above.
        let version = unsafe { VERSION.read(info.version) }?;
        if version.is_empty() {
            return Err("the version is empty");
        }
        if version.chars().any(char::is_control) {
            return Err("the version has a control character");
        }
        session.described = Some(Described { name, version });
        Ok(RV_OK)
    })
}

/// Records one Pack the Plugin inserts, validated; returns RV_OK or
/// RV_ERR_MISUSE.
pub(crate) unsafe extern "C" fn insert_pack(ctx: *mut c_void, info: *const RvPackInfo) -> RvStatus {
    answer(ctx, &LOAD, |session| {
        // SAFETY: the contract makes `info` point at a readable
        // `RvPackInfo` during this call.
        let info = unsafe { PACK_INFO.read(info) }?;
        if info.reserved != 0 {
            return Err(RESERVED);
        }
        // SAFETY: the contract makes each string valid for its length.
        let name = unsafe { PACK_NAME.read(info.name) }?;
        let two_segments = name
            .split_once('/')
            .is_some_and(|(vendor, id)| is_segment(vendor) && is_segment(id));
        if !two_segments {
            return Err(
                "the Pack's name is not a vendor and a Pack id joined by `/`, such as `acme/url`",
            );
        }
        if session.packs.iter().any(|pack| pack.name == name) {
            return Err("two Packs of one Plugin share a name");
        }
        // SAFETY: as above.
        let description = unsafe { PACK_DESCRIPTION.read(info.description) }?;
        if description.chars().any(char::is_control) {
            return Err("the Pack's description has a control character");
        }
        session.packs.push(InsertedPack { name, description });
        Ok(RV_OK)
    })
}

/// What a successful RV_PLUGIN_LOAD left: the Plugin's description, the
/// Packs it inserted, the instance pointer it set, and the session its
/// `ctx` points at, which the caller keeps until RV_PLUGIN_UNLOAD returns.
pub(crate) struct Loading {
    pub(crate) described: Described,
    pub(crate) packs: Vec<InsertedPack>,
    pub(crate) instance: *mut c_void,
    pub(crate) ctx: Box<Session>,
}

/// Sends RV_PLUGIN_LOAD through `entry` with a fresh `ctx` open on this
/// thread, then closes it. On failure, the `ctx` is freed before this
/// returns.
///
/// # Safety
/// `entry` must follow the C contract in `runvil-plugin-sys`, and must not
/// unwind.
pub(crate) unsafe fn send_load(entry: PluginEntry) -> Result<Loading, Error> {
    let mut call = new_call(RV_PLUGIN_LOAD, ptr::null_mut());
    let (status, mut session) = session::open(Verb::Load, |ctx| {
        let args = RvLoadArgs {
            size: size_field::<RvLoadArgs>(),
            reserved: 0,
            ctx,
            refuse,
            describe_plugin,
            insert_pack,
        };
        call.args = (&raw const args).cast();
        // SAFETY: the caller guarantees `entry` follows the contract, and
        // `call` is a live `RvCall` that nothing else uses, whose `args`
        // is live for the call.
        unsafe { entry(&raw mut call) }
    });
    if let Some(reason) = session.refusal.take() {
        return Err(make_load_error(
            "refused",
            format!("the Plugin refused: {reason}"),
        ));
    }
    if let Some(reason) = session.misuse.take() {
        return Err(make_load_error(
            "misuse",
            format!("the Plugin misused the load functions: {reason}"),
        ));
    }
    if status != RV_OK {
        return Err(make_load_error(
            "status",
            format!("RV_PLUGIN_LOAD returned status {status}"),
        ));
    }
    let Some(described) = session.described.take() else {
        return Err(make_load_error(
            "undescribed",
            "RV_PLUGIN_LOAD returned without describing the Plugin".to_owned(),
        ));
    };
    Ok(Loading {
        described,
        packs: mem::take(&mut session.packs),
        instance: call.plugin,
        ctx: session,
    })
}

/// Sends RV_PLUGIN_UNLOAD through `entry` with `plugin` and NULL `args`. No
/// call is open on this thread meanwhile, so any function the Plugin calls
/// answers RV_ERR_MISUSE. The status is ignored, because a Plugin that is
/// going away has no one to tell.
///
/// # Safety
/// `entry` must follow the C contract and must not unwind, and `plugin`
/// must be the pointer its RV_PLUGIN_LOAD set, not yet unloaded, with
/// every Pack instance it made deleted.
pub(crate) unsafe fn send_unload(entry: PluginEntry, plugin: *mut c_void) {
    let mut call = new_call(RV_PLUGIN_UNLOAD, plugin);
    // SAFETY: the caller guarantees `entry` follows the contract and
    // `plugin` is the live instance, and `call` is a live `RvCall`.
    let _status = session::closed(|| unsafe { entry(&raw mut call) });
}
