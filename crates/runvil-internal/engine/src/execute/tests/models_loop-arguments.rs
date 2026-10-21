//! Tests for how `models.loop` and a model handle's `loop` read their
//! arguments: every documented form runs over the section's `messages`
//! list, and each entry's argument errors are raised in order before any
//! request leaves.

use runvil_parser::test_support::one_section;

use super::*;

/// Runs each call in `calls`, a Lua list of functions that each make one
/// refused loop call, under `pcall`, and returns the section's result:
/// every error's `kind:message`, joined by `|`, then the list's size. No
/// gateway reply is scripted, so a call that sends a request fails the
/// run.
#[track_caller]
fn refusals(calls: &str) -> String {
    let gateway = ScriptedChat::new(vec![text("unreachable")]);
    let source = one_section(&format!(
        "local other = models.get('other')
         messages:user('hello')
         local out = {{}}
         for _, call in ipairs({{ {calls} }}) do
           local ok, err = pcall(call)
           assert(not ok, 'the call is refused')
           out[#out + 1] = err.kind .. ':' .. tostring(err)
         end
         return table.concat(out, '|') .. '|' .. #messages"
    ));
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(
        gateway.call_count(),
        0,
        "every refusal fires before any request leaves"
    );
    trace.text().to_owned()
}

#[test]
fn every_argument_form_runs_over_the_sections_list() {
    let gateway = ScriptedChat::new(vec![text("r1"), text("r2"), text("r3"), text("r4")]);
    let source = one_section(
        "local other = models.get('other')
         messages:user('one')
         models.loop()
         messages:user('two')
         models.loop(compactors.fail)
         messages:user('three')
         other:loop()
         messages:user('four')
         other:loop(compactors.fail)
         local replies = {}
         for i = 2, #messages, 2 do replies[#replies + 1] = messages[i].content end
         return #messages .. '|' .. table.concat(replies, ',')",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "8|r1,r2,r3,r4");
    let models: Vec<String> = gateway
        .requests()
        .iter()
        .map(|body| body.options.model().to_owned())
        .collect();
    assert_eq!(
        models,
        ["test-model", "test-model", "other-model", "other-model"],
        "only the handle's own loop runs on the handle's binding"
    );
    let sizes: Vec<usize> = gateway
        .requests()
        .iter()
        .map(|body| body.messages.len())
        .collect();
    assert_eq!(
        sizes,
        [1, 3, 5, 7],
        "each round sends the whole list the section built"
    );
}

#[test]
fn a_handle_passed_to_models_loop_is_the_pointed_error() {
    let out = refusals(
        "function() return models.loop(other) end,
         function() return models.loop(other, compactors.fail) end,",
    );
    let pointed = "lua:models.loop takes (compactor?); \
                   call handle:loop(compactor?) to run on a model handle";
    assert_eq!(out, format!("{pointed}|{pointed}|1"));
}

#[test]
fn a_handles_loop_called_without_its_receiver_is_the_colon_error() {
    // A dot call passes the compactor where the receiver belongs, and the
    // function taken off the handle and called bare has no receiver at all.
    let out = refusals(
        "function() return other.loop(compactors.fail) end,
         function() local run = other.loop; return run(compactors.fail) end,
         function() return other.loop() end,",
    );
    let colon = "lua:call loop on a model handle with a colon: handle:loop(compactor?)";
    assert_eq!(out, format!("{colon}|{colon}|{colon}|1"));
}

#[test]
fn each_loop_entry_refuses_extra_arguments_counting_trailing_nils() {
    let out = refusals(
        "function() return models.loop(compactors.fail, 1) end,
         function() return models.loop(nil, nil) end,
         function() return other:loop(compactors.fail, 1) end,
         function() return other:loop(nil, nil) end,",
    );
    let entry = "lua:models.loop takes (compactor?)";
    let method = "lua:handle:loop takes (compactor?)";
    assert_eq!(out, format!("{entry}|{entry}|{method}|{method}|1"));
}

#[test]
fn each_entry_checks_the_handle_then_the_arity_then_the_compactor() {
    // Every call but the last of each entry fails two adjacent checks, so
    // the earlier check names the error.
    let out = refusals(
        "function() return models.loop(other, 1) end,
         function() return models.loop(42, nil) end,
         function() return models.loop(42) end,
         function() return other.loop(nil, nil, nil) end,
         function() return other:loop(42, nil) end,
         function() return other:loop(42) end,",
    );
    assert_eq!(
        out,
        "lua:models.loop takes (compactor?); \
         call handle:loop(compactor?) to run on a model handle\
         |lua:models.loop takes (compactor?)\
         |lua:compactor must be a function, got integer\
         |lua:call loop on a model handle with a colon: handle:loop(compactor?)\
         |lua:handle:loop takes (compactor?)\
         |lua:compactor must be a function, got integer\
         |1"
    );
}
