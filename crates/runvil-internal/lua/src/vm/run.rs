//! Section VM execution: block coroutines, from their start through each
//! resume to the classified end.

use mlua::thread::ThreadStatus;
use mlua::{MultiValue, Thread, Value};
use runvil_types::error::{ErrorCode, place};

use super::{JUMP_SLOT_POISONED, Jump, SectionVm};
use crate::coro::{StashedFailure, block_guard, take_failure};
use crate::error::{self, Error, Result, prompt_file, set_call_line};
use crate::error_value::{ErrorKind, error_kind};
use crate::program::LuaProgram;
use crate::protocol::{Answer, Request, Retained, YieldParse, render_envelope};
use crate::section::{LuaBlockResult, scalar_return};

impl SectionVm {
    /// Takes any recorded jump: its target and the prompt line of the
    /// `jump` call, propagating a poisoned jump-slot lock rather than
    /// silently coercing the failure into "no jump".
    ///
    /// # Errors
    /// Returns an `Internal` failure when the jump-slot mutex is poisoned.
    pub(crate) fn take_jump(&self) -> Result<Option<Jump>> {
        let mut slot = self
            .jump_slot
            .lock()
            .map_err(|_| error::make_internal_error(JUMP_SLOT_POISONED))?;
        Ok(slot.take())
    }

    /// Clears any jump target an earlier chunk recorded.
    ///
    /// # Errors
    /// Returns an `Internal` failure when the jump-slot mutex is poisoned.
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
    /// [`resume_block_coro_answer`](Self::resume_block_coro_answer). The
    /// start clears the VM's [`call_line`](Self::call_line), which each of
    /// the block's yields then sets. No observation events fire here; the
    /// driver owns the chunk observation boundaries.
    ///
    /// The coroutine body is the shim's block guard, resumed first with
    /// the block function: the guard runs the block under `xpcall`, whose
    /// handler stashes a failure and its raise-point traceback, and
    /// re-raises the same value, so a structured error table a shim raised
    /// reaches the Engine as the universal error it stands for rather than
    /// only as mlua's stringification, and a Lua-raised error keeps the
    /// author's frames for the prompt-line mapping.
    ///
    /// # Errors
    /// Returns an `Internal` failure if the jump slot is poisoned, or a
    /// `LuaRuntime` failure if the program cannot load, the shim install
    /// never ran on this VM, or the thread cannot be created or hooked; a
    /// block failure returns the mapped runtime error.
    pub fn start_block_coro(&self, program: &LuaProgram) -> Result<CoroStep> {
        self.clear_jump()?;
        set_call_line(&self.lua, None);
        let function = program.load(&self.lua)?;
        let guard = block_guard(&self.lua)?;
        let thread = self
            .lua
            .create_thread(guard)
            .map_err(error::make_lua_error)?;
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
    /// (`kind`, `code`, `message`, fields) for the shim to raise. When the
    /// block's failure is a raise of that very table - the shim's own raise,
    /// or the block's re-raise of the value it caught - the typed error the
    /// answer owned is substituted back, so the Rust caller receives the
    /// structured error rather than a string. A block that caught the
    /// table and failed with anything else, its message text included,
    /// keeps its own error. A block that was canceled or spent an Engine
    /// quota keeps that failure whatever it raised.
    ///
    /// # Errors
    /// Same contract as [`start_block_coro`](Self::start_block_coro), plus
    /// a `LuaRuntime` failure if the envelope cannot be rendered on this VM.
    pub fn resume_block_coro_answer(
        &self,
        program: &LuaProgram,
        thread: &Thread,
        answer: std::result::Result<Answer, Error>,
    ) -> Result<CoroStep> {
        let (envelope, retained) =
            render_envelope(&self.lua, answer).map_err(error::make_lua_error)?;
        let result = thread.resume::<MultiValue>(envelope);
        match self.step_block_coro(thread.clone(), result)? {
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
                if let Some((heading, line)) = self.take_jump()? {
                    return Ok(Ok(CoroStep::Done(LuaBlockResult::Jump(heading, line))));
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
    /// error, placed as [`kept_failure`] places a table's, unless the block
    /// was canceled or spent an Engine quota.
    fn answered_block_failure(
        &self,
        program: &LuaProgram,
        error: &mlua::Error,
        retained: Option<Retained>,
    ) -> Error {
        let (stashed, mapped) = match self.mapped_failure(program, error) {
            Ok(parts) => parts,
            Err(failure) => return failure,
        };
        if !is_fatal(&mapped)
            && let Some(retained) = retained
            && stashed.raised_table_is(&retained.table)
        {
            return placed_like(retained.error, &mapped);
        }
        kept_failure(stashed, mapped)
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
    /// instruction hook aborted under the run's cancel flag is a
    /// `Canceled` failure, whatever the raw error says; everything
    /// else maps through `LuaProgram::map_runtime_error_among` of `program`
    /// and of every library block `require` loaded into this VM, in the
    /// VM's prompt file.
    pub(crate) fn map_chunk_failure(&self, program: &LuaProgram, error: &mlua::Error) -> Error {
        if self.cancel_hook.is_canceled() {
            return Error::canceled();
        }
        match self.library_programs.lock() {
            Ok(libraries) => {
                program.map_runtime_error_among(error, &libraries, &prompt_file(&self.lua))
            }
            Err(_) => error::make_internal_error("require's loaded library programs were poisoned"),
        }
    }
}

/// Whether `mapped` is a cancellation or an exhausted Engine quota, which
/// keep their mapping whatever the block raised.
fn is_fatal(mapped: &Error) -> bool {
    matches!(mapped.code, ErrorCode::Canceled | ErrorCode::LuaQuota)
}

/// The failure a block keeps, given what the guard `stashed` and the
/// failure `mapped` to its prompt line: the mapping for a cancellation, an
/// Engine quota, a raise that is no error table, or a `lua`-kind table,
/// whose mapped runtime error keeps the same message with its source and
/// the mapped prompt line; any other structured table as the universal
/// error it stands for, placed on the mapped line when it has no location
/// yet. A raise that is no error table keeps its mapping, so an uncaught
/// direct `compactors.fail` call is its context exhaustion.
fn kept_failure(stashed: StashedFailure, mapped: Error) -> Error {
    if is_fatal(&mapped) {
        return mapped;
    }
    match stashed.raised {
        Some(raised) if error_kind(&raised) != ErrorKind::Lua => placed_like(raised, &mapped),
        _ => mapped,
    }
}

/// `error` placed on the prompt line `mapped` was located on, when the
/// mapping found one; an internal mapping's location is in Rust and
/// places nothing.
fn placed_like(error: Error, mapped: &Error) -> Error {
    match &mapped.at {
        Some(at) if mapped.code != ErrorCode::Internal => {
            place(error, &at.file, Some(at.start_line))
        }
        _ => error,
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
