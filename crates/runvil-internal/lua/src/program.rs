//! Compiled Lua chunks: bytecode compilation with debug info and chunk-line to prompt-line mapping.

use std::num::NonZeroU32;

use mlua::{Function, Lua, LuaOptions, StdLib};
use runvil_types::emitter::Emitter;
use runvil_types::error::ErrorCode;
use runvil_types::event::lifecycle;

use crate::error::{
    self, Error, Result, authored_at, failure_text, lua_compile, lua_runtime, root_text,
};

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
    /// cuts it. Compilation reports contain only fixed strings and never
    /// include `source` or `location`; each is stamped with the emitter's
    /// coordinates.
    ///
    /// # Errors
    /// Returns a `LuaCompile` failure when `source` is not syntactically
    /// valid: its message names the region and holds the Lua diagnostic,
    /// its `Section` fact names the region, and it is located on the line
    /// of `file` the diagnostic names, which its author prose names too.
    /// Returns an `Internal` failure, with the `mlua` error's text, if the
    /// temporary compiler VM cannot be created.
    pub fn compile(
        source: &str,
        file: &str,
        location: &str,
        source_line: NonZeroU32,
        emitter: &Emitter,
        section: &str,
    ) -> Result<Self> {
        emitter.report(section, lifecycle::LUA_COMPILATION_STARTED);

        let chunk_name = format!("line {source_line}");
        let compiled = compiler()
            .map_err(|error| {
                let code = ErrorCode::Internal;
                Error::new(code, error.to_string(), code.prose())
            })
            .and_then(|lua| {
                compile_chunk(&lua, source, &chunk_name).map_err(|error| {
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
    /// file path in errors, which the line mapper never rewrites.
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
    /// A typed context exhaustion, such as an uncaught direct
    /// `compactors.fail` call's, returns as it was raised. An Engine quota
    /// refusal, recognized by its typed value, returns its `LuaQuota`
    /// failure, and every other failure, other typed failures included,
    /// returns a `LuaRuntime` failure with each chunk-relative line
    /// rewritten to an absolute prompt-source line by the program that owns
    /// that chunk, this one first when two share a chunk name. Nested errors
    /// from other chunks (for example a fanout arm) are left unchanged.
    /// Either is located on the first line the mapping rewrote, of `file`,
    /// and its author prose names that line and the failure's own text.
    /// Cancellation is the VM's to classify: a chunk the instruction hook
    /// aborted under the run's cancel flag is an `Interrupted` failure
    /// before the raw error reaches here.
    pub(crate) fn map_runtime_error_among(
        &self,
        error: &mlua::Error,
        loaded: &[Self],
        file: &str,
    ) -> Error {
        let typed = error.downcast_ref::<Error>();
        if let Some(exhausted) = typed.filter(|typed| typed.code == ErrorCode::ContextExhausted) {
            return exhausted.clone();
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
        let (failure, what) = match typed.filter(|typed| typed.code == ErrorCode::LuaQuota) {
            Some(quota) => (quota.clone(), quota.message.clone()),
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

/// The prompt-source line a compiler `diagnostic` names, for the chunk
/// named `chunk_name` that starts on `source_line`: the chunk-relative
/// line after the chunk's marker, mapped to its absolute line, or
/// `source_line` when the diagnostic names none or the mapping overflows.
fn compile_line(chunk_name: &str, source_line: NonZeroU32, diagnostic: &str) -> NonZeroU32 {
    let marker = format!("[string \"{chunk_name}\"]:");
    diagnostic
        .find(&marker)
        .map(|at| &diagnostic[at + marker.len()..])
        .and_then(|rest| {
            let digits = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            absolute_line(source_line, &rest[..digits])
        })
        .and_then(NonZeroU32::new)
        .unwrap_or(source_line)
}

/// Rewrites chunk-relative line numbers for the named chunks in `owners`,
/// each a chunk name, the display location it maps to, and the
/// prompt-source line its chunk starts at, to absolute prompt-source lines,
/// and returns the rewritten message beside the first line it rewrote.
///
/// Only `[string "{chunk name}"]:N:` occurrences of an owner's chunk name
/// are rewritten, each through the first owner whose chunk name it names, to
/// `{location}:{line}:`, the form Lua gives a file chunk. No later pass
/// matches that form, so a parent VM that surfaces a called or fanout
/// child's mapped error leaves the child's absolute lines alone, even for a
/// library both VMs loaded. The message gains a single leading
/// `{location}:{line}:` tag for the first occurrence rewritten; a message
/// with none passes through unchanged, with no line. A chunk line whose
/// absolute mapping would overflow `u32` keeps its original marker and
/// digits.
pub(crate) fn map_chunk_line_to_absolute(
    message: &str,
    owners: &[(&str, &str, NonZeroU32)],
) -> (String, Option<NonZeroU32>) {
    const OPENING: &str = "[string \"";
    let markers: Vec<(String, &str, NonZeroU32)> = owners
        .iter()
        .filter(|(chunk_name, location, _)| !chunk_name.is_empty() && !location.is_empty())
        .map(|&(chunk_name, location, source_line)| {
            (format!("{OPENING}{chunk_name}\"]:"), location, source_line)
        })
        .collect();
    let mut result = String::with_capacity(message.len() + 64);
    let mut rest = message;
    let mut tag: Option<(&str, u32)> = None;
    while let Some(start) = rest.find(OPENING) {
        result.push_str(&rest[..start]);
        let at = &rest[start..];
        let Some((marker, location, source_line)) = markers
            .iter()
            .find(|(marker, ..)| at.starts_with(marker.as_str()))
        else {
            result.push_str(OPENING);
            rest = &at[OPENING.len()..];
            continue;
        };
        let after = &at[marker.len()..];
        let digit_end = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        if let Some(absolute) = absolute_line(*source_line, &after[..digit_end]) {
            tag.get_or_insert((location, absolute));
            result.push_str(location);
            result.push(':');
            result.push_str(&absolute.to_string());
            rest = &after[digit_end..];
        } else {
            result.push_str(marker);
            rest = after;
        }
    }
    result.push_str(rest);

    if let Some((location, absolute)) = tag {
        // Leading `location:line:` tag naming the prompt file and its
        // absolute line: `briefer.md:51: ...`
        (
            format!("{location}:{absolute}: {result}"),
            NonZeroU32::new(absolute),
        )
    } else {
        (result, None)
    }
}

/// The absolute prompt-source line of `chunk_line`, a chunk-relative line's
/// digits, in a chunk starting at `source_line`: the two summed, less one.
/// `None` when the digits or the sum overflow `u32`, so a pathological line
/// count cannot wrap into a wrong number.
fn absolute_line(source_line: NonZeroU32, chunk_line: &str) -> Option<u32> {
    let Ok(chunk_line) = chunk_line.parse::<u32>() else {
        return None;
    };
    match source_line.get().checked_add(chunk_line) {
        Some(sum) => sum.checked_sub(1),
        None => None,
    }
}

#[cfg(test)]
#[path = "program-tests.rs"]
mod tests;
