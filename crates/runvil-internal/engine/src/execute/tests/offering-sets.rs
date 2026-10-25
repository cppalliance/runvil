//! Tests for the frozen sets a section entry builds at its first model
//! call: each catalog tool advertised under its own wire name, a clash or
//! an invalid name failing only the sections that offer it, each Pack's
//! tools kept together at its first offer, and the validation events
//! around the merge.

use super::*;
use crate::execute::tests::context::Recorder;

/// The refusal of a section that offers both `dup/a/b` and `dup/a.b`,
/// whose default wire names are both `dup_a_b`.
const DUP_CLASH: &str = "the tools dup/a/b of the dup Pack in the Static Set and dup/a.b of the \
                         dup Pack in the Static Set share the wire name \"dup_a_b\", and a wire \
                         name names one tool across a section's Static Set and Dynamic Set; \
                         offer only one of them";

/// One section's Lua that offers `offer`, then runs one loop round whose
/// failure it catches, returning the failure's kind and text.
fn failing_round(offer: &str) -> String {
    format!(
        "tools.offer('{offer}')\n\
         messages:user('go')\n\
         local ok, err = pcall(models.loop)\n\
         assert(not ok, 'the merge fails the first model call')\n\
         return err.kind .. ':' .. tostring(err)"
    )
}

#[test]
fn two_catalog_tools_whose_default_names_clash_fail_only_the_section_that_offers_both() {
    let gateway = ScriptedChat::new(vec![text("one")]);
    let both = format!(
        "{}\n\
         .. '|' .. tostring(tools.get('dup/a/b') ~= nil) .. tostring(tools.get('dup/a.b') ~= nil)",
        failing_round("dup")
    );
    let source = prompt(
        "",
        &[
            (
                "One",
                "tools.offer('dup/a.b')\nmessages:user('one')\nmodels.loop()",
            ),
            ("Both", &both),
        ],
    );
    let rig = Rig::run(
        catalog_scenario(source)
            .tool(offered_tool("dup/a.b"))
            .tool(offered_tool("dup/a/b"))
            .chat(&gateway),
    );
    let (refusal, catalog) = rig
        .trace()
        .text()
        .split_once('|')
        .expect("the section joins its refusal and its catalog reads");
    assert_eq!(refusal, format!("internal:{DUP_CLASH}"));
    assert_eq!(catalog, "truetrue", "both tools stay in the run catalog");
    let (advertised, _) = rounds_and_calls(rig.issued());
    assert_eq!(
        advertised,
        vec![vec!["dup_a_b".to_owned()]],
        "the section that offers one tool sends it, and the clash sends no round"
    );
}

#[test]
fn a_catalog_tool_whose_default_name_is_invalid_fails_only_the_sections_that_offer_it() {
    let gateway = ScriptedChat::new(vec![text("plain")]);
    let offers = format!(
        "{} .. '|' .. tools.call('9lives/fetch', {{ value = 'x' }})",
        failing_round("9lives/fetch")
    );
    let source = prompt(
        "",
        &[
            (
                "Plain",
                "tools.offer('tools/echo')\nmessages:user('plain')\nmodels.loop()",
            ),
            ("Offers", &offers),
        ],
    );
    let rig = Rig::run(
        catalog_scenario(source)
            .tool(echo_tool())
            .tool(offered_tool("9lives/fetch"))
            .chat(&gateway),
    );
    assert_eq!(
        rig.trace().text(),
        "internal:model-facing schema build failure for tool alias \"9lives_fetch\"|\
         9lives/fetch: x",
        "the section that offers the tool fails, and a script still calls it by id"
    );
    let (advertised, _) = rounds_and_calls(rig.issued());
    assert_eq!(advertised, vec![vec!["tools_echo".to_owned()]]);
}

#[test]
fn a_catalog_tool_is_advertised_and_called_under_its_own_wire_name() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "echo", r#"{"value":"hi"}"#)]),
        text("done"),
    ]);
    let source = one_section(
        "tools.offer('tools/echo')\n\
         messages:user('go')\n\
         models.loop()\n\
         return messages[3].content",
    );
    let rig = Rig::run(
        catalog_scenario(source)
            .tool(echo_tool().wire_name("echo"))
            .chat(&gateway),
    );
    assert_eq!(rig.trace().text(), "echoed: hi");
    let (advertised, calls) = rounds_and_calls(rig.issued());
    assert_eq!(advertised, vec![vec!["echo".to_owned()]; 2]);
    assert_eq!(calls, vec![model_call("tools/echo", "echo", "hi")]);
}

#[test]
fn the_offer_hint_names_a_catalog_tool_by_its_own_wire_name_only() {
    let source = one_section(
        "local _, own = pcall(tools.offer, 'echo')\n\
         local _, default = pcall(tools.offer, 'tools_echo')\n\
         return tostring(own) .. '|' .. tostring(default)",
    );
    let rig = Rig::run(catalog_scenario(source).tool(echo_tool().wire_name("echo")));
    let (own, default) = rig
        .trace()
        .text()
        .split_once('|')
        .expect("the section joins both refusals");
    assert!(
        own.ends_with("(did you mean 'tools/echo'?)"),
        "the tool's own wire name hints its id: {own}"
    );
    assert!(
        !default.contains("did you mean"),
        "the default name of a renamed tool hints nothing: {default}"
    );
}

#[test]
fn the_scope_gate_says_a_name_exists_only_for_a_catalog_tools_own_wire_name() {
    let refused = |name: &str| {
        let gateway = ScriptedChat::new(vec![calls(&[("call_1", name, "{}")])]);
        let source = one_section("tools.offer('gh/search')\nmessages:user('go')\nmodels.loop()");
        let trace = catalog_scenario(source)
            .tool(echo_tool().wire_name("echo"))
            .tool(offered_tool("gh/search"))
            .chat(&gateway)
            .run();
        match trace.error().inner() {
            Error::OutOfScopeToolCall {
                global_exists,
                in_scope,
                ..
            } => {
                assert_eq!(in_scope, &["gh_search".to_owned()]);
                *global_exists
            }
            other => panic!("expected OutOfScopeToolCall, got {other:?}"),
        }
    };
    assert!(refused("echo"), "the unoffered tool's own wire name exists");
    assert!(
        !refused("tools_echo"),
        "the default name of a renamed tool names nothing"
    );
}

#[test]
fn the_scope_validation_events_bracket_the_merge_whether_it_holds_or_fails() {
    let gateway = ScriptedChat::new(vec![text("fine")]);
    let source = prompt(
        "",
        &[
            (
                "Fine",
                "tools.offer('dup/a.b')\nmessages:user('fine')\nmodels.loop()",
            ),
            ("Clash", &failing_round("dup")),
        ],
    );
    let trace = catalog_scenario(source)
        .tool(offered_tool("dup/a.b"))
        .tool(offered_tool("dup/a/b"))
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), format!("internal:{DUP_CLASH}"));
    let recorder = Recorder::default();
    trace.replay(&recorder);
    let validation = [
        detail::TOOL_SCOPE_VALIDATION_STARTED,
        detail::TOOL_SCOPE_VALIDATION_SUCCEEDED,
        detail::TOOL_SCOPE_VALIDATION_FAILED,
    ]
    .map(|observation| observation.to_string());
    let events: Vec<(String, String)> = recorder
        .events()
        .into_iter()
        .filter(|(_, detail)| validation.contains(detail))
        .collect();
    let event =
        |section: &str, observation: &Observation| (section.to_owned(), observation.to_string());
    assert_eq!(
        events,
        [
            event("Fine", &detail::TOOL_SCOPE_VALIDATION_STARTED),
            event("Fine", &detail::TOOL_SCOPE_VALIDATION_SUCCEEDED),
            event("Clash", &detail::TOOL_SCOPE_VALIDATION_STARTED),
            event("Clash", &detail::TOOL_SCOPE_VALIDATION_FAILED),
        ]
    );
}

#[test]
fn a_section_that_interleaves_two_packs_offers_sees_each_packs_tools_together() {
    let gateway = ScriptedChat::new(vec![text("done")]);
    let source = one_section(
        "tools.offer('kit/b')\n\
         tools.offer('web/fetch')\n\
         tools.offer('kit')\n\
         messages:user('go')\n\
         models.loop()",
    );
    let rig = Rig::run(
        catalog_scenario(source)
            .tool(offered_tool("kit/a"))
            .tool(offered_tool("kit/b"))
            .tool(offered_tool("web/fetch"))
            .chat(&gateway),
    );
    let (advertised, _) = rounds_and_calls(rig.issued());
    assert_eq!(
        advertised,
        vec![vec![
            "kit_b".to_owned(),
            "kit_a".to_owned(),
            "web_fetch".to_owned()
        ]],
        "kit's tools sit together where kit was first offered, in the order they were offered"
    );
}
