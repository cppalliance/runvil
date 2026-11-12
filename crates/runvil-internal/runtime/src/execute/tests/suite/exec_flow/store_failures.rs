//! How store failures reach the author and the run: caught error tables,
//! uncaught run errors, the store view's confinement and the line its
//! failure is placed on, and write conflicts from library code.

use runvil_lua::VfsOp;
use runvil_types::error::ErrorCode;

use super::*;

use crate::execute::{Effect, EffectRecord};
use crate::test_support::at_line;
use crate::test_support::rig::Rig;

/// A store failure the author caught with `pcall` is an error table of
/// kind `store`: its `code`, `reason`, `path`, and `anchor` fields and its
/// integer `count` read from Lua, and `tostring` returns the store code's
/// remedy prose after the failing call's `file:line`.
#[test]
fn a_caught_store_error_is_a_store_table_with_reason_fields_and_prose() {
    let source = one_section(&format!(
        "
        local ok, err = pcall(function() return store.read('missing.txt') end)
        assert(ok == false, 'the missing read fails')
        assert(err.kind == 'store', 'kind is store, got ' .. tostring(err.kind))
        assert(err.code == 'not_found', 'code is not_found, got ' .. tostring(err.code))
        assert(err.reason == 'not_found', 'reason is not_found, got ' .. tostring(err.reason))
        assert(err.path == 'missing.txt', 'the path field names the file')
        assert(tostring(err) == 'prompt:12: {}', 'tostring is the prose, got ' .. tostring(err))
        store.write('a.txt', 'na na na')
        local ok2, err2 = pcall(function() store.str_replace('a.txt', 'na', 'la') end)
        assert(ok2 == false, 'the ambiguous anchor fails')
        assert(err2.reason == 'anchor', 'reason is anchor, got ' .. tostring(err2.reason))
        assert(err2.anchor == 'na', 'the anchor field names the anchor')
        assert(type(err2.count) == 'number' and err2.count % 1 == 0, 'count is an integer')
        assert(err2.count == 3, 'count is the match count')
        assert(tostring(err2) == 'prompt:20: {}', 'tostring is the prose, got ' .. tostring(err2))
        return 'ok'",
        ErrorCode::NotFound.prose(),
        ErrorCode::Anchor.prose()
    ));
    assert_eq!(Scenario::new(source).run().text(), "ok");
}

/// A store view the chain cannot derive ends the chain without reaching
/// Lua, so it is placed on the store call's line. The run's start probe
/// rules out a missing store, so the hook re-dispatches the parked write
/// through an access whose handle declares none.
#[test]
fn a_store_view_failure_is_placed_on_the_store_calls_line() {
    let source = one_section(
        "local ready = true
         store.write('a.txt', 'x')",
    );
    let mut rig =
        Rig::start(Scenario::new(source).hold(|effect| matches!(effect, Effect::Vfs { .. })));
    rig.settle();
    let error = rig
        .scheduler_for_test()
        .dispatch_store_without_a_store_for_test(VfsOp::Write {
            path: "a.txt".to_owned(),
            contents: "x".to_owned(),
        })
        .expect_err("a handle that declares no store has no store view");
    assert_eq!(error.code, ErrorCode::NoStore, "{error:?}");
    assert_eq!(error.at, at_line(13), "{error:?}");
}

/// A caught `store.read` failure names the read's own line, which the read's
/// yield recorded, not the line of the `pcall` that caught it.
#[test]
fn a_caught_store_read_failure_names_its_calls_file_and_line() {
    let source = one_section(
        "
        local ok, err = pcall(function()
          local text = store.read('missing.txt')
          return text
        end)
        return tostring(err)",
    );
    assert_eq!(
        Scenario::new(source).run().text(),
        format!("prompt:13: {}", ErrorCode::NotFound.prose())
    );
}

/// A store argument type error stays kind `lua`, like every other Engine
/// function's argument errors: only what the VFS reports is kind `store`.
#[test]
fn a_store_argument_type_error_is_kind_lua() {
    let source = one_section(
        "
        local ok, err = pcall(function() store.write(123, 'x') end)
        assert(ok == false, 'the non-string path fails')
        assert(err.kind == 'lua', 'an argument error is kind lua, got ' .. tostring(err.kind))
        local ok2, err2 = pcall(function() store.glob('a') end)
        return 'ok'",
    );
    assert_eq!(Scenario::new(source).run().text(), "ok");
}

/// An uncaught store failure ends the run with the store's code, and its
/// message is the model-facing rendering.
#[test]
fn an_uncaught_store_failure_ends_the_run_as_a_store_error() {
    let trace = Scenario::new(one_section("return store.read('missing.txt')")).run();
    let error = trace.error();
    assert_eq!(
        error.code,
        ErrorCode::NotFound,
        "the run fails as a store error: {error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("file not found in store: missing.txt"),
        "the message is the store's rendering: {error}"
    );
}

/// An uncaught store failure in the H1 pass ends the run with the store's
/// code: the H1 blocks run through the same coroutine machinery as any
/// section.
#[test]
fn an_uncaught_store_failure_in_h1_ends_the_run_as_a_store_error() {
    let source = PromptSource::new()
        .h1("return store.read('missing.txt')")
        .prose("Only", "Done.");
    let trace = Scenario::new(source.build()).run();
    let error = trace.error();
    assert_eq!(
        error.code,
        ErrorCode::NotFound,
        "the H1 failure is a store error: {error:?}"
    );
}

/// A caught store error raised again, after another suspending call,
/// keeps its code: the run ends with the store's code, and its message is
/// the original rendering.
#[test]
fn a_caught_store_error_raised_again_after_a_suspending_call_keeps_store() {
    let source = prompt(
        "",
        &[
            (
                "First",
                "
                local ok, err = pcall(function() return store.read('missing.txt') end)
                assert(ok == false, 'the missing read fails')
                call('Sibling', '')
                error(err)",
            ),
            ("### Sibling", "return 'sibling'"),
        ],
    );
    let trace = Scenario::new(source).run();
    let error = trace.error();
    assert_eq!(
        error.code,
        ErrorCode::NotFound,
        "the re-raised error keeps its store code: {error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("file not found in store: missing.txt"),
        "the message is the original rendering: {error}"
    );
}

/// A store declared at `/` covers only its own mount: a directory
/// mounted beneath it lies outside the store view, so a file that
/// directory holds reads as absent from the store.
#[test]
fn a_store_at_the_root_cannot_reach_a_mount_beneath_it() {
    let source = one_section(
        "
        local ok, err = pcall(function() return store.read('mount/secret.txt') end)
        assert(ok == false, 'the mounted path is not in the store')
        assert(err.reason == 'not_found', 'the mounted file is simply absent: ' .. tostring(err))
        return 'ok'",
    );
    let vfs = runvil_vfs::VfsRef::builder()
        .store("/", runvil_vfs::MemoryBackend::new())
        .mount("/mount", runvil_vfs::MemoryBackend::new())
        .build();
    // The mount holds the file; the store view reads only its own
    // mount.
    vfs.acquire(runvil_vfs::Origin::new("mount seeding"))
        .expect("the handle acquires")
        .write("/mount/secret.txt", b"secret")
        .expect("the mount seeds");
    let trace = Scenario::new(source).vfs(vfs).run();
    assert_eq!(
        trace.text(),
        "ok",
        "the store view is confined to its own mount"
    );
}

/// The `## Parent` that fans `### Worker` out over `alpha` and `beta`, each
/// arm requiring `## seeder`, whose body is `seeder`.
fn arms_requiring_a_seeder(seeder: &str) -> String {
    prompt(
        "",
        &[
            (
                "Parent",
                "
                local r = tasks.fanout('Worker', {'alpha', 'beta'})
                return r[1].text",
            ),
            (
                "### Worker",
                "
                require '/seeder'
                return item",
            ),
            ("seeder", seeder),
        ],
    )
}

/// A library's top-level write runs under each requiring arm's own
/// identity: the second arm's load meets the first's standing write
/// claim and the run ends with a make_determinism_error violation.
#[test]
fn a_library_write_across_two_arms_ends_the_run_with_determinism() {
    let source = arms_requiring_a_seeder("store.write('seeded.txt', 'seed')");
    let trace = Scenario::new(source).run();
    let error = trace.error();
    assert!(
        error.code == ErrorCode::Determinism,
        "the load-time conflict is a determinism violation: {error:?}"
    );
}

/// A conflict a library caught with its own `pcall` while loading still
/// ends the run with a make_determinism_error violation.
#[test]
fn a_library_conflict_caught_with_pcall_still_ends_the_run_with_determinism() {
    let source = arms_requiring_a_seeder(
        "local ok = pcall(function() store.write('seeded.txt', 'seed') end)",
    );
    let trace = Scenario::new(source).run();
    let error = trace.error();
    assert!(
        error.code == ErrorCode::Determinism,
        "a caught load-time conflict is still fatal: {error:?}"
    );
}

/// The library the captured-store cases require: `store.write` captured
/// in a local at load time, reached later only through a helper.
const CAPTURED_STORE_WRITE: &str = "
    local write = store.write
    function save_note(path, text)
      write(path, text)
    end";

/// A store function a library captured at load time yields like
/// `store.*` when called after the load, so two arms racing on one path
/// through it end the run with a make_determinism_error violation even though each
/// arm caught its own failure.
#[test]
fn a_captured_store_function_conflict_caught_with_pcall_still_ends_the_run_with_determinism() {
    let source = prompt(
        "",
        &[
            (
                "Parent",
                "
                local r = tasks.fanout('Worker', {'alpha', 'beta'})
                return r[1].text",
            ),
            (
                "### Worker",
                "
                require '/notes'
                pcall(save_note, 'note.txt', item)
                return item",
            ),
            ("notes", CAPTURED_STORE_WRITE),
        ],
    );
    let trace = Scenario::new(source).run();
    let error = trace.error();
    assert!(
        error.code == ErrorCode::Determinism,
        "a conflict through a captured store function is fatal: {error:?}"
    );
}

/// A store function a library captured at load time reaches the caller
/// as an `Effect::Vfs` when called after load, like `store.*` does.
#[test]
fn a_captured_store_function_called_after_load_reaches_the_caller_as_a_store_effect() {
    let source = prompt(
        "",
        &[
            (
                "Only",
                "
                require '/notes'
                save_note('note.txt', 'kept')
                return 'saved'",
            ),
            ("notes", CAPTURED_STORE_WRITE),
        ],
    );
    let trace = Scenario::new(source).run();
    assert_eq!(trace.text(), "saved");
    assert!(
        trace.effects().iter().any(|(effect, _)| matches!(
            effect,
            EffectRecord::Vfs { op: VfsOp::Write { path, .. } } if path == "note.txt"
        )),
        "the captured write is performed by the test driver: {:?}",
        trace.effects()
    );
}
