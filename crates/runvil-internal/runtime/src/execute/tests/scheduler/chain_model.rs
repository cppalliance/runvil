//! A chain's current model: `models.use` lasts across the chain's sections,
//! a `call` child and a spawned task start with a copy of the parent's model
//! taken when they are dispatched, a child's switch never reaches its parent
//! or its siblings, and the root walk starts on the model the H1 pass ended
//! on.

use super::*;
use crate::execute::tests::fixtures::echo_tool;

#[test]
fn a_switch_in_one_section_is_still_in_effect_in_the_next() {
    let gateway = ScriptedChat::new(vec![text("r")]);
    let md = sections(&[
        ("## One", "models.use('strong')"),
        (
            "## Two",
            "local label = models.current().label\nreturn label .. '|' .. models.infer('go')",
        ),
    ]);
    let trace = chain_scenario(&md).chat(&gateway).run();
    assert_eq!(trace.text(), "strong|r");
    assert_eq!(gateway.requested_models(), ["model-b"]);
}

#[test]
fn a_prompt_that_never_switches_reads_default_in_every_section() {
    let md = sections(&[
        ("## One", "var.first = models.current().label"),
        (
            "## Two",
            "return var.first .. '|' .. models.current().label",
        ),
    ]);
    let trace = chain_scenario(&md).run();
    assert_eq!(trace.text(), "default|default");
}

#[test]
fn sys_model_at_the_first_tool_call_of_a_later_section_reflects_the_earlier_switch() {
    let md = sections(&[
        ("## One", "models.use('strong')"),
        (
            "## Two",
            "tools.call('tools/echo', { value = 'y' })\nreturn sys.model",
        ),
    ]);
    let trace = chain_scenario(&md).offer("echo", echo_tool()).run();
    assert_eq!(trace.text(), "model-b");
}

#[test]
fn a_call_child_and_a_spawned_task_inherit_the_parents_switch() {
    let md = sections(&[
        (
            "## Main",
            "models.use('strong')\n\
             local viaCall = call('## Child')\n\
             local t = tasks.spawn('## Child')\n\
             local _, ok, viaTask = tasks.join_any({ t })\n\
             assert(ok)\n\
             return viaCall .. '|' .. viaTask",
        ),
        ("## Child", "return models.current().label"),
    ]);
    let trace = chain_scenario(&md).run();
    assert_eq!(trace.text(), "strong|strong");
}

#[test]
fn a_childs_switch_never_reaches_its_parent_or_a_sibling() {
    let md = sections(&[
        (
            "## Main",
            "models.use('strong')\n\
             local viaCall = call('## Switcher')\n\
             local after = models.current().label\n\
             local a = tasks.spawn('## Switcher')\n\
             local b = tasks.spawn('## Reader')\n\
             local _, okA, labelA = tasks.join_any({ a })\n\
             local _, okB, labelB = tasks.join_any({ b })\n\
             assert(okA and okB)\n\
             return viaCall .. '|' .. after .. '|' .. labelA .. '|' .. labelB .. '|' .. models.current().label",
        ),
        (
            "## Switcher",
            "models.use('fast')\nreturn models.current().label",
        ),
        ("## Reader", "return models.current().label"),
    ]);
    let trace = chain_scenario(&md).run();
    assert_eq!(trace.text(), "fast|strong|fast|strong|strong");
}

#[test]
fn a_switch_in_h1_is_the_starting_model_of_the_root_walk() {
    let md = sections(&[
        ("", "models.use('strong')"),
        ("## Main", "return models.current().label"),
    ]);
    let trace = chain_scenario(&md).run();
    assert_eq!(trace.text(), "strong");
}

#[test]
fn a_switch_in_h1_reaches_the_walk_a_jump_starts() {
    let md = sections(&[
        ("", "models.use('strong')\njump('## Target')"),
        ("## Skipped", "error('the jump must skip this section')"),
        ("## Target", "return models.current().label"),
    ]);
    let trace = chain_scenario(&md).run();
    assert_eq!(trace.text(), "strong");
}

#[test]
fn a_queued_task_keeps_the_model_its_parent_had_when_it_spawned_it() {
    // `tasks.concurrency(1)` admits the first child, which parks on its model
    // round, so the second stays queued while the parent switches to `fast`.
    // The second child reads the model the parent had at the spawn.
    let gateway = ScriptedChat::new(vec![text("r1"), text("r2")]);
    let md = sections(&[
        (
            "## Main",
            "tasks.concurrency(1)\n\
             models.use('strong')\n\
             local a = tasks.spawn('## Child')\n\
             local b = tasks.spawn('## Child')\n\
             models.use('fast')\n\
             local _, okA, first = tasks.join_any({ a })\n\
             local _, okB, second = tasks.join_any({ b })\n\
             assert(okA and okB)\n\
             return first .. '|' .. second",
        ),
        (
            "## Child",
            "local label = models.current().label\nreturn label .. ':' .. models.infer('x')",
        ),
    ]);
    let trace = chain_scenario(&md).limits(ceiling(4)).chat(&gateway).run();
    assert_eq!(trace.text(), "strong:r1|strong:r2");
    assert_eq!(gateway.requested_models(), ["model-b", "model-b"]);
}

#[test]
fn all_arms_of_one_fanout_start_on_the_same_model() {
    let md = sections(&[
        (
            "## Main",
            "tasks.concurrency(1)\n\
             models.use('strong')\n\
             local r = tasks.fanout('### Arm', {'a', 'b', 'c'})\n\
             return table.concat(r, ',')",
        ),
        ("### Arm", "return models.current().label"),
    ]);
    let trace = chain_scenario(&md).limits(ceiling(4)).run();
    assert_eq!(trace.text(), "strong,strong,strong");
}
