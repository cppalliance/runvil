//! Runvil's `require`: library blocks run over the caller's globals and
//! in its coroutine, return values, caching by library identity, cycles,
//! retries after a failed load, the path passed to the resolver, the
//! `jump` refusal during a load, library prose, library errors, and the
//! library-load reports.

use super::*;

mod environment;
mod errors;
mod jump;
mod prose;
mod reports;

/// One resolver entry: the path as written, and the library it resolves
/// to.
type Entry = (&'static str, Library);

/// Every path the resolver was asked for so far, as written.
type Calls = Arc<Mutex<Vec<String>>>;

/// A library under `identity` whose blocks are `sources`, each compiled
/// at the location `library {path}` from prompt line 100, with empty
/// prose. The line keeps the chunk name `line 100` apart from the line 1
/// of the caller `program` builds.
fn library(identity: &str, path: &str, sources: &[&str]) -> Library {
    Library {
        key: LibraryKey::new(identity.to_owned()),
        path: path.to_owned(),
        blocks: sources
            .iter()
            .map(|source| LibraryBlock {
                program: LuaProgram::compile(
                    source,
                    "prompt",
                    &format!("library {path}"),
                    NonZeroU32::new(100).expect("100 is non-zero"),
                )
                .expect("library Lua must compile"),
                prose: String::new(),
                line: None,
            })
            .collect(),
    }
}

/// A resolver over fixed entries that records every call. A path resolves
/// to the entry written the same way; any other names no section.
fn resolver(
    entries: Vec<Entry>,
    calls: &Calls,
) -> impl Fn(String) -> Result<Library> + Send + Sync + 'static {
    let calls = Arc::clone(calls);
    move |path| {
        calls
            .lock()
            .expect("the calls lock must not be poisoned")
            .push(path.clone());
        entries
            .iter()
            .find(|(written, _)| *written == path)
            .map(|(_, library)| library.clone())
            .ok_or_else(|| crate::make_lua_runtime_error(format!("section `{path}` not found")))
    }
}

/// A shim VM with `require` installed over `entries`, rendering library
/// prose verbatim and reporting through `emitter`, and the log of its
/// resolver calls.
#[track_caller]
fn require_vm(entries: Vec<Entry>, emitter: &Emitter) -> (BuiltVm, Calls) {
    require_vm_rendering(entries, |template, _, _| Ok(template.to_owned()), emitter)
}

/// [`require_vm`] with library prose rendered through `render`.
#[track_caller]
fn require_vm_rendering<P>(entries: Vec<Entry>, render: P, emitter: &Emitter) -> (BuiltVm, Calls)
where
    P: Fn(&str, Option<NonZeroU32>, ProseState<'_>) -> mlua::Result<String> + Send + Sync + 'static,
{
    let calls = Calls::default();
    let vm = TestVm::new().shims().build();
    vm.install_require(resolver(entries, &calls), render, emitter, TEST_SECTION)
        .expect("require must install");
    (vm, calls)
}

/// Runs `source` as a block coroutine to its end and returns its scalar.
fn returned(vm: &SectionVm, source: &str) -> Option<String> {
    match vm.start_block_coro(&program(source)) {
        Ok(CoroStep::Done(LuaBlockResult::Returned(value))) => value,
        other => panic!("the block must return, got {other:?}"),
    }
}

/// Runs `source` as a block coroutine and returns its failure's display.
fn failure(vm: &SectionVm, source: &str) -> String {
    match vm.start_block_coro(&program(source)) {
        Err(error) => error.to_string(),
        other => panic!("the block must fail, got {other:?}"),
    }
}

#[test]
fn require_hands_back_the_first_returned_value_of_any_type() {
    let (vm, _) = require_vm(
        vec![
            (
                "/table",
                library("t", "/table", &["return { name = 'tbl' }"]),
            ),
            (
                "/function",
                library("f", "/function", &["return function() return 'fn' end"]),
            ),
            ("/string", library("s", "/string", &["return 'text'"])),
            ("/false", library("n", "/false", &["return false"])),
        ],
        &null_emitter(),
    );
    let value = returned(
        &vm,
        "local t = require '/table'\n\
         local f = require '/function'\n\
         return type(t) .. ':' .. t.name .. '|' .. f() .. '|' .. (require '/string') .. '|' \
           .. tostring(require '/false')",
    );
    assert_eq!(value.as_deref(), Some("table:tbl|fn|text|false"));
}

#[test]
fn a_block_without_a_value_lets_the_next_block_run() {
    let (vm, _) = require_vm(
        vec![
            (
                "/later",
                library(
                    "later",
                    "/later",
                    &[
                        "first = 'ran'",
                        "return nil",
                        "return first .. ' then third'",
                    ],
                ),
            ),
            (
                "/silent",
                library("silent", "/silent", &["quiet = 1", "return nil"]),
            ),
        ],
        &null_emitter(),
    );
    let value = returned(
        &vm,
        "return (require '/later') .. '|' .. tostring(require '/silent')",
    );
    assert_eq!(value.as_deref(), Some("ran then third|true"));
}

#[test]
fn library_globals_reach_the_caller_and_its_locals_stay_inside() {
    let (vm, _) = require_vm(
        vec![(
            "/a",
            library("a", "/a", &["local hidden = 'local'\nexported = 'global'"]),
        )],
        &null_emitter(),
    );
    let value = returned(
        &vm,
        "require '/a'\nreturn exported .. ':' .. tostring(hidden)",
    );
    assert_eq!(value.as_deref(), Some("global:nil"));
}

#[test]
fn a_second_require_returns_the_saved_value_without_rerunning() {
    let (vm, _) = require_vm(
        vec![(
            "/a",
            library(
                "a",
                "/a",
                &["loads = (loads or 0) + 1\nreturn { count = loads }"],
            ),
        )],
        &null_emitter(),
    );
    let value = returned(
        &vm,
        "local first = require '/a'\n\
         local second = require '/a'\n\
         return tostring(first == second) .. ':' .. loads",
    );
    assert_eq!(value.as_deref(), Some("true:1"));
}

#[test]
fn a_saved_false_comes_back_without_a_rerun() {
    let (vm, _) = require_vm(
        vec![(
            "/a",
            library("a", "/a", &["loads = (loads or 0) + 1\nreturn false"]),
        )],
        &null_emitter(),
    );
    let value = returned(
        &vm,
        "local first, second = require '/a', require '/a'\n\
         return tostring(first) .. ':' .. tostring(second) .. ':' .. loads",
    );
    assert_eq!(value.as_deref(), Some("false:false:1"));
}

#[test]
fn a_library_requiring_a_loaded_library_gets_its_saved_value() {
    let (vm, _) = require_vm(
        vec![
            (
                "/a",
                library(
                    "a",
                    "/a",
                    &["loads = (loads or 0) + 1\nreturn 'value of a'"],
                ),
            ),
            (
                "/b",
                library("b", "/b", &["return 'b sees ' .. require '/a'"]),
            ),
        ],
        &null_emitter(),
    );
    let value = returned(&vm, "require '/a'\nreturn (require '/b') .. ':' .. loads");
    assert_eq!(value.as_deref(), Some("b sees value of a:1"));
}

#[test]
fn two_helpers_with_one_title_are_two_libraries() {
    let (vm, _) = require_vm(
        vec![
            ("/a", library("a", "/a", &["return require '/a/helpers'"])),
            ("/b", library("b", "/b", &["return require '/b/helpers'"])),
            (
                "/a/helpers",
                library("a/helpers", "/a/helpers", &["return 'helper of a'"]),
            ),
            (
                "/b/helpers",
                library("b/helpers", "/b/helpers", &["return 'helper of b'"]),
            ),
        ],
        &null_emitter(),
    );
    let value = returned(&vm, "return (require '/a') .. '|' .. (require '/b')");
    assert_eq!(value.as_deref(), Some("helper of a|helper of b"));
}

#[test]
fn one_library_loads_once_however_its_path_is_written() {
    let counted = || library("a", "/a", &["loads = (loads or 0) + 1\nreturn {}"]);
    let (vm, _) = require_vm(
        vec![("/a", counted()), (" / a ", counted())],
        &null_emitter(),
    );
    let value = returned(
        &vm,
        "local first, second = require '/a', require ' / a '\n\
         return tostring(first == second) .. ':' .. loads",
    );
    assert_eq!(value.as_deref(), Some("true:1"));
}

#[test]
fn a_two_library_cycle_raises_naming_both_sections_in_order() {
    let (vm, _) = require_vm(
        vec![
            ("/a", library("a", "/a", &["require '/b'"])),
            ("/b", library("b", "/b", &["require '/a'"])),
        ],
        &null_emitter(),
    );
    let error = failure(&vm, "require '/a'");
    assert!(
        error.contains("require cycle: /a -> /b -> /a"),
        "the cycle must name both sections in order, got {error}"
    );
}

#[test]
fn a_library_that_requires_itself_meets_the_cycle_error() {
    let (vm, _) = require_vm(
        vec![("/Main", library("main", "/Main", &["require '/Main'"]))],
        &null_emitter(),
    );
    let error = failure(&vm, "require '/Main'");
    assert!(
        error.contains("require cycle: /Main -> /Main"),
        "a self-require must close a cycle of one, got {error}"
    );
}

#[test]
fn a_failed_load_stays_out_of_the_cache_and_runs_again() {
    let (vm, _) = require_vm(
        vec![(
            "/a",
            library(
                "a",
                "/a",
                &[
                    "runs = (runs or 0) + 1\npartial = 'kept'\nif broken then error('library fault') end\nreturn 'loaded'",
                ],
            ),
        )],
        &null_emitter(),
    );
    let value = returned(
        &vm,
        "broken = true\n\
         local ok, err = pcall(require, '/a')\n\
         local kept = partial\n\
         broken = false\n\
         return tostring(ok) .. '|' .. tostring(err) .. '|' .. kept .. '|' .. (require '/a') \
           .. '|' .. runs",
    );
    assert_eq!(
        value.as_deref(),
        Some("false|[string \"line 100\"]:3: library fault|kept|loaded|2")
    );
}

#[test]
fn an_uncaught_library_failure_keeps_the_library_position() {
    let (vm, _) = require_vm(
        vec![("/a", library("a", "/a", &["\nerror('library fault')"]))],
        &null_emitter(),
    );
    let error = failure(&vm, "require '/a'");
    assert!(
        error.contains("library /a:101: library fault"),
        "the raise must still name the library block, got {error}"
    );
}

#[test]
fn the_resolver_gets_each_path_as_written_during_a_load_and_after_it() {
    let (vm, calls) = require_vm(
        vec![
            (
                "/a",
                library(
                    "a",
                    "/a",
                    &["require '/a/helpers'\nfunction later() return require ' /a/late ' end"],
                ),
            ),
            (
                "/a/helpers",
                library("a/helpers", "/a/helpers", &["return 'h'"]),
            ),
            (" /a/late ", library("late", "/a/late", &["return 'l'"])),
        ],
        &null_emitter(),
    );
    assert_eq!(
        returned(&vm, "require '/a'\nreturn later()").as_deref(),
        Some("l")
    );
    assert_eq!(
        *calls.lock().expect("the calls lock must not be poisoned"),
        ["/a", "/a/helpers", " /a/late "]
    );
}

#[test]
fn a_path_that_is_not_a_string_raises_without_reaching_the_resolver() {
    let (vm, calls) = require_vm(Vec::new(), &null_emitter());
    let value = returned(
        &vm,
        "local ok, err = pcall(require, 42)\n\
         return tostring(ok) .. '|' .. tostring(err)",
    )
    .expect("the block must return its text");
    assert!(
        value.starts_with("false|")
            && value.contains("require takes a section path such as '/helpers', got integer"),
        "a number must raise the path-type error, got {value}"
    );
    assert!(
        calls
            .lock()
            .expect("the calls lock must not be poisoned")
            .is_empty(),
        "a number must never reach the resolver"
    );
}

#[test]
fn a_suspending_library_call_yields_from_the_callers_coroutine() {
    let (vm, _) = require_vm(
        vec![(
            "/a",
            library("a", "/a", &["answer = call('../worker', 'job')"]),
        )],
        &null_emitter(),
    );
    let block = program("require '/a'\nreturn answer");
    let CoroStep::Yielded(thread, values) = vm.start_block_coro(&block).expect("the block starts")
    else {
        panic!("the library's call must suspend the caller's block");
    };
    match vm.request_from_yield(&values) {
        YieldParse::Request(Request::Call { target, input, .. }) => {
            assert_eq!(target, "../worker");
            assert_eq!(input.as_deref(), Some("job"));
        }
        other => panic!("expected a call request, got {other:?}"),
    }
    let step = vm
        .resume_block_coro(&block, &thread, (true, "worker said hi"))
        .expect("resuming finishes the load");
    assert!(
        matches!(step, CoroStep::Done(LuaBlockResult::Returned(Some(ref text))) if text == "worker said hi"),
        "the load must finish and the block return, got {step:?}"
    );
}
