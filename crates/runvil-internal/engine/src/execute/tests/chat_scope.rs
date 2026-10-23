//! Tests for the `Chat` arm's tool scope from a section VM: every round
//! sends the section's rendered tool list, its effective scope plus every
//! adhoc tool at the first round.

use runvil_parser::test_support::one_section;

use super::fixtures::echo_tool;
use super::models_loop::loop_scenario;
use super::*;

#[test]
fn a_round_advertises_the_section_scope_with_adhoc_tools() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "grab", "{\"value\":\"x\"}")]),
        text("done"),
    ]);
    let source = one_section(
        "tools.offer('tools/echo')
         tools.offer_local('grab', 'Local grab', { value = 'string' }, function(args)
           return 'grabbed ' .. args.value
         end)
         messages:user('use the adhoc tool')
         models.loop()
         assert(messages[2].tool_calls[1].name == 'grab', 'an adhoc tool is in the advertised scope')
         assert(messages[3].role == 'tool', 'the shim dispatched the adhoc tool')
         return 'ok'",
    );
    let trace = loop_scenario(source)
        .offer("echo", echo_tool())
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "ok");
    let body = &gateway.requests()[0];
    assert_eq!(
        body.tool_names(),
        ["echo", "grab"],
        "the bound scope then the adhoc tools: {body:?}"
    );
}
