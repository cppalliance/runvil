//! `models.compact(compactor, reason)`: one compaction of the section's
//! `messages` list outside any loop. It runs the compactor with the
//! caller's reason, applies only the structural rules, puts the records
//! back on any failure, returns nothing, and refuses to start while a
//! loop or a compactor runs. Compaction after an overflow sits in
//! `compaction`.

use runvil_parser::test_support::one_section;

use super::models_loop::loop_scenario;
use super::*;

/// The refusal of a compaction started while a loop or a compactor runs.
const COMPACT_RUNNING: &str =
    "models.compact cannot start while a loop or a compactor is running in this section";

#[test]
fn compacting_with_clear_starts_the_conversation_over_and_a_later_loop_runs_normally() {
    let gateway = ScriptedChat::new(vec![text("a1"), text("a2")]);
    let source = one_section(
        "messages:user('one')
         models.loop()
         models.compact(compactors.clear, 'manual compaction')
         local cleared = #messages
         messages:user('two')
         models.loop()
         return cleared .. '|' .. #messages .. '|' .. messages[2].content",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "0|2|a2");
    let bodies = gateway.requests();
    assert_eq!(bodies.len(), 2);
    let sent: Vec<&str> = bodies[1]
        .messages
        .iter()
        .map(crate::model::Message::content)
        .collect();
    assert_eq!(sent, ["two"], "the later round sends only the new record");
}

#[test]
fn a_compactor_that_drops_the_last_reply_runs_and_returns_nothing() {
    let gateway = ScriptedChat::new(vec![text("a1")]);
    let source = one_section(
        "messages:user('one')
         models.loop()
         local seen
         local result = models.compact(function(reason)
           seen = reason
           messages:replace(#messages, #messages)
         end, 'drop the failed reply')
         return table.concat({ tostring(result), seen, #messages, messages[1].content }, '|')",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "nil|drop the failed reply|1|one");
}

#[test]
fn models_compact_refuses_while_a_loop_its_handler_or_a_compactor_runs() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("c1", "probe", "{\"value\":\"x\"}")]),
        text("done"),
    ]);
    let source = one_section(
        "local seen = {}
         local function refuse(label)
           local ok, err = pcall(models.compact, compactors.clear, label)
           seen[#seen + 1] = label .. ':' .. tostring(ok) .. ':' .. err.kind .. ':' .. tostring(err)
         end
         tools.offer_local('probe', 'Probe models.compact', { value = 'string' }, function()
           refuse('handler')
           return 'probed'
         end)
         messages:user('go')
         models.loop()
         messages:user(string.rep('x', 100000))
         pcall(models.loop, function()
           refuse('compactor')
           error('stop', 0)
         end)
         models.compact(function() refuse('compact') end, 'outer')
         return table.concat(seen, '\\n') .. '\\n' .. #messages",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    let expected: Vec<String> = ["handler", "compactor", "compact"]
        .iter()
        .map(|label| format!("{label}:false:lua:{COMPACT_RUNNING}"))
        .chain(["5".to_owned()])
        .collect();
    assert_eq!(trace.text().lines().collect::<Vec<_>>(), expected);
    assert_eq!(gateway.call_count(), 2, "no refusal sends a round");
}

#[test]
fn models_compact_applies_the_structural_rules_but_not_the_size_rules() {
    let gateway = ScriptedChat::new(vec![text("unreachable")]);
    let source = one_section(
        "messages:user(string.rep('x', 100000))
         models.compact(function() end, 'no shrink needed')
         local _, broken = pcall(models.compact, function()
           messages:replace(1, 1, { role = 'user', content = 'short' })
           messages:assistant('', { { id = 'c1', name = 'echo', arguments = { value = 'v' } } })
         end, 'broken')
         local broken_kept = #messages .. ':' .. #messages[1].content
         local _, raised = pcall(models.compact, function()
           messages:replace(1, 1)
           error('gave up', 0)
         end, 'raised')
         return table.concat({
           broken.kind, tostring(broken), broken_kept, raised,
           #messages .. ':' .. #messages[1].content,
         }, '|')",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "lua|messages[2] tool call \"c1\" has no tool result|1:100000|gave up|1:100000"
    );
    assert_eq!(gateway.call_count(), 0, "no request leaves");
}

#[test]
fn models_compact_checks_the_compactor_then_the_reason() {
    let gateway = ScriptedChat::new(vec![text("unreachable")]);
    let source = one_section(
        "messages:user('kept')
         local function refusal(...)
           local ok, err = pcall(models.compact, ...)
           return tostring(ok) .. ':' .. err.kind .. ':' .. tostring(err)
         end
         return table.concat({
           refusal(),
           refusal('clear', 'reason'),
           refusal(compactors.clear),
           refusal(compactors.clear, 42),
           refusal(compactors.clear, 4.5),
           refusal(compactors.clear, 'reason', nil),
           #messages,
         }, '\\n')",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    let refusals = [
        "compactor must be a function, got nil",
        "compactor must be a function, got string",
        "reason must be a string, got nil",
        "reason must be a string, got integer",
        "reason must be a string, got number",
        "models.compact takes (compactor, reason)",
    ]
    .map(|message| format!("false:lua:{message}"));
    let lines: Vec<&str> = trace.text().lines().collect();
    assert_eq!(lines[..6], refusals);
    assert_eq!(lines[6], "1", "a refused call leaves the list alone");
}
