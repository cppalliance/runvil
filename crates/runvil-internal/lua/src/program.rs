//! Compiled Lua chunks: bytecode compilation with debug info and chunk-line to prompt-line mapping.

use std::num::NonZeroU32;

use mlua::{Function, Lua, LuaOptions, StdLib};
use runvil_types::emitter::Emitter;
use runvil_types::error::ErrorCode;
use runvil_types::event::lifecycle;

use crate::chunk_line::{PROMPT_CHUNK_PREFIX, compile_line, map_chunk_line_to_absolute};
use crate::error::{
    self, Error, Result, authored_at, failure_text, lua_compile, lua_runtime, root_text,
};

/// The statement every prompt block compiles behind, on its first line so
/// no line moves. Lua compiles no tail call inside a to-be-closed
/// variable's scope, so a main chunk's `return f()` keeps the author's
/// frame on the stack for the call line to be read from; a nil
/// to-be-closed value is never closed.
const FRAME_GUARD: &str = "local __runvil_site <close> = nil; ";

/// The fixed temporary VM a chunk compiles in.
fn compiler() -> mlua::Result<Lua> {
    Lua::new_with(
        StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::UTF8,
        LuaOptions::default(),
    )
}

/// Compiles a named chunk in `lua` and keeps debug info.
fn compile_chunk(lua: &Lua, source: &str, chunk_name: &str) -> mlua::Result<Vec<u8>> {
    let function = lua.load(source).set_name(chunk_name).into_function()?;
    // Keep debug info so runtime errors report the chunk name and line
    // (`dump(true)` strips them and leaves `?:` in the traceback).
    Ok(function.dump(false))
}

/// Compiled Lua 5.5 source that can be loaded into multiple process-local VMs.
///
/// A program retains its original source for diagnostics and stores bytecode
/// produced once by Lua 5.5. The bytecode is an in-memory implementation detail:
/// it is not a stable or portable serialization format and must not be persisted.
///
/// Compilation does not execute the source. Loading a program ([`load`](Self::load))
/// creates a function in the supplied VM but likewise does not call it.
///
/// # Sensitivity
/// A program retains the author's original prompt Lua source verbatim, and
/// [`source`](Self::source) exposes it. Prompt source can embed
/// author-sensitive material (system instructions, embedded credentials in a
/// poorly written prompt, private policy text), so treat the value returned by
/// [`source`](Self::source) as sensitive: it is a full-fidelity diagnostic
/// accessor, not a value to log at info level, echo to untrusted sinks, or place
/// in a model-facing message. [`location`](Self::location) and
/// [`source_line`](Self::source_line) are safe positional metadata (a display
/// name and a line number) and hold no source text. The crate itself never logs the
/// retained source; compilation observations contain only fixed strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuaProgram {
    source: String,
    bytecode: Vec<u8>,
    /// Name the chunk was compiled under, which Lua renders in its errors as
    /// `[string "{chunk_name}"]`: `line {source_line}` for a prompt block,
    /// the explicit name for an internal chunk. Mapping finds a chunk's
    /// lines by this name, so it must be unique among the programs one VM
    /// maps through and stay under the 45 bytes Lua keeps whole.
    chunk_name: String,
    /// Parser location string that mapped runtime errors name (for example
    /// ``section `Web Search` epilog``).
    location: String,
    /// 1-based line number in the prompt source where this Lua region begins.
    ///
    /// A [`NonZeroU32`] so line zero is unrepresentable. Used together with
    /// chunk-relative line numbers from Lua runtime errors to produce an
    /// absolute prompt-source line: `source_line + chunk_line - 1`.
    source_line: NonZeroU32,
}

impl LuaProgram {
    /// Compiles `source` as Lua 5.5 bytecode without executing it.
    ///
    /// `location` identifies the source region in diagnostics, and `file`
    /// is the prompt file the region is in. The chunk
    /// itself compiles under the name `line {source_line}`, which Lua
    /// renders as `[string "line 120"]`: no two blocks of one prompt start
    /// on the same line, and the name never reaches the length at which Lua
    /// cuts it. The chunk compiles behind a statement on its first line
    /// that keeps a main-chunk tail call from dropping the author's frame;
    /// [`source`](Self::source) is the source as given. Compilation reports
    /// contain only fixed strings and never include `source` or `location`;
    /// each is stamped with the emitter's coordinates.
    ///
    /// # Errors
    /// Returns a `LuaCompile` failure when `source` is not syntactically
    /// valid: its message names the region and holds the Lua diagnostic,
    /// its `Section` fact names the region, it is located on the line of
    /// `file` the diagnostic names, and its author prose names what failed.
    /// Returns an `Internal` failure located at this crate's Rust source,
    /// whose message holds the `mlua` error's text, if the temporary
    /// compiler VM cannot be created.
    pub fn compile(
        source: &str,
        file: &str,
        location: &str,
        source_line: NonZeroU32,
        emitter: &Emitter,
        section: &str,
    ) -> Result<Self> {
        emitter.report(section, lifecycle::LUA_COMPILATION_STARTED);

        let chunk_name = format!("{PROMPT_CHUNK_PREFIX}{source_line}");
        let guarded = format!("{FRAME_GUARD}{source}");
        let compiled = compiler()
            .map_err(|error| error::internal(error.to_string()))
            .and_then(|lua| {
                compile_chunk(&lua, &guarded, &chunk_name).map_err(|error| {
                    let diagnostic = match &error {
                        mlua::Error::SyntaxError { message, .. } => message.clone(),
                        other => other.to_string(),
                    };
                    let line = compile_line(&chunk_name, source_line, &diagnostic);
                    authored_at(
                        lua_compile(location, source_line.get(), &error.to_string()),
                        file,
                        Some(line),
                        failure_text(&diagnostic),
                    )
                })
            });
        let bytecode = match compiled {
            Ok(bytecode) => bytecode,
            Err(error) => {
                emitter.report(section, lifecycle::LUA_COMPILATION_FAILED);
                return Err(error);
            }
        };

        emitter.report(section, lifecycle::LUA_COMPILATION_SUCCEEDED);
        Ok(Self {
            source: source.to_owned(),
            bytecode,
            chunk_name,
            location: location.to_owned(),
            source_line,
        })
    }

    /// Compiles one of the crate's embedded Lua chunks without observations.
    ///
    /// Internal chunks are crate source, not author prompt source, so the
    /// compilation is bookkeeping. `chunk_name` serves as both the chunk
    /// name and the display location; an `@`-prefixed name renders as a
    /// file path in errors, which the line mapper never rewrites. Every
    /// embedded chunk is included with `include_str!`, so chunk line 1 is
    /// file line 1, and compiles once in a `LazyLock`; a compile failure is
    /// a crate bug that each install clones.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure if the temporary compiler VM cannot be
    /// created or the source fails to compile.
    pub fn compile_internal(source: &str, chunk_name: &str) -> Result<Self> {
        let bytecode = compiler()
            .and_then(|lua| compile_chunk(&lua, source, chunk_name))
            .map_err(error::lua)?;
        Ok(Self {
            source: source.to_owned(),
            bytecode,
            chunk_name: chunk_name.to_owned(),
            location: chunk_name.to_owned(),
            source_line: NonZeroU32::MIN,
        })
    }

    /// Returns the original Lua source retained for diagnostics.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the 1-based prompt-source line where this Lua region begins.
    #[must_use]
    pub fn source_line(&self) -> NonZeroU32 {
        self.source_line
    }

    /// Loads the compiled function into `lua` without executing it.
    ///
    /// The bytecode is loaded only into a VM in the same process and is never
    /// exposed as a persistence format.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure if the VM rejects the internally
    /// compiled bytecode.
    pub fn load(&self, lua: &Lua) -> Result<Function> {
        lua.load(self.bytecode.as_slice())
            .into_function()
            .map_err(error::lua)
    }

    /// Maps a Lua runtime failure to its ordered core outcome, for a chunk
    /// that ran beside `loaded`, the programs of the libraries loaded into
    /// its VM, from the prompt file `file`.
    ///
    /// A typed internal or substitution failure that already has a
    /// location returns as it was raised, so an internal failure keeps its
    /// Rust line. A typed context exhaustion, such as an uncaught direct
    /// `compactors.fail` call's, and an Engine quota refusal, recognized by
    /// its typed value, keep their own failure, and every other failure,
    /// other typed failures included, returns a `LuaRuntime` failure with
    /// each chunk-relative line rewritten to an absolute prompt-source line
    /// by the program that owns that chunk, this one first when two share a
    /// chunk name. Nested errors from other chunks (for example a fanout
    /// arm) are left unchanged. Each is placed on the first line the
    /// mapping rewrote, of `file`, when it has no location yet, and the
    /// quota and the runtime failure get the author prose of the failure's
    /// own text. Cancellation is the VM's to classify: a chunk the
    /// instruction hook aborted under the run's cancel flag is a
    /// `Canceled` failure before the raw error reaches here.
    pub(crate) fn map_runtime_error_among(
        &self,
        error: &mlua::Error,
        loaded: &[Self],
        file: &str,
    ) -> Error {
        let typed = error.downcast_ref::<Error>();
        if let Some(located) = typed.filter(|typed| {
            matches!(typed.code, ErrorCode::Internal | ErrorCode::Substitution)
                && typed.at.is_some()
        }) {
            return located.clone();
        }
        let raw = error.to_string();
        let owners: Vec<(&str, &str, NonZeroU32)> = std::iter::once(self)
            .chain(loaded)
            .map(|program| {
                (
                    program.chunk_name.as_str(),
                    program.location(),
                    program.source_line,
                )
            })
            .collect();
        let (mapped, line) = map_chunk_line_to_absolute(&raw, &owners);
        let kept = typed.filter(|typed| {
            matches!(
                typed.code,
                ErrorCode::ContextExhausted | ErrorCode::LuaQuota
            )
        });
        let (failure, what) = match kept {
            Some(kept) => (kept.clone(), kept.message.clone()),
            None => (lua_runtime(mapped), root_text(error)),
        };
        authored_at(failure, file, line, failure_text(&what))
    }

    /// Display location recorded at compile time (parser location string),
    /// the name mapped runtime errors give this chunk.
    #[must_use]
    pub fn location(&self) -> &str {
        &self.location
    }
}

#[cfg(test)]
#[path = "program-tests.rs"]
mod tests;
