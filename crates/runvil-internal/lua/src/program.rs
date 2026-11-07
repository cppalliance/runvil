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
mod tests {
    //! Tests for the compile path's location, the runtime mapping's location
    //! and author prose, a quota refusal's among them, the typed failures the
    //! mapping keeps, and the prompt line a block's chunk position names.

    use std::num::NonZeroU32;

    use mlua::Lua;
    use runvil_types::emitter::{Emitter, EventSink};
    use runvil_types::error::{ErrorCode, FactKey, Location};
    use serde_json::Value;

    use super::LuaProgram;
    use crate::chunk_line::{
        compile_line, first_prompt_line, map_chunk_line_to_absolute, prompt_line,
    };
    use crate::error::{QuotaResource, context_exhausted, internal, lua_quota};

    /// The file every program here comes from.
    const FILE: &str = "agents/a.md";

    /// The region every program here is.
    const REGION: &str = "section `S` prologue";

    /// The line `n`.
    fn line(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).expect("tests pass non-zero lines")
    }

    /// `source` compiled as the block of [`REGION`] in [`FILE`] that starts on
    /// `start`.
    fn compiled(source: &str, start: u32) -> LuaProgram {
        let emitter = Emitter::root(EventSink::default(), "test", false);
        LuaProgram::compile(source, FILE, REGION, line(start), &emitter, "S")
            .expect("the block compiles")
    }

    /// The failure running `program` in `lua` raises.
    fn raised(program: &LuaProgram, lua: &Lua) -> mlua::Error {
        program
            .load(lua)
            .expect("the bytecode loads")
            .call::<()>(())
            .expect_err("the block fails")
    }

    #[test]
    fn a_compile_line_maps_the_chunk_line_or_keeps_the_block_line() {
        let diagnostic = "[string \"line 40\"]:3: unexpected symbol near <eof>";
        assert_eq!(compile_line("line 40", line(40), diagnostic), line(42));
        assert_eq!(
            compile_line("line 41", line(41), diagnostic),
            line(41),
            "another chunk's line is not this block's"
        );
        assert_eq!(compile_line("line 40", line(40), "no chunk line"), line(40));
        let chunk = format!("line {}", u32::MAX);
        assert_eq!(
            compile_line(
                &chunk,
                NonZeroU32::MAX,
                &format!("[string \"{chunk}\"]:2: x")
            ),
            NonZeroU32::MAX,
            "an overflowing mapping keeps the block line"
        );
    }

    #[test]
    fn a_compile_failure_is_located_on_its_diagnostics_line_with_author_prose() {
        let emitter = Emitter::root(EventSink::default(), "test", false);
        let error = LuaProgram::compile(
            "local a = 1\nlocal b =",
            FILE,
            REGION,
            line(11),
            &emitter,
            "S",
        )
        .expect_err("the block must not compile");
        let diagnostic = "[string \"line 11\"]:2: unexpected symbol near <eof>";
        assert_eq!(error.code, ErrorCode::LuaCompile);
        assert_eq!(
            error.message,
            format!("lua compilation error at {REGION} (line 11): syntax error: {diagnostic}")
        );
        assert_eq!(error.at, Some(Location::line(FILE, line(12))));
        assert_eq!(
            error.facts.get(&FactKey::Section),
            Some(&Value::from(REGION))
        );
        assert_eq!(
            error.prose,
            format!(
                "unexpected symbol near <eof>. {}",
                ErrorCode::LuaCompile.prose()
            )
        );
    }

    #[test]
    fn a_block_keeps_its_source_and_its_lines_behind_the_frame_guard() {
        let source = "local x = 1\nreturn x";
        let program = compiled(source, 30);
        assert_eq!(program.source(), source);
        let lua = Lua::new();
        let raw = raised(&compiled("local x = 1\nerror('boom')", 30), &lua);
        assert!(
            raw.to_string().contains("[string \"line 30\"]:2: boom"),
            "the guard adds no line: {raw}"
        );
    }

    #[test]
    fn a_runtime_failure_is_located_on_its_mapped_line_with_author_prose() {
        let program = compiled("local x = 1\nerror('boom')", 50);
        let lua = Lua::new();
        let raw = raised(&program, &lua);
        let mapped = program.map_runtime_error_among(&raw, &[], FILE);
        let (message, _) =
            map_chunk_line_to_absolute(&raw.to_string(), &[("line 50", REGION, line(50))]);
        assert_eq!(mapped.code, ErrorCode::LuaRuntime);
        assert_eq!(mapped.message, message, "the message keeps its mapped text");
        assert_eq!(mapped.at, Some(Location::line(FILE, line(51))));
        assert_eq!(
            mapped.prose,
            format!("boom. {}", ErrorCode::LuaRuntime.prose())
        );
    }

    /// The failure running `program` raises in a Lua VM whose global `fail`
    /// raises `error` from Rust.
    fn raised_through(program: &LuaProgram, error: &runvil_types::error::Error) -> mlua::Error {
        let lua = Lua::new();
        let raise = error.clone();
        let fail = lua
            .create_function(move |_, ()| -> mlua::Result<()> {
                Err(mlua::Error::external(raise.clone()))
            })
            .expect("the callback builds");
        lua.globals()
            .set("fail", fail)
            .expect("the global installs");
        raised(program, &lua)
    }

    #[test]
    fn a_raised_internal_failure_keeps_its_rust_location_through_the_mapping() {
        let fault = internal("a test invariant broke");
        assert!(fault.at.is_some(), "an internal failure is located in Rust");
        let program = compiled("local x = 1\nfail()", 5);
        let mapped = program.map_runtime_error_among(&raised_through(&program, &fault), &[], FILE);
        assert_eq!(
            mapped, fault,
            "it is not recoded at the line that raised it"
        );
    }

    #[test]
    fn a_raised_context_exhaustion_is_placed_on_its_mapped_line() {
        let exhausted = context_exhausted("precheck");
        let program = compiled("local x = 1\nfail()", 5);
        let mapped =
            program.map_runtime_error_among(&raised_through(&program, &exhausted), &[], FILE);
        assert_eq!(
            (mapped.code, &mapped.message, &mapped.prose),
            (exhausted.code, &exhausted.message, &exhausted.prose),
            "it keeps its own failure and its prose"
        );
        assert_eq!(mapped.at, Some(Location::line(FILE, line(6))));
    }

    #[test]
    fn a_prompt_line_is_named_only_by_a_prompt_blocks_chunk() {
        assert_eq!(prompt_line("line 40", 3), Some(line(42)));
        assert_eq!(prompt_line("line 40", 0), Some(line(39)));
        for other in [
            "line 0",
            "line +4",
            "line ",
            "@pack:web",
            "probe",
            "lines 4",
        ] {
            assert_eq!(prompt_line(other, 3), None, "{other:?}");
        }
        assert_eq!(
            prompt_line(&format!("line {}", u32::MAX), 2),
            None,
            "an overflowing line is none"
        );
    }

    #[test]
    fn the_first_prompt_position_skips_function_markers_and_other_chunks() {
        let traceback = "stack traceback:\n\t[C]: in ?\n\
                     \t@crates/runvil-internal/lua/src/__impl_coro.lua:91: in function 'pcall'\n\
                     \t[string \"probe\"]:3: in function <[string \"line 7\"]:2>\n\
                     \t[string \"line 7\"]:4: in main chunk";
        assert_eq!(first_prompt_line(traceback), Some(line(10)));
        assert_eq!(
            first_prompt_line("in function <[string \"line 7\"]:2>"),
            None,
            "a function marker is no position"
        );
        assert_eq!(first_prompt_line("[string \"line 7\"]:: x"), None);
    }

    #[test]
    fn a_quota_refusal_raised_through_lua_is_recognized_at_the_boundary() {
        let lua = Lua::new();
        let log = lua
            .create_function(|_, ()| -> mlua::Result<()> {
                Err(mlua::Error::external(lua_quota(QuotaResource::LogEvent)))
            })
            .expect("the callback builds");
        lua.globals().set("log", log).expect("the global installs");
        let program = compiled("local x = 1\nlog()", 7);
        let mapped = program.map_runtime_error_among(&raised(&program, &lua), &[], FILE);
        let quota = lua_quota(QuotaResource::LogEvent);
        assert_eq!(
            (mapped.code, &mapped.message, &mapped.facts),
            (ErrorCode::LuaQuota, &quota.message, &quota.facts)
        );
        assert_eq!(mapped.at, Some(Location::line(FILE, line(8))));
        assert_eq!(
            mapped.prose,
            format!(
                "lua log event quota exceeded. {}",
                ErrorCode::LuaQuota.prose()
            )
        );
        let mimic = compiled("error('lua log event quota exceeded')", 7);
        assert_eq!(
            mimic
                .map_runtime_error_among(&raised(&mimic, &lua), &[], FILE)
                .code,
            ErrorCode::LuaRuntime,
            "an author who raises the same text fails as a runtime error"
        );
    }
}
