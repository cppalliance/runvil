//! The crate's error: the universal [`Error`] every Engine failure is, and
//! the constructors that build each failure the Engine raises itself.
//!
//! A run a failure ends reports the error itself in the result of
//! [`Step::Done`](crate::Step::Done), with a caller code reported as an
//! internal one. A failure that reaches an author's Lua renders as the
//! error table the Lua crate builds from the same code and facts. A
//! parse, a model round, a tool, a `{{ }}` substitution, and the Lua crate
//! fail with this type already; the Lua crate owns the constructors of the
//! failures it shares with the Engine, which this module re-exports.
//!
//! Each constructor gives its failure the text it has always had as the
//! message, and the code's default prose unless the failure has
//! model-facing text of its own. A cause's text joins the message through
//! [`Error::with_cause`].

use std::borrow::Cow;

#[cfg(test)]
pub(crate) use runvil_lua::{
    make_context_exhausted_error, make_lua_quota_error, make_tool_loop_exhausted_error,
};
pub(crate) use runvil_lua::{
    make_internal_error, make_lua_runtime_error, make_task_canceled_error,
    make_task_consumed_error, make_task_not_owned_error,
};
pub(crate) use runvil_types::error::Error;
use runvil_types::error::{ErrorCode, ErrorFact, make_failure_error, with_fact};
use runvil_types::ids::TaskId;

#[cfg(test)]
mod tests;
mod tools;

pub(crate) use tools::{
    make_bind_schema_error, make_model_only_tool_error, make_out_of_scope_tool_error,
    make_tool_sets_error, make_unbound_tool_error, tool_in_set,
};

/// Crate-internal result alias over [`Error`].
pub(crate) type Result<T> = std::result::Result<T, Error>;

/// An internal invariant failure that names the failure which broke the
/// invariant after its own text, capturing the call site as [`make_internal_error`]
/// does.
#[track_caller]
pub(crate) fn make_internal_with_cause_error(
    message: impl Into<Cow<'static, str>>,
    cause: &(dyn std::error::Error + 'static),
) -> Error {
    make_internal_error(message).with_cause(cause)
}

/// A prompt that declares the `runvil:` major `version`, which this build
/// does not support, so it is refused rather than run under mismatched
/// rules. Major 0 is the only supported major.
pub(crate) fn make_unsupported_version_error(version: u32) -> Error {
    make_failure_error(
        ErrorCode::UnsupportedVersion,
        format!("unsupported runvil version: {version} (this build supports major 0)"),
    )
}

/// The environment's refusal of the prompt: `notice`, the model-readable
/// refusal with one line per gap, is the prose, and the message follows it
/// with the text of the failure the refusal stands in for, when one raised
/// it.
pub(crate) fn make_requirements_unmet_error(
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
pub(crate) fn make_model_required_error(section: &str) -> Error {
    with_fact(
        make_failure_error(
            ErrorCode::ModelRequired,
            format!("model binding required for section {section}"),
        ),
        ErrorFact::Section,
        section,
    )
}

/// A `call`, `tasks.spawn`, or `tasks.fanout` arm that would run deeper
/// than `cap`. The dispatch's answer, so an author `pcall` catches it, and
/// a quota when it ends the run.
pub(crate) fn make_call_depth_exceeded_error(cap: usize) -> Error {
    with_fact(
        make_failure_error(
            ErrorCode::CallDepthExceeded,
            format!("recursion exceeded cap of {cap}"),
        ),
        ErrorFact::Limit,
        cap,
    )
}

/// A chain that ended while the tasks it owned were still live, which it
/// names in spawn order. The `Tasks` fact holds their ids.
pub(crate) fn make_tasks_live_error(tasks: &[TaskId]) -> Error {
    let ids: Vec<String> = tasks.iter().map(ToString::to_string).collect();
    let message = format!(
        "chain ended with tasks still live: {}; wait on or cancel every task a chain spawns \
         before it ends",
        ids.join(", ")
    );
    with_fact(
        make_failure_error(ErrorCode::TasksLive, message),
        ErrorFact::Tasks,
        ids,
    )
}

/// A run that stalled: no chain can move, the main chain does not wait on
/// its inbox, the one wait a caller's message can end, and `waiting` names,
/// in id order, the tasks that wait on their inboxes, which only their
/// owners could have filled. The `Tasks` fact holds their ids. A stall with
/// no such task is the scheduler's own fault and ends the run with an
/// internal error instead.
pub(crate) fn make_stalled_error(waiting: &[TaskId]) -> Error {
    let ids: Vec<String> = waiting.iter().map(ToString::to_string).collect();
    let message = format!(
        "the run stalled: no chain can move, and these tasks wait on their inboxes: {}",
        ids.join(", ")
    );
    with_fact(
        make_failure_error(ErrorCode::Stalled, message),
        ErrorFact::Tasks,
        ids,
    )
}

/// A store failure with no operation to render, such as a handle
/// acquisition or the declared-store probe: the store's code and facts,
/// and a message that tells a run that declares no store so and follows
/// the fixed probe text for any other failure.
pub(crate) fn make_store_error(cause: &runvil_vfs::VfsError) -> Error {
    runvil_lua::make_store_failure_error(cause, crate::lua::store_probe_message(cause))
}

/// One store operation's failure: the store's code and facts, and the
/// model-facing message rendered with the operation for its wording.
pub(crate) fn make_store_op_error(op: &crate::lua::VfsOp, cause: &runvil_vfs::VfsError) -> Error {
    runvil_lua::make_store_failure_error(cause, crate::lua::store_error_message(op, cause))
}

/// Two execution identities that claimed one store path: `detail` is the
/// claims model's whole diagnosis, naming the canonical path, both
/// identities, and both claim kinds. Fatal to the run on the spot and
/// never resumed into Lua, so no author `pcall` can catch it.
pub(crate) fn make_determinism_error(detail: &str) -> Error {
    make_failure_error(
        ErrorCode::Determinism,
        format!("store determinism violation: {detail}"),
    )
}
