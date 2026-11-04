//! The crate's error: the universal [`Error`] every Engine failure is, and
//! the constructors that build each failure the Engine raises itself.
//!
//! A run a failure ends reports a kind and a message in
//! [`RunOutcome::Failed`](crate::RunOutcome::Failed), both read from the
//! error's code, message, location, and facts. A failure that reaches an
//! author's Lua renders as the error table the Lua crate builds from the
//! same code and facts. A parse, a model round, a tool, a `{{ }}`
//! substitution, and the Lua crate fail with this type already; the Lua
//! crate owns the constructors of the failures it shares with the Engine,
//! which this module re-exports.
//!
//! Each constructor gives its failure the text it has always had as the
//! message, and the code's default prose unless the failure has
//! model-facing text of its own. A cause's text joins the message through
//! [`Error::with_cause`].

use std::borrow::Cow;

#[cfg(test)]
pub(crate) use runvil_lua::{context_exhausted, lua_quota, tool_loop_exhausted};
pub(crate) use runvil_lua::{
    internal, interrupted, lua_runtime, task_cancelled, task_consumed, task_not_owned,
};
pub(crate) use runvil_types::error::Error;
use runvil_types::error::{ErrorCode, FactKey, failure, with_fact};
use runvil_types::ids::TaskId;

#[cfg(test)]
mod tests;
mod tools;

pub(crate) use tools::{
    bind_schema, model_only_tool, out_of_scope_tool, tool_in_set, tool_sets, unbound_tool,
};

/// Crate-internal result alias over [`Error`].
pub(crate) type Result<T> = std::result::Result<T, Error>;

/// An internal invariant failure that names the failure which broke the
/// invariant after its own text, capturing the call site as [`internal`]
/// does.
#[track_caller]
pub(crate) fn internal_with_cause(
    message: impl Into<Cow<'static, str>>,
    cause: &(dyn std::error::Error + 'static),
) -> Error {
    internal(message).with_cause(cause)
}

/// A prompt that declares the `runvil:` major `version`, which this build
/// does not support, so it is refused rather than run under mismatched
/// rules. Major 0 is the only supported major.
pub(crate) fn unsupported_version(version: u32) -> Error {
    failure(
        ErrorCode::UnsupportedVersion,
        format!("unsupported runvil version: {version} (this build supports major 0)"),
    )
}

/// The environment's refusal of the prompt: `notice`, the model-readable
/// refusal with one line per gap, is the prose, and the message follows it
/// with the text of the failure the refusal stands in for, when one raised
/// it.
pub(crate) fn requirements_unmet(
    notice: &str,
    cause: Option<&(dyn std::error::Error + 'static)>,
) -> Error {
    let refusal = Error::new(ErrorCode::RequirementsUnmet, notice, notice);
    match cause {
        Some(cause) => refusal.with_cause(cause),
        None => refusal,
    }
}

/// A `models.infer` or `models.loop` round that found no current model,
/// because the caller supplied none. `section` is the section whose round
/// had no binding, or the prompt's H1 title for a round in the H1 pass.
pub(crate) fn model_required(section: &str) -> Error {
    with_fact(
        failure(
            ErrorCode::ModelRequired,
            format!("model binding required for section {section}"),
        ),
        FactKey::Section,
        section,
    )
}

/// A `call`, `tasks.spawn`, or `tasks.fanout` arm that would run deeper
/// than `cap`. The dispatch's answer, so an author `pcall` catches it, and
/// a quota when it ends the run.
pub(crate) fn call_depth_exceeded(cap: usize) -> Error {
    with_fact(
        failure(
            ErrorCode::CallDepthExceeded,
            format!("recursion exceeded cap of {cap}"),
        ),
        FactKey::Limit,
        cap,
    )
}

/// A chain that ended while the tasks it owned were still live, which it
/// names in spawn order. The `Tasks` fact holds their ids.
pub(crate) fn tasks_live(tasks: &[TaskId]) -> Error {
    let ids: Vec<String> = tasks.iter().map(ToString::to_string).collect();
    let message = format!(
        "chain ended with tasks still live: {}; wait on or cancel every task a chain spawns \
         before it ends",
        ids.join(", ")
    );
    with_fact(failure(ErrorCode::TasksLive, message), FactKey::Tasks, ids)
}

/// A store failure with no operation to render, such as a handle
/// acquisition or the declared-store probe: the store's code and facts,
/// and a message that tells a run that declares no store so and follows
/// the fixed probe text for any other failure.
pub(crate) fn store(cause: &runvil_vfs::VfsError) -> Error {
    runvil_lua::store_failure(cause, crate::lua::store_probe_message(cause))
}

/// One store operation's failure: the store's code and facts, and the
/// model-facing message rendered with the operation for its wording.
pub(crate) fn store_op(op: &crate::lua::VfsOp, cause: &runvil_vfs::VfsError) -> Error {
    runvil_lua::store_failure(cause, crate::lua::store_error_message(op, cause))
}

/// Two execution identities that claimed one store path: `detail` is the
/// claims model's whole diagnosis, naming the canonical path, both
/// identities, and both claim kinds. Fatal to the run on the spot and
/// never resumed into Lua, so no author `pcall` can catch it.
pub(crate) fn determinism(detail: &str) -> Error {
    failure(
        ErrorCode::Determinism,
        format!("store determinism violation: {detail}"),
    )
}
