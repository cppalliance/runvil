//! One tool call of a Pack instance that a Plugin's entry point made: the
//! future that sends RV_TOOL_CALL once each time it is polled, the
//! function an `RvToolCallArgs` carries to end the call, and the sender
//! of RV_TOOL_CANCEL.

use std::ffi::c_void;
use std::future::Future;
use std::pin::Pin;
use std::ptr;
use std::task::{Context, Poll};

use runvil_plugin::{Access, Error, ErrorCode, ToolOutput, make_tool_error};
use runvil_plugin_sys::{
    RV_MAX_RESULT, RV_OK, RV_PENDING, RV_RESULT_BACKEND, RV_RESULT_INVALID_ARGUMENTS,
    RV_RESULT_OTHER, RV_RESULT_TRANSPORT, RV_RESULT_TRUSTED, RV_RESULT_UNTRUSTED, RV_TOOL_CALL,
    RV_TOOL_CANCEL, RvCall, RvStatus, RvToolCallArgs, RvToolResult,
};
use serde_json::Value;

use crate::file::{file_list, file_read, file_stat};
use crate::pack::EntryPack;
use crate::session::{
    self, Field, Session, Shape, TOOL_CALL, answer, field, rv_str, shape, size_field,
};

#[cfg(test)]
#[path = "call-tests.rs"]
mod tests;

const RESULT: Shape = shape!("finish", "result", "RvToolResult");
const TEXT: Field = field!("the result's text", 16_777_216, RV_MAX_RESULT);

/// One tool call in flight. Each poll sends one RV_TOOL_CALL step. A step
/// that returns RV_PENDING wakes the future's own task and leaves it
/// pending, so the Host's executor gives the run's other work a turn and
/// then polls again. Dropping the future while its last step returned
/// RV_PENDING sends RV_TOOL_CANCEL, so the Plugin frees the call's state.
///
/// It borrows the Pack instance, so the instance is deleted only after
/// every one of its calls has ended.
pub(crate) struct Steps<'a> {
    pack: &'a EntryPack,
    /// The call's files, which each step's file functions read.
    access: &'a Access,
    /// The tool's name within its Pack.
    tool: String,
    /// The JSON text of the call's arguments.
    arguments: String,
    /// What the last step left in `call->tool_call`, which the loader
    /// passes back and never reads through.
    state: *mut c_void,
    /// Whether the last step returned RV_PENDING, so the Plugin still
    /// holds state for the call.
    pending: bool,
    /// Whether the call has ended.
    ended: bool,
}

// SAFETY: the `# Threads and lifetime` section of `runvil-plugin-sys` lets
// the steps of one tool call, and its RV_TOOL_CANCEL, come on different
// threads, and the loader never reads through `state`. Every other field
// is `Send`, which the assertion below checks for the two borrows.
unsafe impl Send for Steps<'_> {}

const _: () = {
    const fn is_send<T: Send>() {}
    is_send::<(&EntryPack, &Access)>();
};

impl<'a> Steps<'a> {
    /// A call of the tool named `tool` on `pack` with `arguments`, over
    /// the files `access` reaches. Nothing is sent until the first poll.
    pub(crate) fn new(
        pack: &'a EntryPack,
        tool: &str,
        arguments: &Value,
        access: &'a Access,
    ) -> Steps<'a> {
        Steps {
            pack,
            access,
            tool: tool.to_owned(),
            arguments: arguments.to_string(),
            state: ptr::null_mut(),
            pending: false,
            ended: false,
        }
    }

    /// An `RvCall` that sends `verb` for this call.
    fn new_call(&self, verb: u32) -> RvCall {
        RvCall {
            tool_call: self.state,
            ..self.pack.new_call(verb)
        }
    }

    /// Sends one RV_TOOL_CALL step, and returns the call's result once it
    /// has ended.
    fn step(&mut self) -> Option<Result<ToolOutput, Error>> {
        let mut call = self.new_call(RV_TOOL_CALL);
        let (status, mut session) = session::open_step(self.access, |ctx| {
            let args = RvToolCallArgs {
                size: size_field::<RvToolCallArgs>(),
                reserved: 0,
                ctx,
                tool: rv_str(&self.tool),
                arguments: rv_str(&self.arguments),
                finish,
                file_stat,
                file_list,
                file_read,
            };
            call.args = (&raw const args).cast();
            // SAFETY: `call` carries this call's own pointers and a live
            // `RvToolCallArgs`, and no other step of the call runs,
            // because a step needs `&mut self`.
            unsafe { self.pack.send(&mut call) }
        });
        self.state = call.tool_call;
        self.pending = status == RV_PENDING;
        let outcome = judge(status, &mut session);
        if outcome.is_some() {
            self.end();
        }
        outcome
    }

    /// Ends the call: sends RV_TOOL_CANCEL when its last step returned
    /// RV_PENDING, so the Plugin frees the state it still holds.
    fn end(&mut self) {
        self.ended = true;
        if !std::mem::take(&mut self.pending) {
            return;
        }
        let mut call = self.new_call(RV_TOOL_CANCEL);
        // SAFETY: the call's last step returned RV_PENDING and no step
        // runs, so this is the one cancel the contract allows it.
        let _status = session::closed(|| unsafe { self.pack.send(&mut call) });
    }
}

impl Future for Steps<'_> {
    type Output = Result<ToolOutput, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.ended {
            return Poll::Ready(Err(failure(
                "the tool call was polled after it ended".to_owned(),
            )));
        }
        if let Some(result) = this.step() {
            return Poll::Ready(result);
        }
        // The contract has no wake function: a pending call gets its next
        // step as soon as the executor polls again.
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl Drop for Steps<'_> {
    fn drop(&mut self) {
        self.end();
    }
}

/// A failed call that is the Plugin's fault or the loader's, with
/// `message` as the text a model reads.
fn failure(message: String) -> Error {
    make_tool_error(ErrorCode::ToolOther, message)
}

/// What a step that returned `status` means: `None` for a call that needs
/// another step, and otherwise how the call ended. The checks come in
/// this order: a misuse, then a result beside RV_PENDING, then a status
/// other than RV_OK, then no result.
fn judge(status: RvStatus, session: &mut Session) -> Option<Result<ToolOutput, Error>> {
    if let Some(misuse) = session.misuse.take() {
        return Some(Err(failure(format!(
            "the tool's Plugin misused the tool call: {misuse}"
        ))));
    }
    match (status, session.result.take()) {
        (RV_PENDING, None) => None,
        (RV_PENDING, Some(_)) => Some(Err(failure(
            "the tool's Plugin misused the tool call: a step called finish and then returned \
             RV_PENDING"
                .to_owned(),
        ))),
        (RV_OK, Some(result)) => Some(result),
        (RV_OK, None) => Some(Err(failure(
            "the tool's Plugin answered RV_TOOL_CALL with RV_OK without finishing the call"
                .to_owned(),
        ))),
        (status, _) => Some(Err(failure(format!(
            "the tool's Plugin answered RV_TOOL_CALL with status {status}"
        )))),
    }
}

/// Records the result a step ends its call with, validated; returns RV_OK
/// or RV_ERR_MISUSE.
pub(crate) unsafe extern "C" fn finish(ctx: *mut c_void, result: *const RvToolResult) -> RvStatus {
    answer(ctx, &TOOL_CALL, |session| {
        if session.result.is_some() {
            return Err("finish was called twice");
        }
        // SAFETY: the contract makes `result` point at a readable
        // `RvToolResult` during this call.
        let result = unsafe { RESULT.read(result) }?;
        let code = match result.kind {
            RV_RESULT_UNTRUSTED | RV_RESULT_TRUSTED => None,
            RV_RESULT_INVALID_ARGUMENTS => Some(ErrorCode::ToolInvalidArguments),
            RV_RESULT_BACKEND => Some(ErrorCode::ToolBackend),
            RV_RESULT_TRANSPORT => Some(ErrorCode::ToolTransport),
            RV_RESULT_OTHER => Some(ErrorCode::ToolOther),
            _ => return Err("the result's kind is not one of the RV_RESULT constants"),
        };
        // SAFETY: the contract makes the text valid for its length.
        let text = unsafe { TEXT.read(result.text) }?;
        session.result = Some(match code {
            Some(code) => Err(make_tool_error(code, text)),
            None if result.kind == RV_RESULT_TRUSTED => Ok(ToolOutput::trusted(text)),
            None => Ok(ToolOutput::untrusted(text)),
        });
        Ok(RV_OK)
    })
}
