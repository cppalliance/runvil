//! Tests for the vendor a tool-call or tool-result event records: the
//! vendor the caller reported for the answering Pack, for a model call and
//! a script call to a catalog tool, and none for an adhoc call or a run
//! given no roster.

use runvil_parser::test_support::one_section;
use runvil_types::event::Event;
use runvil_types::packs::PackId;

use super::fixtures::echo_tool;
use super::models_loop::catalog_scenario;
use super::*;

/// A section that offers `tools/echo` and an adhoc `grab`, lets the model
/// call both in one batch, then calls `tools/echo` from the script.
const BOTH_CALLERS: &str = "tools.offer('tools/echo')
     tools.offer_local('grab', 'Local grab', { value = 'string' }, function(args)
       return 'grabbed ' .. args.value
     end)
     messages:user('use both tools')
     models.loop()
     return tools.call('tools/echo', { value = 'script' })";

/// The run of [`BOTH_CALLERS`] with `tools/echo` in the catalog, its
/// context edited by `roster`.
fn both_callers(roster: impl FnOnce(RunContext) -> RunContext + 'static) -> Trace {
    let gateway = ScriptedChat::new(vec![
        calls(&[
            ("call_1", "grab", r#"{"value":"x"}"#),
            ("call_2", "tools_echo", r#"{"value":"y"}"#),
        ]),
        text("done"),
    ]);
    let trace = catalog_scenario(one_section(BOTH_CALLERS))
        .tool(echo_tool())
        .context(roster)
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "echoed: script");
    trace
}

/// Each requested call's wire name and vendor, in batch order.
fn batch_vendors(events: &[Event]) -> Vec<(String, Option<String>)> {
    events
        .iter()
        .find_map(|event| match event {
            Event::AssistantToolCalls { calls, .. } => Some(
                calls
                    .iter()
                    .map(|call| (call.name.clone(), call.vendor.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the round reports its batch: {events:?}"))
}

/// Each result's call id, alias, and vendor, sorted by call id.
fn result_vendors(events: &[Event]) -> Vec<(String, String, Option<String>)> {
    let mut results: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::ToolResult {
                tool_call_id,
                alias,
                vendor,
                ..
            } => Some((tool_call_id.clone(), alias.clone(), vendor.clone())),
            _ => None,
        })
        .collect();
    results.sort();
    results
}

fn owned(name: &str, vendor: Option<&str>) -> (String, Option<String>) {
    (name.to_owned(), vendor.map(str::to_owned))
}

fn result(id: &str, alias: &str, vendor: Option<&str>) -> (String, String, Option<String>) {
    (id.to_owned(), alias.to_owned(), vendor.map(str::to_owned))
}

#[test]
fn a_catalog_call_records_the_vendor_the_caller_reported_and_an_adhoc_call_none() {
    let tools = PackId::parse("tools").expect("a static valid Pack id");
    let trace = both_callers(move |ctx| ctx.packs(vec![UsablePack::new(tools, "acme")], vec![]));
    let events = trace.events();
    assert_eq!(
        batch_vendors(events),
        [owned("grab", None), owned("tools_echo", Some("acme"))],
        "the model's catalog call names acme and its adhoc call names none"
    );
    assert_eq!(
        result_vendors(events),
        [
            result("", "tools_echo", Some("acme")),
            result("call_1", "grab", None),
            result("call_2", "tools_echo", Some("acme")),
        ],
        "the script's and the model's catalog results name acme, the adhoc result none"
    );
}

#[test]
fn a_run_given_no_roster_records_no_vendor() {
    let trace = both_callers(|ctx| ctx);
    let events = trace.events();
    assert_eq!(
        batch_vendors(events),
        [owned("grab", None), owned("tools_echo", None)]
    );
    assert_eq!(
        result_vendors(events),
        [
            result("", "tools_echo", None),
            result("call_1", "grab", None),
            result("call_2", "tools_echo", None),
        ]
    );
}
