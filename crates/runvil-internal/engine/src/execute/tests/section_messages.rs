//! The section's one `messages` list: every section entry gets a fresh,
//! empty list with its VM, which keeps its identity for the section's
//! whole life, is shared with the libraries the section requires, and
//! cannot be reassigned or written through. Both loop entries run over it
//! and take only `(compactor?)`.

use runvil_parser::test_support::PromptSource;

use super::models_loop::loop_scenario;
use super::*;

/// The refusal of an assignment of the `messages` global.
const GLOBAL_REFUSAL: &str = "messages is the section's conversation and cannot be reassigned; \
                              change it with append(record) or replace(first, last, records...)";

/// The refusal of an assignment by index.
const INDEX_REFUSAL: &str = "messages cannot be assigned to by index; add records with \
                             append(record) and change them with replace(first, last, records...)";

/// The refusal of a write to a record view.
const VIEW_REFUSAL: &str = "records read from messages are read-only; change messages with \
                            replace(first, last, records...)";

/// The refusal of a loop started while another runs in the section.
const LOOP_RUNNING: &str = "a loop or a compactor is already running in this section; \
                            ask a side question with models.infer(prompt)";

/// Runs `source` over the loop models against `replies`, returning what
/// the run returned beside the gateway that served its rounds.
#[track_caller]
fn run(source: &PromptSource, replies: Vec<ScriptedReply>) -> (String, ScriptedChat) {
    let gateway = ScriptedChat::new(replies);
    let trace = loop_scenario(source.build()).chat(&gateway).run();
    (trace.text().to_owned(), gateway)
}

/// The user texts of every request the gateway received, in order.
fn user_texts(gateway: &ScriptedChat) -> Vec<Vec<String>> {
    gateway
        .requests()
        .iter()
        .map(|body| {
            body.messages
                .iter()
                .filter(|message| message.role() == Role::User)
                .map(|message| message.content().to_owned())
                .collect()
        })
        .collect()
}

#[test]
fn a_section_reads_one_list_that_starts_empty_and_lua_reaches_no_other() {
    let source = PromptSource::new().prose(
        "Only",
        "```lua
         kept = messages
         first_len = #messages
         messages:user('one')
         ```

         ```lua
         local ok, err = pcall(function() return messages['new']() end)
         local own = tostring(err):find('attempt to call a nil value', 1, true) ~= nil
         return table.concat({
           tostring(kept == messages), first_len, #messages, tostring(messages['new']),
           tostring(ok), tostring(own),
         }, '|')
         ```",
    );
    let (out, gateway) = run(&source, vec![text("unreachable")]);
    assert_eq!(gateway.call_count(), 0, "no round runs");
    assert_eq!(out, "true|0|1|nil|false|true");
}

#[test]
fn a_jump_target_a_task_a_fanout_arm_and_a_called_section_each_start_empty() {
    let entry = "local entry = #messages\n\
                 messages:user('{name}')\n\
                 return '{name}:' .. entry .. ':' .. #messages";
    let named = |name: &str| entry.replace("{name}", name);
    let source = PromptSource::new()
        .section(
            "Main",
            "messages:user('main')
             local called = call('## Called')
             local _, _, task = tasks.join_any({ tasks.spawn('## Task') })
             local arms = tasks.fanout('### Arm', { 'a', 'b' })
             var.seen = table.concat({
               called, task, arms[1].text, arms[2].text, #messages .. ':' .. messages[1].content,
             }, '|')
             jump('## Next')",
        )
        .section(
            "### Arm",
            "local entry = #messages
             messages:user(item)
             return item .. ':' .. entry .. ':' .. #messages",
        )
        .section("Next", "return var.seen .. '|next:' .. #messages")
        .section("Called", &named("called"))
        .section("Task", &named("task"));
    let (out, gateway) = run(&source, vec![text("unreachable")]);
    assert_eq!(gateway.call_count(), 0, "no round runs");
    assert_eq!(out, "called:0:1|task:0:1|a:0:1|b:0:1|1:main|next:0");
}

#[test]
fn a_called_sections_loop_sends_only_its_own_list_and_leaves_the_callers() {
    let source = PromptSource::new()
        .section(
            "Main",
            "messages:user('caller')
             local called = call('## Called')
             models.loop()
             return called .. '|' .. #messages .. '|' .. messages[#messages].content",
        )
        .section(
            "Called",
            "messages:user('callee')
             models.loop()
             return messages[#messages].content",
        );
    let (out, gateway) = run(&source, vec![text("callee reply"), text("caller reply")]);
    assert_eq!(out, "callee reply|2|caller reply");
    assert_eq!(
        user_texts(&gateway),
        [vec!["callee".to_owned()], vec!["caller".to_owned()]],
        "each round sends only its own section's list"
    );
}

#[test]
fn the_first_section_starts_empty_after_h1_ran_a_loop() {
    let source = PromptSource::new()
        .h1("messages:user('h1 question')
             models.loop()
             var.h1 = #messages")
        .section("Only", "return var.h1 .. '|' .. #messages");
    let (out, gateway) = run(&source, vec![text("h1 reply")]);
    assert_eq!(out, "2|0");
    assert_eq!(gateway.call_count(), 1, "only the H1 loop sent a round");
}

#[test]
fn a_required_library_reads_and_extends_the_requiring_sections_list() {
    let source = PromptSource::new()
        .section(
            "Main",
            "messages:user('main')
             local seen = require '## lib'
             return seen .. '|' .. #messages .. '|' .. messages[2].content",
        )
        .section(
            "lib",
            "local seen = #messages .. ':' .. messages[1].content
             messages:user('lib')
             return seen",
        );
    let (out, gateway) = run(&source, vec![text("unreachable")]);
    assert_eq!(gateway.call_count(), 0, "no round runs");
    assert_eq!(out, "1:main|2|lib");
}

#[test]
fn assigning_the_global_by_index_or_through_a_view_raises_its_own_refusal() {
    let source = PromptSource::new().section(
        "Only",
        "messages:user('kept')
         local out = {}
         for _, write in ipairs({
           function() messages = {} end,
           function() messages[1] = { role = 'user', content = 'x' } end,
           function() messages[2] = { role = 'user', content = 'x' } end,
           function() messages[1].content = 'x' end,
         }) do
           local ok, err = pcall(write)
           out[#out + 1] = tostring(ok) .. ':' .. err.kind .. ':' .. tostring(err)
         end
         out[#out + 1] = #messages .. ':' .. messages[1].content
         return table.concat(out, '\\n')",
    );
    let (out, gateway) = run(&source, vec![text("unreachable")]);
    assert_eq!(gateway.call_count(), 0, "no round runs");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 5, "{out}");
    let refusals = [GLOBAL_REFUSAL, INDEX_REFUSAL, INDEX_REFUSAL, VIEW_REFUSAL];
    for (line, refusal) in lines.iter().zip(refusals) {
        assert!(
            line.starts_with("false:lua:") && line.contains(refusal),
            "expected the lua-kind refusal {refusal:?}, got {line:?}"
        );
    }
    assert_eq!(
        lines[4], "1:kept",
        "the list still reads after the refusals"
    );
}

#[test]
fn both_loop_entries_take_only_a_compactor_and_check_it_before_a_running_loop() {
    let source = PromptSource::new().section(
        "Only",
        "local other = models.get('other')
         local refusals = {}
         local function refuse(call)
           local ok, err = pcall(call)
           refusals[#refusals + 1] = tostring(ok) .. ':' .. err.kind .. ':' .. tostring(err)
         end
         tools.offer_local('probe', 'Probe the loop entries', { value = 'string' }, function()
           refuse(function() return models.loop(other) end)
           refuse(function() return models.loop(nil, nil) end)
           refuse(function() return models.loop(42) end)
           refuse(function() return models.loop() end)
           refuse(function() return other.loop() end)
           refuse(function() return other:loop(nil, nil) end)
           refuse(function() return other:loop(42) end)
           refuse(function() return other:loop(compactors.fail) end)
           return 'probed'
         end)
         messages:user('probe the entries')
         models.loop()
         return table.concat(refusals, '\\n') .. '\\n' .. #messages",
    );
    let (out, gateway) = run(
        &source,
        vec![calls(&[("c1", "probe", "{\"value\":\"x\"}")]), text("done")],
    );
    let compactor = "compactor must be a function, got integer";
    let expected = [
        "models.loop takes (compactor?); call handle:loop(compactor?) to run on a model handle",
        "models.loop takes (compactor?)",
        compactor,
        LOOP_RUNNING,
        "call loop on a model handle with a colon: handle:loop(compactor?)",
        "handle:loop takes (compactor?)",
        compactor,
        LOOP_RUNNING,
    ]
    .map(|message| format!("false:lua:{message}"));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[..8], expected);
    assert_eq!(
        lines[8], "4",
        "the user message, the call, its result, and the reply"
    );
    assert_eq!(gateway.call_count(), 2, "no refused entry sends a round");
}
