//! Tests for how `models.loop` reads its arguments: every documented form
//! runs over the section's `messages` list, and the argument errors are
//! raised in order before any request leaves.

use runvil_prompt::test_support::one_section;

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
        "local other = models.get('strong')
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
        "local other = models.get('strong')
         messages:user('one')
         models.loop()
         messages:user('two')
         models.loop(compactors.fail)
         messages:user('three')
         models.loop(other)
         messages:user('four')
         models.loop(other, compactors.fail)
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
        "a leading handle runs on the handle's binding, and its absence on the section's model"
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
fn the_arity_error_counts_trailing_nils_in_both_shapes() {
    let out = refusals(
        "function() return models.loop(compactors.fail, 1) end,
         function() return models.loop(nil, nil) end,
         function() return models.loop(other, compactors.fail, 1) end,
         function() return models.loop(other, nil, nil) end,
         function() return models.loop(other, compactors.fail, nil) end,",
    );
    let arity = "lua:models.loop takes (compactor?) or (model, compactor?)";
    assert_eq!(out, format!("{arity}|{arity}|{arity}|{arity}|{arity}|1"));
}

#[test]
fn the_compactor_is_checked_after_the_arity() {
    // Every call but the last fails two adjacent checks, so the earlier
    // check names the error.
    let out = refusals(
        "function() return models.loop(42, nil) end,
         function() return models.loop(42) end,
         function() return models.loop(other, 42, nil) end,
         function() return models.loop(other, 42) end,",
    );
    assert_eq!(
        out,
        "lua:models.loop takes (compactor?) or (model, compactor?)\
         |lua:compactor must be a function, got integer\
         |lua:models.loop takes (compactor?) or (model, compactor?)\
         |lua:compactor must be a function, got integer\
         |1"
    );
}
