//! Contract tests for `models.loop`: the behaviors the loop's earlier
//! suites leave unpinned, each driven through the public Lua surface over
//! the section's `messages` list and asserting only what an author or the
//! caller sees. The default and argument compactor, the empty-list check,
//! an uncaught argument error, the single return value, and the
//! compactor call sit here; the exit rules, the batch rule, tool results,
//! and the event trace sit in `rounds`.

use runvil_parser::test_support::{one_section, prompt};

use super::fixtures::structured_tool;
use super::gateway::overflowing;
use super::models_loop::loop_scenario;
use super::*;
use crate::test_support::scenario::Trace;

#[path = "models_loop_contract-rounds.rs"]
mod rounds;

/// Runs `source` against a model whose every round is a provider
/// overflow, returning the run's trace and the rounds the model answered.
fn run_overflowing(source: String) -> (Trace, usize) {
    let rounds = Arc::new(AtomicUsize::new(0));
    let trace = loop_scenario(source).chat_with(overflowing(&rounds)).run();
    (trace, rounds.load(Ordering::SeqCst))
}

#[test]
fn the_default_compactor_is_whatever_compactors_fail_holds_at_the_call() {
    // The default is read from `compactors.fail` at each call and never
    // checked: an author's replacement becomes the default, and a
    // non-function there lets an ordinary round run and fails only at an
    // overflow round, with Lua's own call error as a string. `compactors`
    // is per VM, so each section sets its own.
    let gateway = ScriptedChat::new(vec![text("fine")]);
    let source = prompt(
        "",
        &[
            (
                "Replaced",
                "messages:user(string.rep('x', 100000))
                 compactors.fail = function(reason) error('replaced: ' .. reason, 0) end
                 local _, replaced = pcall(models.loop)
                 var.replaced = replaced .. '|' .. #messages",
            ),
            (
                "Number",
                "compactors.fail = 42
                 messages:user('hello')
                 models.loop()
                 local ordinary = #messages
                 messages:user(string.rep('x', 100000))
                 local _, called = pcall(models.loop)
                 return var.replaced .. '|' .. ordinary .. '|' .. type(called) .. '|' .. called",
            ),
        ],
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "replaced: precheck|1|2|string|attempt to call a number value"
    );
    assert_eq!(
        gateway.call_count(),
        1,
        "only the ordinary round sends a request"
    );
}

#[test]
fn a_light_userdata_compactor_is_named_userdata_as_lua_names_it() {
    // A JSON null in a structured tool's result is a null light userdata,
    // which Lua's `type()` calls `userdata`.
    let gateway = ScriptedChat::new(vec![text("unreachable")]);
    let source = one_section(
        "tools.offer('tools/structured')
         local null = tools.call('tools/structured', {}).none
         local other = models.get('other')
         messages:user('hello')
         local ok, err = pcall(other.loop, other, null)
         assert(not ok, 'a light userdata compactor is refused')
         return type(null) .. '|' .. err.kind .. '|' .. tostring(err) .. '|' .. #messages",
    );
    let trace = loop_scenario(source)
        .offer("structured", structured_tool("{\"none\":null}", true))
        .chat(&gateway)
        .run();
    assert_eq!(
        trace.text(),
        "userdata|lua|compactor must be a function, got userdata|1"
    );
    assert_eq!(gateway.call_count(), 0, "no round runs");
}

#[test]
fn the_arity_error_still_raises_when_the_author_has_rebound_error() {
    // A shim raise that read the global `error` would call the author's
    // no-op instead of raising. Had that sent a round, the run would wait
    // on nothing but the held round, and the cancel ends it `Cancelled`;
    // had it spun in the block, nextest's timeout ends the test.
    let gateway = ScriptedChat::new(vec![text("unreachable")]);
    let source = one_section(
        "error = function() end
         messages:user('hello')
         local ok, err = pcall(models.loop, nil, nil)
         return tostring(err)",
    );
    let mut driving = loop_scenario(source)
        .chat(&gateway)
        .hold(|effect| matches!(effect, Effect::Chat { .. }))
        .start();
    while driving.step() {}
    driving.cancel();
    let trace = driving.finish();
    assert!(
        !matches!(trace.result(), RunResult::Cancelled),
        "the raise ends the call before the cancel: {:?}",
        trace.result()
    );
    assert_eq!(trace.text(), "models.loop takes (compactor?)");
    assert!(trace.effects().is_empty(), "no round runs");
}

#[test]
fn an_empty_list_raises_before_any_round() {
    let gateway = ScriptedChat::new(vec![text("unreachable")]);
    let source = one_section(
        "local ok, err = pcall(models.loop)\n\
         assert(not ok, 'an empty list raises')\n\
         return err.kind .. ':' .. tostring(err)",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "lua:messages must not be empty");
    assert_eq!(gateway.call_count(), 0, "the refused call sends no request");
}

#[test]
fn an_uncaught_loop_arity_error_fails_the_run_naming_the_authors_prompt_line() {
    // The loop block opens at prompt line 12, so its second line is 13.
    let gateway = ScriptedChat::new(vec![text("unreachable")]);
    let source = one_section(
        "messages:user('hello')
         models.loop(nil, nil)
         return 'unreachable'",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    match trace.error().inner() {
        Error::Lua {
            message,
            cause: Some(_),
        } => {
            assert!(
                message.contains("models.loop takes (compactor?)"),
                "the runtime error keeps the shim's message: {message}"
            );
            assert!(
                message.contains("section `Only` prologue:13: in"),
                "the runtime error names the author's prompt line: {message}"
            );
        }
        other => panic!("expected the mapped runtime error, got {other:?}"),
    }
    assert_eq!(gateway.call_count(), 0, "no round runs");
}

#[test]
fn each_loop_entry_returns_exactly_one_value() {
    let gateway = ScriptedChat::new(vec![text("one"), text("two")]);
    let source = one_section(
        "messages:user('first')
         local n = select('#', models.loop())
         messages:user('second')
         local other = models.get('other')
         local m = select('#', other:loop())
         return n .. '|' .. m .. '|' .. #messages",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "1|1|4");
}

#[test]
fn the_compactor_gets_only_the_reason_and_may_suspend_before_raising() {
    // The compactor runs inside the block, so its own `models.infer`
    // round suspends and resumes like any other.
    let gateway = ScriptedChat::new(vec![text("summary")]);
    let source = one_section(
        "messages:user(string.rep('x', 100000))
         local count, summary
         local ok, err = pcall(models.loop, function(...)
           count = select('#', ...)
           summary = models.infer('summarize')
           error('compacted after ' .. summary, 0)
         end)
         return tostring(ok) .. '|' .. count .. '|' .. err .. '|' .. #messages",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "false|1|compacted after summary|1");
    assert_eq!(
        gateway.call_count(),
        1,
        "the compactor's infer round is the only request"
    );
}

#[test]
fn a_returning_compactor_that_shrank_nothing_raises_the_exact_exhaustion_text() {
    let (trace, rounds) = run_overflowing(one_section(
        "messages:user('a small prompt')
         local ok, err = pcall(models.loop, function() return 'summary' end)
         assert(not ok, 'a compactor that shrank nothing raises')
         return err.kind .. '|' .. tostring(err) .. '|' .. #messages",
    ));
    assert_eq!(
        trace.text(),
        "context_exhausted|context exhausted: the provider rejected the request as exceeding \
         the model's context window|1"
    );
    assert_eq!(rounds, 1, "the one round left and overflowed");
}

#[test]
fn a_compactors_own_table_raise_reaches_the_caller_as_the_same_table() {
    let (trace, rounds) = run_overflowing(one_section(
        "messages:user('a small prompt')
         local own = { reason = 'mine' }
         local ok, err = pcall(models.loop, function() error(own) end)
         return tostring(ok) .. '|' .. tostring(err == own) .. '|' \
         .. tostring(getmetatable(err) == nil) .. '|' .. #messages",
    ));
    assert_eq!(trace.text(), "false|true|true|1");
    assert_eq!(rounds, 1, "the one round left and overflowed");
}
