//! Section VM execution: block coroutines, from their start through each
//! resume to the classified end.

use mlua::thread::ThreadStatus;
use mlua::{MultiValue, Thread, Value};

use super::SectionVm;
use crate::coro::{StashedFailure, block_guard, take_failure};
use crate::error::{Error, Result, into_universal};
use crate::error_value::ErrorKind;
use crate::program::LuaProgram;
use crate::protocol::{Answer, Request, Retained, YieldParse, render_envelope};
use crate::section::LuaBlockResult;

impl SectionVm {
    /// Takes any recorded jump target, propagating a poisoned jump-slot lock
    /// rather than silently coercing the failure into "no jump".
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the jump-slot mutex is poisoned.
    pub(crate) fn take_jump(&self) -> Result<Option<String>> {
        let mut slot = self
            .jump_slot
            .lock()
            .map_err(|_| Error::internal(JUMP_SLOT_POISONED))?;
        Ok(slot.take())
    }

    /// Clears any jump target an earlier chunk recorded.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the jump-slot mutex is poisoned.
    pub(crate) fn clear_jump(&self) -> Result<()> {
        self.take_jump().map(drop)
    }

    /// Starts one Lua block as a coroutine on this VM and resumes it to its
    /// first yield or its end.
    ///
    /// This is the scheduler's chunk-execution path: one coroutine per Lua
    /// block, created from the block's loaded function on this persistent
    /// VM, so the VM's globals (`var` and the bare globals) roll forward
    /// across blocks. Instruction hooks are
    /// per-coroutine in PUC Lua, so the VM's cancellation hook is
    /// installed on the fresh thread; the main-state hook from construction
    /// never fires inside a resumed coroutine.
    ///
    /// A coroutine that returns ends the block: a recorded jump takes
    /// precedence over the chunk's own error, genuine failures map through
    /// `LuaProgram::map_runtime_error_among` with the VM's loaded library
    /// programs, and the scalar-return rule applies to the return values.
    /// A coroutine that yields suspends with the shim's request table; the
    /// driver resumes it with
    /// [`resume_block_coro_answer`](Self::resume_block_coro_answer). No
    /// observation events fire here; the driver owns the chunk observation
    /// boundaries.
    ///
    /// The coroutine body is the shim's block guard, resumed first with
    /// the block function: the guard runs the block under `xpcall`, whose
    /// handler stashes a failure and its raise-point traceback, and
    /// re-raises the same value, so a structured error table a shim raised
    /// reaches the Engine as a typed [`Error::Raised`] rather than only as
    /// mlua's stringification, and a Lua-raised error keeps the author's
    /// frames for the prompt-line mapping.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] if the jump slot is poisoned, or
    /// [`Error::LuaRuntime`] if the program cannot load, the shim prelude
    /// never ran on this VM, or the thread cannot be created or hooked; a
    /// block failure returns the mapped runtime error.
    pub fn start_block_coro(&self, program: &LuaProgram) -> Result<CoroStep> {
        self.clear_jump()?;
        let function = program.load(&self.lua)?;
        let guard = block_guard(&self.lua)?;
        let thread = self.lua.create_thread(guard).map_err(Error::lua)?;
        self.cancel_hook.install_on_thread(&thread)?;
        let result = thread.resume::<MultiValue>(function);
        match self.step_block_coro(thread, result)? {
            Ok(step) => Ok(step),
            Err(error) => Err(self.block_failure(program, &error)?),
        }
    }

    /// Resumes a suspended block coroutine with the driver's answer values.
    ///
    /// # Errors
    /// Same contract as [`start_block_coro`](Self::start_block_coro).
    #[cfg(any(test, feature = "test-support"))]
    pub fn resume_block_coro(
        &self,
        program: &LuaProgram,
        thread: &Thread,
        args: impl mlua::IntoLuaMulti,
    ) -> Result<CoroStep> {
        let result = thread.resume::<MultiValue>(args);
        match self.step_block_coro(thread.clone(), result)? {
            Ok(step) => Ok(step),
            Err(error) => Err(self.block_failure(program, &error)?),
        }
    }

    /// Validates a suspended block coroutine's yielded values into the
    /// boundary outcome.
    ///
    /// A shim yields exactly its request table; anything else is
    /// [`YieldParse::Malformed`] and fails the block as a hand-rolled or
    /// corrupted yield. Author code cannot reach
    /// `coroutine.yield` (the global is stripped at shim install), so this
    /// strict validation is defense in depth. A well-formed call whose
    /// argument fails validation is [`YieldParse::Refused`]: the error is
    /// returned as the answer so the shim raises it at the call site.
    #[must_use]
    pub fn request_from_yield(&self, values: &MultiValue) -> YieldParse {
        Request::from_yield(&self.lua, values.iter().next().unwrap_or(&Value::Nil))
    }

    /// Resumes a suspended block coroutine with the driver's answer.
    ///
    /// The answer renders to its `(ok, result)` envelope on this VM. On a
    /// failure answer the envelope includes the error's structured table
    /// (`kind`, `message`, fields) for the shim to raise. When the block's
    /// failure is a raise of that very table - the shim's own raise, or the
    /// block's re-raise of the value it caught - the typed error the answer
    /// owned is substituted back, so the Rust caller receives the
    /// structured error rather than a string. A block that caught the
    /// table and failed with anything else, its message text included,
    /// keeps its own error.
    ///
    /// This crate's own failures leave it as universal errors, through
    /// [`into_universal`].
    ///
    /// # Errors
    /// Same contract as [`start_block_coro`](Self::start_block_coro), as
    /// universal errors, plus a `LuaRuntime` failure if the envelope cannot
    /// be rendered on this VM.
    pub fn resume_block_coro_answer(
        &self,
        program: &LuaProgram,
        thread: &Thread,
        answer: std::result::Result<Answer, runvil_types::error::Error>,
    ) -> std::result::Result<CoroStep, runvil_types::error::Error> {
        let (envelope, retained) = render_envelope(&self.lua, answer)
            .map_err(|error| into_universal(Error::lua(error)))?;
        let result = thread.resume::<MultiValue>(envelope);
        match self
            .step_block_coro(thread.clone(), result)
            .map_err(into_universal)?
        {
            Ok(step) => Ok(step),
            Err(error) => Err(self.answered_block_failure(program, &error, retained)),
        }
    }

    /// Steps a resumed block coroutine: a yield suspends it with the
    /// shim's request table, a recorded jump takes precedence over the
    /// chunk's own error, which is the jump's own transfer marker, and a
    /// return applies the scalar-return rule. Any other end is the block's
    /// failure, left for the caller to classify.
    fn step_block_coro(
        &self,
        thread: Thread,
        result: mlua::Result<MultiValue>,
    ) -> Result<std::result::Result<CoroStep, mlua::Error>> {
        match result {
            // A resumed thread that is still resumable suspended on a yield;
            // the yielded values are the shim's request table.
            Ok(values) if thread.status() == ThreadStatus::Resumable => {
                Ok(Ok(CoroStep::Yielded(thread, values)))
            }
            result => {
                if let Some(heading) = self.take_jump()? {
                    return Ok(Ok(CoroStep::Done(LuaBlockResult::Jump(heading))));
                }
                match result {
                    Ok(returned) => Ok(Ok(CoroStep::Done(LuaBlockResult::Returned(
                        scalar_return(returned)?,
                    )))),
                    Err(error) => Ok(Err(error)),
                }
            }
        }
    }

    /// Classifies a block coroutine's failure: the guard's stash restores
    /// the raise-point traceback onto a Lua-raised error first (the guard's
    /// re-raise is what killed the coroutine, so mlua's own traceback shows
    /// only the guard's frame), and the failure maps through
    /// [`map_chunk_failure`](Self::map_chunk_failure) to the mapped prompt
    /// line, which [`kept_failure`] then keeps or replaces with the
    /// structured table the guard stashed. The stash is taken on every
    /// failure so it never goes stale.
    fn block_failure(&self, program: &LuaProgram, error: &mlua::Error) -> Result<Error> {
        let (stashed, mapped) = self.mapped_failure(program, error)?;
        Ok(kept_failure(stashed, mapped))
    }

    /// Classifies the failure of a block resumed with an answer, as
    /// [`block_failure`](Self::block_failure) does, except that a raise of
    /// `retained`'s error table, the answer's own, is the answer's typed
    /// error, unless the block was cancelled or spent an Engine quota.
    fn answered_block_failure(
        &self,
        program: &LuaProgram,
        error: &mlua::Error,
        retained: Option<Retained>,
    ) -> runvil_types::error::Error {
        let (stashed, mapped) = match self.mapped_failure(program, error) {
            Ok(parts) => parts,
            Err(failure) => return into_universal(failure),
        };
        if !is_fatal(&mapped)
            && let Some(retained) = retained
            && stashed.raised_table_is(&retained.table)
        {
            return retained.error;
        }
        into_universal(kept_failure(stashed, mapped))
    }

    /// Takes the guard's stash for a block's failure and maps the failure,
    /// its raise-point traceback restored, to its prompt line.
    fn mapped_failure(
        &self,
        program: &LuaProgram,
        error: &mlua::Error,
    ) -> Result<(StashedFailure, Error)> {
        let stashed = take_failure(&self.lua)?;
        let mapped = self.map_chunk_failure(program, &stashed.restore_traceback(error));
        Ok((stashed, mapped))
    }

    /// Maps one chunk's Lua failure to its typed outcome: a chunk the
    /// instruction hook aborted under the run's cancel flag is
    /// [`Error::Interrupted`], whatever the raw error says; everything
    /// else maps through `LuaProgram::map_runtime_error_among` of `program`
    /// and of every library block `require` loaded into this VM.
    pub(crate) fn map_chunk_failure(&self, program: &LuaProgram, error: &mlua::Error) -> Error {
        if self.cancel_hook.is_cancelled() {
            return Error::Interrupted;
        }
        match self.library_programs.lock() {
            Ok(libraries) => program.map_runtime_error_among(error, &libraries),
            Err(_) => Error::internal("require's loaded library programs were poisoned"),
        }
    }
}

/// The failure of a poisoned jump slot.
const JUMP_SLOT_POISONED: &str = "jump slot poisoned";

/// Whether `mapped` is a cancellation or an exhausted Engine quota, which
/// keep their mapping whatever the block raised.
fn is_fatal(mapped: &Error) -> bool {
    matches!(mapped, Error::Interrupted | Error::LuaQuota { .. })
}

/// The failure a block keeps, given what the guard `stashed` and the
/// failure `mapped` to its prompt line: the mapping for a cancellation, an
/// Engine quota, a raise that is no error table, or a `lua`-kind table,
/// whose mapped runtime error keeps the same message with its source and
/// the mapped prompt line; any other structured table as
/// [`Error::Raised`]. A raise that is no error table keeps its mapping, so
/// an uncaught direct `compactors.fail` call is
/// [`Error::ContextExhausted`].
fn kept_failure(stashed: StashedFailure, mapped: Error) -> Error {
    if is_fatal(&mapped) {
        return mapped;
    }
    match stashed.raised {
        None => mapped,
        Some(raised) => match raised.kind {
            ErrorKind::Lua => mapped,
            ErrorKind::ToolLoopExhausted
            | ErrorKind::ContextExhausted
            | ErrorKind::EmptyModelReply
            | ErrorKind::ModelFailure
            | ErrorKind::ModelRequired
            | ErrorKind::OutOfScopeTool
            | ErrorKind::UnboundTool
            | ErrorKind::ModelOnlyTool
            | ErrorKind::Tool
            | ErrorKind::TaskNotOwned
            | ErrorKind::TaskConsumed
            | ErrorKind::TasksLive
            | ErrorKind::Cancelled
            | ErrorKind::Store
            | ErrorKind::Internal => Error::Raised(raised),
        },
    }
}

/// Renders a returned Lua scalar as the section's result string. Tables and other
/// non-scalar returns are errors.
fn value_to_string(value: &Value) -> Result<String> {
    match value {
        Value::String(s) => Ok(s.to_string_lossy()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Boolean(b) => Ok(b.to_string()),
        other => Err(Error::Lua(format!(
            "cannot return a {} as a result",
            other.type_name()
        ))),
    }
}

/// Converts only the first returned Lua value into an optional scalar string.
///
/// Later values are ignored. A missing or nil first value returns `None`.
///
/// # Errors
/// Returns [`Error::Lua`] when the first value is not a supported scalar.
pub(crate) fn scalar_return(returned: MultiValue) -> Result<Option<String>> {
    match returned.into_iter().next() {
        None | Some(Value::Nil) => Ok(None),
        Some(value) => value_to_string(&value).map(Some),
    }
}

/// One step of a Lua block running on a coroutine.
///
/// Produced by [`SectionVm::start_block_coro`] and
/// [`SectionVm::resume_block_coro_answer`]; consumed by the scheduler's
/// driver, which validates a [`Yielded`](CoroStep::Yielded) request,
/// dispatches it, and resumes the thread with the answer.
#[derive(Debug)]
pub enum CoroStep {
    /// The coroutine suspended on a shim yield; the yielded values are
    /// the request table.
    Yielded(Thread, MultiValue),
    /// The coroutine ended (return, jump, or error); a recorded jump wins
    /// over the chunk's own error, and the scalar-return rule applies.
    Done(LuaBlockResult),
}
