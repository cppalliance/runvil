//! The model-first `tasks.fanout(model, worker, collection)`: every arm runs
//! on the given model, the parent's own model is what it was before the call
//! (also when a spawn fails part-way), a raw-id handle works with no model
//! supplied, a first argument that is not a handle and a wrong argument count
//! are refused, and an arm's own switch stays inside that arm.

use super::*;

#[test]
fn a_three_argument_fanout_runs_every_arm_on_the_given_model() {
    let md = sections(&[
        (
            "## Main",
            "local r = tasks.fanout(models.get('fast'), '### Arm', {'a', 'b', 'c'})\n\
             return table.concat(r, ',')",
        ),
        ("### Arm", "return models.current().model_id"),
    ]);
    let trace = chain_scenario(&md).limits(ceiling(4)).run();
    assert_eq!(trace.text(), "model-c,model-c,model-c");
}

#[test]
fn the_parents_model_after_a_three_argument_fanout_is_what_it_was_before() {
    let md = sections(&[
        (
            "## Main",
            "models.use('strong')\n\
             local r = tasks.fanout(models.get('fast'), '### Arm', {'a', 'b'})\n\
             return table.concat(r, ',') .. '|' .. models.current().label",
        ),
        ("### Arm", "return models.current().label"),
    ]);
    let trace = chain_scenario(&md).limits(ceiling(4)).run();
    assert_eq!(trace.text(), "fast,fast|strong");
}

#[test]
fn a_spawn_failure_part_way_still_restores_the_parents_model() {
    // The chain at depth 8 asks a model-first fanout, whose spawn the depth
    // cap refuses. The shim restores the chain's model before it re-raises,
    // so the label read after the caught failure is the label read before it.
    let md = sections(&[
        (
            "## Alpha",
            "var.n = (var.n or 0) + 1\n\
             if var.n == 1 then models.use('strong') end\n\
             if var.n >= 9 then\n\
               local before = models.current().label\n\
               local ok, err = pcall(tasks.fanout, models.get('fast'), '### Worker', {'x'})\n\
               local cap = tostring(err):match('recursion exceeded cap of %d+')\n\
               return before .. '|' .. tostring(ok) .. '|' .. tostring(cap) .. '|' .. models.current().label\n\
             end\n\
             return call('## Beta')",
        ),
        ("### Worker", "return item"),
        ("## Beta", "var.n = var.n + 1\nreturn call('## Alpha')"),
    ]);
    let trace = chain_scenario(&md).run();
    assert_eq!(
        trace.text(),
        "strong|false|recursion exceeded cap of 8|strong"
    );
}

#[test]
fn a_raw_id_handle_fans_out_with_no_model_supplied() {
    let md = sections(&[
        (
            "## Main",
            "local r = tasks.fanout(models.get('vendor/raw-model'), '### Arm', {'a', 'b'})\n\
             return table.concat(r, ',')",
        ),
        ("### Arm", "return models.current().model_id"),
    ]);
    let trace = Scenario::new(md)
        .models(ModelSet::default())
        .limits(ceiling(4))
        .params(|ctx| ctx.with_ui(serde_json::json!({})))
        .run();
    assert_eq!(trace.text(), "vendor/raw-model,vendor/raw-model");
}

#[test]
fn a_failing_arm_still_leaves_the_parents_model_as_it_was_before_the_call() {
    // The arm raises, so the fanout re-raises the failure at its join. The
    // model is already back by then, so the label read after the caught
    // failure is the label read before the call.
    let md = sections(&[
        (
            "## Main",
            "models.use('strong')\n\
             local before = models.current().label\n\
             local ok = pcall(tasks.fanout, models.get('fast'), '### Arm', {'a'})\n\
             return before .. '|' .. tostring(ok) .. '|' .. models.current().label",
        ),
        ("### Arm", "error('boom')"),
    ]);
    let trace = chain_scenario(&md).limits(ceiling(4)).run();
    assert_eq!(trace.text(), "strong|false|strong");
}

#[test]
fn a_first_argument_that_is_not_a_model_handle_is_refused() {
    let md = sections(&[
        (
            "## Main",
            "local out = {}\n\
             for _, first in ipairs({ 'fast', 5 }) do\n\
               local ok, err = pcall(tasks.fanout, first, '### Arm', {'a'})\n\
               assert(not ok, 'a non-handle first argument must be refused')\n\
               out[#out + 1] = tostring(err)\n\
             end\n\
             local ok, err = pcall(tasks.fanout, nil, '### Arm', {'a'})\n\
             assert(not ok, 'a nil first argument must be refused')\n\
             out[#out + 1] = tostring(err)\n\
             return table.concat(out, '\\n')",
        ),
        ("### Arm", "return item"),
    ]);
    let trace = chain_scenario(&md).run();
    let text = trace.text();
    for got in ["string", "number", "nil"] {
        assert!(
            text.contains(&format!(
                "tasks.fanout: the first argument must be a model handle, got {got}"
            )),
            "the refusal names the {got} first argument: {text}"
        );
    }
}

#[test]
fn an_argument_count_other_than_two_or_three_is_refused() {
    let md = sections(&[(
        "## Main",
        "local ok0, err0 = pcall(tasks.fanout)\n\
         local ok1, err1 = pcall(tasks.fanout, '### Arm')\n\
         local ok4, err4 = pcall(tasks.fanout, models.get('fast'), '### Arm', {'a'}, 4)\n\
         assert(not ok0 and not ok1 and not ok4, 'a wrong argument count must be refused')\n\
         return tostring(err0) .. '\\n' .. tostring(err1) .. '\\n' .. tostring(err4)",
    )]);
    let trace = chain_scenario(&md).run();
    let text = trace.text();
    assert_eq!(
        text.matches("tasks.fanout takes (worker, collection) or (model, worker, collection)")
            .count(),
        3,
        "each wrong count is refused with the same text: {text}"
    );
}

#[test]
fn an_arms_own_switch_stays_inside_that_arm() {
    let md = sections(&[
        (
            "## Main",
            "local r = tasks.fanout(models.get('fast'), '### Arm', {'a', 'b'})\n\
             return table.concat(r, ',') .. '|' .. models.current().label",
        ),
        (
            "### Arm",
            "if item == 'a' then models.use('strong') end\nreturn models.current().label",
        ),
    ]);
    let trace = chain_scenario(&md).limits(ceiling(4)).run();
    assert_eq!(trace.text(), "strong,fast|default");
}

#[test]
fn models_use_default_inside_an_arm_selects_the_default_role() {
    let md = sections(&[
        (
            "## Main",
            "local r = tasks.fanout(models.get('fast'), '### Arm', {'a', 'b'})\n\
             return table.concat(r, ',')",
        ),
        (
            "### Arm",
            "models.use('default')\nreturn models.current().label",
        ),
    ]);
    let trace = chain_scenario(&md).limits(ceiling(4)).run();
    assert_eq!(trace.text(), "default,default");
}

#[test]
fn a_two_argument_fanout_gives_every_arm_the_parents_current_model() {
    let md = sections(&[
        (
            "## Main",
            "models.use('strong')\n\
             local r = tasks.fanout('### Arm', {'a', 'b'})\n\
             return table.concat(r, ',')",
        ),
        ("### Arm", "return models.current().label"),
    ]);
    let trace = chain_scenario(&md).limits(ceiling(4)).run();
    assert_eq!(trace.text(), "strong,strong");
}
