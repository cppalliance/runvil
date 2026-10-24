//! In-place compaction after an overflow: the compactor edits the
//! section's `messages` list itself, the loop validates the result and
//! retries the round once without counting it, and every failure puts the
//! records back as they were. The validation rules' cases sit in `rules`;
//! `models.compact`, the same compaction outside a loop, in
//! `models_compact`.

use runvil_parser::test_support::{one_section, prompt};

use super::fixtures::echo_tool;
use super::gateway::overflowing;
use super::models_loop::{catalog_scenario, loop_scenario};
use super::*;
use crate::model::CompletionErrorKind;

#[path = "compaction-rules.rs"]
mod rules;

/// The summarizing compactor: it keeps the system block and the last two
/// user turns and summarizes the rest into one user record, cutting on a
/// user record so no tool-call batch is split.
pub(super) const SUMMARIZE: &str = r#"local function compact(reason)
  local first = 1
  while messages[first] and messages[first].role == "system" do
    first = first + 1
  end
  local cut, turns = #messages + 1, 0
  for i = #messages, first, -1 do
    if messages[i].role == "user" then
      turns = turns + 1
      if turns == 2 then cut = i break end
    end
  end
  if cut == first then return compactors.fail(reason) end
  local lines = {}
  for i = first, cut - 1 do
    lines[#lines + 1] = messages[i].role .. ": " .. json.encode(messages[i].content)
  end
  local summary = models.infer("Summarize this conversation:\n" .. table.concat(lines, "\n"))
  messages:replace(first, cut - 1, {
    role = "user",
    content = "Summary of the earlier conversation: " .. summary,
    meta = { summary = true },
  })
end
"#;

/// The refusal of a loop started while another runs in the section.
const LOOP_RUNNING: &str = "a loop or a compactor is already running in this section; \
                            ask a side question with models.infer(prompt)";

/// The tail of every offer function's refusal after the render.
const FIXED: &str = "the section's tool list is fixed at its first models.loop round; make \
                     every offer before the first loop, or offer in another section";

#[test]
fn a_compaction_keeps_the_lists_identity_and_old_views_read_their_own_records() {
    let gateway = ScriptedChat::new(vec![text("done")]);
    let source = one_section(
        "messages:user(string.rep('x', 100000))
         local before, view = messages, messages[1]
         models.loop(function(reason)
           messages:replace(1, 1, { role = 'user', content = 'short' })
         end)
         return table.concat({
           tostring(before == messages), before[1].content, #view.content,
           #messages, messages[2].content,
         }, '|')",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "true|short|100000|2|done");
    assert_eq!(gateway.call_count(), 1, "only the retried round leaves");
}

#[test]
fn the_summarizing_compactor_lets_the_retried_round_succeed_under_a_cap_of_one() {
    let gateway = ScriptedChat::new(vec![text("short summary"), text("done")]);
    let lua = format!(
        "{SUMMARIZE}
         messages:system('be brief')
         messages:user(string.rep('x', 100000))
         messages:assistant('a1')
         messages:user('q2')
         messages:assistant('a2')
         messages:user('q3')
         models.loop(compact)
         return table.concat({{
           #messages, messages[2].content, tostring(messages[2].meta.summary),
           messages[#messages].content,
         }}, '|')"
    );
    let source = prompt("max_tool_iterations: 1", &[("Only", &lua)]);
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "6|Summary of the earlier conversation: short summary|true|done"
    );
    let bodies = gateway.requests();
    assert_eq!(
        bodies.len(),
        2,
        "the summary's infer, then the retried round"
    );
    assert!(
        bodies[0].messages[0]
            .content()
            .starts_with("Summarize this conversation:\nuser: \"xxx"),
        "the infer prompt holds the old turns"
    );
    let users: Vec<&str> = bodies[1]
        .messages
        .iter()
        .filter(|message| message.role() == Role::User)
        .map(crate::model::Message::content)
        .collect();
    assert_eq!(
        users,
        [
            "Summary of the earlier conversation: short summary\n\nq2",
            "q3"
        ],
        "the retry sends the compacted records"
    );
}

#[test]
fn a_retry_that_overflows_again_restores_the_records_and_raises_context_exhaustion() {
    let rounds = Arc::new(AtomicUsize::new(0));
    let source = one_section(
        "messages:user('a prompt long enough to shrink')
         local ok, err = pcall(models.loop, function(reason)
           messages:replace(1, 1, { role = 'user', content = 'short' })
         end)
         return table.concat({
           tostring(ok), err.kind, err.reason, #messages, messages[1].content,
         }, '|')",
    );
    let trace = loop_scenario(source).chat_with(overflowing(&rounds)).run();
    assert_eq!(
        trace.text(),
        "false|context_exhausted|provider|1|a prompt long enough to shrink"
    );
    assert_eq!(
        rounds.load(Ordering::SeqCst),
        2,
        "the round and its one retry"
    );
}

#[test]
fn a_retry_that_fails_another_way_keeps_the_compacted_records() {
    let gateway = ScriptedChat::new(vec![text("unreachable")]);
    let source = one_section(
        "messages:user(string.rep('x', 100000))
         local ok, err = pcall(models.loop, compactors.clear)
         return tostring(ok) .. '|' .. err.kind .. ':' .. tostring(err) .. '|' .. #messages",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "false|lua:messages must not be empty|0");
    assert_eq!(gateway.call_count(), 0, "no request leaves");
}

#[test]
fn a_compactor_that_raises_after_editing_restores_the_records_and_raises_its_failure() {
    let gateway = ScriptedChat::new(vec![text("unreachable")]);
    let source = one_section(
        "messages:user(string.rep('x', 100000))
         local function shrink_then(raise)
           return function(reason)
             messages:replace(1, 1, { role = 'user', content = 'short' })
             messages:user('more')
             raise(reason)
           end
         end
         local _, own = pcall(models.loop, shrink_then(function() error('gave up', 0) end))
         local own_kept = #messages .. ':' .. #messages[1].content
         local _, failed = pcall(models.loop, shrink_then(function() compactors.fail('manual') end))
         return table.concat({
           own, own_kept, failed.kind, failed.reason, #messages .. ':' .. #messages[1].content,
         }, '|')",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "gave up|1:100000|context_exhausted|manual|1:100000"
    );
    assert_eq!(gateway.call_count(), 0, "no request leaves");
}

#[test]
fn a_compactor_that_calls_a_tool_gets_its_result() {
    let gateway = ScriptedChat::new(vec![text("done")]);
    let source = one_section(
        "messages:user(string.rep('x', 100000))
         local got
         models.loop(function(reason)
           got = tools.call('tools/echo', { value = reason })
           messages:replace(1, 1, { role = 'user', content = got })
         end)
         return got .. '|' .. messages[1].content .. '|' .. messages[2].content",
    );
    let trace = catalog_scenario(source)
        .tool(echo_tool())
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "echoed: precheck|echoed: precheck|done");
}

#[test]
fn a_loop_inside_a_compactor_raises_loop_running() {
    let gateway = ScriptedChat::new(vec![text("unreachable")]);
    let source = one_section(
        "messages:user(string.rep('x', 100000))
         local _, err = pcall(models.loop, function()
           local _, nested = pcall(models.loop)
           error(nested, 0)
         end)
         return err.kind .. ':' .. tostring(err) .. '|' .. #messages",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), format!("lua:{LOOP_RUNNING}|1"));
    assert_eq!(gateway.call_count(), 0, "no request leaves");
}

#[test]
fn a_compaction_a_restore_and_a_clear_leave_the_rendered_tool_list_attached() {
    let gateway = ScriptedChat::new(vec![
        fail(
            CompletionErrorKind::ContextOverflow,
            400,
            "This model's maximum context length is 4096 tokens.",
        ),
        text("done"),
    ]);
    let source = one_section(
        "tools.offer('tools/echo')
         messages:user('a prompt long enough to shrink')
         models.loop(function()
           messages:replace(1, 1, { role = 'user', content = 'short' })
         end)
         local restored = pcall(models.compact, function()
           messages:user('scratch')
           error('undo', 0)
         end, 'restore')
         local kept = #messages
         models.compact(compactors.clear, 'start over')
         local offered, refusal = pcall(tools.offer, tools.adhoc({ name = 'grab', description = 'Local grab' }, function()
           return ''
         end))
         return table.concat({
           tostring(restored), kept, #messages, tostring(offered), tostring(refusal),
         }, '|')",
    );
    let trace = loop_scenario(source)
        .offer("echo", echo_tool())
        .chat(&gateway)
        .run();
    assert_eq!(
        trace.text(),
        format!("false|2|0|false|tools.offer: {FIXED}")
    );
    let bodies = gateway.requests();
    assert_eq!(bodies.len(), 2, "the overflowed round and its retry");
    assert_eq!(bodies[1].tool_names(), ["echo"]);
    let bytes: Vec<String> = bodies
        .iter()
        .map(|body| serde_json::to_string(&body.tools).expect("tool schemas serialize"))
        .collect();
    assert_eq!(bytes[0], bytes[1], "the retry sends the same tool bytes");
}
