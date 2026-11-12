//! A relative path in a library's code starts at the library's section.
//!
//! Whole prompts through the scheduler. In each, the library and the
//! section using it both have a child with the same title, so a lookup
//! made from the wrong place returns the wrong text.

use super::*;

#[test]
fn a_library_function_fans_out_over_the_librarys_worker_from_every_kind_of_caller() {
    let source = PromptSource::new()
        .h1("var.h1 = tostring(require('/kit').run({ 'h' })[1])")
        .section(
            "Research",
            "
            local kit = require '/kit'
            local own = tostring(tasks.fanout('Worker', { 'r' })[1])
            local mine = table.concat(kit.run({ 'alpha', 'beta' }), ',')
            return var.h1 .. '|' .. own .. '|' .. mine .. '|' .. call('Deep')",
        )
        .section("### Worker", "return 'research worker ' .. item")
        .section(
            "### Deep",
            "return tostring(require('/kit').run({ 'deep' })[1])",
        )
        .section(
            "kit",
            "return { run = function(items) return tasks.fanout('Worker', items) end }",
        )
        .section(
            "### Worker",
            "
            return 'kit worker ' .. item .. ' in ' .. sys.section_name .. ' beside '
              .. tostring(prompt.section('../Notes'))",
        )
        .prose("### Notes", "Kit notes.");
    let trace = require_case(source.build()).run();
    let arm = |item: &str| format!("kit worker {item} in Worker beside Kit notes.");
    assert_eq!(
        trace.text(),
        [
            arm("h"),
            "research worker r".to_owned(),
            format!("{},{}", arm("alpha"), arm("beta")),
            arm("deep"),
        ]
        .join("|"),
        "from the H1 pass, a top-level section, and a nested section, the library's fanout \
         runs the library's worker; the section's own fanout runs its own; each arm runs in \
         the library's subtree"
    );
}

#[test]
fn a_library_call_reaches_the_librarys_child_at_load_and_later() {
    let source = PromptSource::new()
        .section(
            "Main",
            "
            local kit = require '/kit'
            return kit.at_load .. '|' .. kit.help() .. '|' .. call('helper')",
        )
        .section("### helper", "return 'main helper'")
        .section(
            "kit",
            "
            local at_load = call('helper')
            return { at_load = at_load, help = function() return call('helper') end }",
        )
        .section("### helper", "return 'kit helper'");
    let trace = require_case(source.build()).run();
    assert_eq!(trace.text(), "kit helper|kit helper|main helper");
}

#[test]
fn a_library_read_reaches_the_librarys_child_and_g_prompt_reaches_the_callers() {
    let source = PromptSource::new()
        .section(
            "Main",
            "
            local kit = require '/kit'
            return kit.at_load .. '|' .. kit.template() .. '|' .. kit.users() .. '|'
              .. prompt.section('Template')",
        )
        .prose("### Template", "Main template.")
        .section(
            "kit",
            "
            local at_load = prompt.section('Template')
            return {
              at_load = at_load,
              template = function() return prompt.section('Template') end,
              users = function() return _G.prompt.section('Template') end,
            }",
        )
        .prose("### Template", "Kit template.");
    let trace = require_case(source.build()).run();
    assert_eq!(
        trace.text(),
        "Kit template.|Kit template.|Main template.|Main template."
    );
}

#[test]
fn g_call_and_g_tasks_in_a_librarys_code_start_at_the_running_section() {
    let source = PromptSource::new()
        .section(
            "Main",
            "
            local kit = require '/kit'
            return kit.users_call() .. '|' .. tostring(kit.users_fanout({ 'a' })[1]) .. '|'
              .. tasks.join({ kit.users_spawn('b') })[1].result",
        )
        .section("### helper", "return 'main helper'")
        .section("### Worker", "return 'main worker ' .. item")
        .section(
            "kit",
            "
            return {
              users_call = function() return _G.call('helper') end,
              users_fanout = function(items) return _G.tasks.fanout('Worker', items) end,
              users_spawn = function(item) return _G.tasks.spawn('Worker', { item = item }) end,
            }",
        )
        .section("### helper", "return 'kit helper'")
        .section("### Worker", "return 'kit worker ' .. item");
    let trace = require_case(source.build()).run();
    assert_eq!(trace.text(), "main helper|main worker a|main worker b");
}

#[test]
fn a_miss_in_a_librarys_code_names_the_library_and_one_through_g_names_the_running_section() {
    let source = PromptSource::new()
        .section(
            "Review",
            "
            local kit = require '/kit'
            local messages = {}
            for _, read in ipairs({ kit.own, kit.users, kit.users_call }) do
              local ok, err = pcall(read)
              assert(not ok, 'each lookup misses')
              messages[#messages + 1] = tostring(err)
            end
            return table.concat(messages, '\\n')",
        )
        .section(
            "kit",
            "
            return {
              own = function() return prompt.section('Topics') end,
              users = function() return _G.prompt.section('Topics') end,
              users_call = function() return _G.call('../Topics') end,
            }",
        );
    let trace = require_case(source.build()).run();
    let text = trace.text();
    let messages: Vec<&str> = text.lines().collect();
    let [own, users, users_call] = messages.as_slice() else {
        panic!("three messages: {messages:?}");
    };
    assert!(
        own.contains("section `Topics` not found: `/kit` has no child `Topics`"),
        "the library's own read searched the library: {own}"
    );
    assert!(
        users.contains("section `Topics` not found: `/Review` has no child `Topics`"),
        "a read through _G searched the running section: {users}"
    );
    assert!(
        users_call.contains("section `../Topics` not found: `/Review` has no sibling `Topics`"),
        "a call through _G searched the running section: {users_call}"
    );
}

#[test]
fn a_library_spawn_starts_the_librarys_worker() {
    let source = PromptSource::new()
        .section(
            "Main",
            "
            local kit = require '/kit'
            local results = tasks.join({ kit.start('x'), tasks.spawn('Worker', { item = 'y' }) })
            return results[1].result .. '|' .. results[2].result",
        )
        .section("### Worker", "return 'main worker ' .. item")
        .section(
            "kit",
            "return { start = function(item) return tasks.spawn('Worker', { item = item }) end }",
        )
        .section("### Worker", "return 'kit worker ' .. item");
    let trace = require_case(source.build()).run();
    assert_eq!(trace.text(), "kit worker x|main worker y");
}

#[test]
fn each_of_two_libraries_reaches_its_own_child_when_one_requires_the_other() {
    let source = PromptSource::new()
        .section(
            "Main",
            "
            local a = require '/a'
            return a.help() .. '|' .. a.b_help()",
        )
        .section(
            "a",
            "
            local b = require '/b'
            return { help = function() return call('helper') end, b_help = b.help }",
        )
        .section("### helper", "return 'helper of a'")
        .section(
            "b",
            "return { help = function() return call('helper') end }",
        )
        .section("### helper", "return 'helper of b'");
    let trace = require_case(source.build()).run();
    assert_eq!(trace.text(), "helper of a|helper of b");
}

#[test]
fn every_arm_that_requires_a_library_reaches_the_librarys_child() {
    let source = PromptSource::new()
        .section(
            "Main",
            "return table.concat(tasks.fanout('Arm', { 'a', 'b' }), ',')",
        )
        .section("### Arm", "return require('/kit').help() .. ' ' .. item")
        .section("### helper", "return 'main helper'")
        .section(
            "kit",
            "return { help = function() return call('helper') end }",
        )
        .section("### helper", "return 'kit helper'");
    let trace = require_case(source.build()).run();
    assert_eq!(trace.text(), "kit helper a,kit helper b");
}

#[test]
fn a_tool_handler_written_in_a_library_reaches_the_librarys_child() {
    let source = PromptSource::new()
        .section(
            "Main",
            "
            local kit = require '/kit'
            local mine = tools.adhoc({ name = 'mine', description = 'Mine' }, function()
              return call('helper')
            end)
            return tools.call(kit.tool, {}) .. '|' .. tools.call(mine, {})",
        )
        .section("### helper", "return 'main helper'")
        .section(
            "kit",
            "
            return { tool = tools.adhoc({ name = 'grab', description = 'Grab' }, function()
              return call('helper')
            end) }",
        )
        .section("### helper", "return 'kit helper'");
    let trace = require_case(source.build()).bound().run();
    assert_eq!(
        trace.text(),
        "kit helper|main helper",
        "a handler keeps the environment of the code it was written in"
    );
}

#[test]
fn a_library_function_that_jumps_still_moves_the_callers_walk() {
    // `jump` moves the running section's walk, so its path names a
    // neighbor of the running section, in a library's code too.
    let source = PromptSource::new()
        .section(
            "Main",
            "
            require('/kit').go()",
        )
        .section("### next", "return 'main next'")
        .section("kit", "return { go = function() jump 'next' end }")
        .section("### next", "return 'kit next'");
    let trace = require_case(source.build()).run();
    assert_eq!(trace.text(), "main next");
}

#[test]
fn a_name_the_caller_passes_to_a_library_is_looked_up_in_the_library() {
    let source = PromptSource::new()
        .section(
            "Main",
            "
            local kit = require '/kit'
            local ok, err = pcall(kit.run, 'Mine')
            return tostring(ok) .. '|' .. tostring(err)",
        )
        .section("### Mine", "return 'mine'")
        .section(
            "kit",
            "return { run = function(name) return call(name) end }",
        )
        .section("### Worker", "return 'w'");
    let trace = require_case(source.build()).run();
    let text = trace.text();
    assert!(
        text.starts_with("false|")
            && text.contains("section `Mine` not found: `/kit` has no child `Mine`"),
        "the miss names the library as the section it searched, got {text}"
    );
}

#[test]
fn an_uncaught_miss_in_a_library_function_is_placed_on_the_librarys_line() {
    let source = PromptSource::new()
        .section("Main", "return require('/kit').run()")
        .section(
            "kit",
            "
            return { run = function()
              local found = call('Missing')
              return found
            end }",
        )
        .build();
    let line = source
        .lines()
        .position(|line| line.contains("call('Missing')"))
        .expect("the source holds the call")
        + 1;
    let trace = require_case(source).run();
    let error = trace.error();
    assert!(
        error
            .message
            .contains("section `Missing` not found: `/kit` has no child `Missing`"),
        "got {error:?}"
    );
    assert_eq!(
        error.at.as_ref().map(|at| at.start_line.get() as usize),
        Some(line),
        "the failure names the line of the call inside the library"
    );
}

#[test]
fn a_tail_called_miss_resolves_from_the_library_and_is_placed_on_the_callers_line() {
    // Where the target resolves from does not depend on the stack: the
    // library's `call` carries the library. Where the failure is placed
    // does: a tail call drops the library function's frame, so the
    // innermost author frame is the caller's.
    let source = PromptSource::new()
        .section(
            "Main",
            "
            local kit = require '/kit'
            local out = kit.run()
            return out",
        )
        .section("### Missing", "return 'the callers own'")
        .section(
            "kit",
            "
            return { run = function()
              return call('Missing')
            end }",
        )
        .build();
    let line = source
        .lines()
        .position(|line| line.contains("kit.run()"))
        .expect("the source holds the call")
        + 1;
    let trace = require_case(source).run();
    let error = trace.error();
    assert!(
        error
            .message
            .contains("section `Missing` not found: `/kit` has no child `Missing`"),
        "the lookup ran in the library, where there is no such child, got {error:?}"
    );
    assert_eq!(
        error.at.as_ref().map(|at| at.start_line.get() as usize),
        Some(line),
        "the failure names the caller's line"
    );
}
