//! Tests for the frozen sets: the first loop round that builds them fixes
//! the section's tools, every later round sends the Static Set unchanged,
//! and every offer after it raises. A round refused before the merge, or
//! whose merge fails, freezes nothing, one section's sets never reach
//! another, and `models.infer` sends no tools either side of the freeze.

use runvil_parser::test_support::{one_section, prompt};

use super::fixtures::echo_tool;
use super::models_loop::{catalog_scenario, loop_models, loop_scenario};
use super::*;
use crate::model::{ModelBinding, ModelId, ModelInvocation};
use crate::test_support::scenario::test_binding;

/// The tail of every offer function's refusal after the render.
const FIXED: &str = "the section's tool list is fixed at its first models.loop round; make \
                     every offer before the first loop, or offer in another section";

/// The loop models plus `wide`, a model whose window is large enough for
/// a request the 4096-token `writer` refuses.
fn wide_models() -> ModelSet {
    let mut models = loop_models();
    models.push(ModelBinding::new(
        "wide",
        "A model with a wide window",
        ModelId::gateway("wide-model").expect("the test model id is valid"),
        ModelInvocation::new(),
        NonZeroU32::new(131_072).expect("the window is non-zero"),
    ));
    models
}

/// Each round's tool schemas as the JSON the wire carries.
fn tool_bytes(bodies: &[ScriptedCall]) -> Vec<String> {
    bodies
        .iter()
        .map(|body| serde_json::to_string(&body.tools).expect("tool schemas serialize"))
        .collect()
}

#[test]
fn a_first_round_the_precheck_refuses_still_fixes_the_tool_list() {
    let gateway = ScriptedChat::new(vec![text("done")]);
    let source = one_section(
        "tools.offer('tools/echo')
         messages:user(string.rep('x', 20000))
         local ok, err = pcall(models.loop)
         assert(not ok and err.reason == 'precheck', 'the narrow window refuses the round')
         local offered, refusal = pcall(tools.offer, tools.adhoc({ name = 'grab', description = 'Local grab' }, function()
           return ''
         end))
         assert(not offered, 'the refused round fixed the list')
         models.use('wide')
         models.loop()
         return refusal.kind .. ':' .. tostring(refusal)",
    );
    let trace = loop_scenario(source)
        .models(wide_models())
        .offer("echo", echo_tool())
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), format!("lua:tools.offer: {FIXED}"));
    let bodies = gateway.requests();
    assert_eq!(bodies.len(), 1, "only the round on the wide model left");
    assert_eq!(
        bodies[0].tool_names(),
        ["echo"],
        "the round sends the list the refused round rendered: {bodies:?}"
    );
}

#[test]
fn every_later_round_sends_the_same_tool_bytes_across_a_model_change() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "echo", r#"{"value":"one"}"#)]),
        text("first"),
        calls(&[("call_2", "grab", r#"{"value":"two"}"#)]),
        text("second"),
    ]);
    let source = one_section(
        "tools.offer('tools/echo')
         tools.offer(tools.adhoc({ name = 'grab', description = 'Local grab', parameters = { value = 'string' } }, function(args)
           return 'grabbed ' .. args.value
         end))
         messages:user('one')
         models.loop()
         messages:user('two')
         models.get('other'):loop()
         return messages[#messages].content",
    );
    let trace = loop_scenario(source)
        .offer("echo", echo_tool())
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "second");
    let bodies = gateway.requests();
    let served: Vec<&str> = bodies.iter().map(|body| body.options.model()).collect();
    assert_eq!(
        served,
        ["test-model", "test-model", "other-model", "other-model"]
    );
    assert_eq!(bodies[0].tool_names(), ["echo", "grab"]);
    let bytes = tool_bytes(&bodies);
    assert!(
        bytes.iter().all(|sent| *sent == bytes[0]),
        "every round sends the first round's tools byte for byte: {bytes:?}"
    );
}

#[test]
fn after_the_render_every_offer_raises_even_an_identical_one_while_defining_still_works() {
    let gateway = ScriptedChat::new(vec![calls(&[("call_1", "grab", "{}")]), text("done")]);
    let source = prompt(
        "uses: [tools]",
        &[
            (
                "Only",
                "tools.offer('tools/echo')
                 tools.offer('tools')
                 tools.offer(tools.adhoc({ name = 'grab', description = 'Local grab' }, function()
                   local _, err = pcall(tools.offer, 'tools/echo')
                   return err.kind .. ':' .. tostring(err)
                 end))
                 messages:user('go')
                 models.loop()
                 local out = { messages[3].content }
                 for _, offer in ipairs({
                   function() tools.offer('tools/echo') end,
                   function() tools.offer('tools') end,
                   function() tools.offer(tools.adhoc({ name = 'late', description = 'Late' }, function() return '' end)) end,
                   function() tools.offer_dynamic('tools') end,
                   function() require '## offers' end,
                 }) do
                   local _, err = pcall(offer)
                   out[#out + 1] = err.kind .. ':' .. tostring(err)
                 end
                 out[#out + 1] = tostring(tools.get('adhoc/late') ~= nil)
                 return table.concat(out, '\\n')",
            ),
            ("offers", "tools.offer('tools/echo')"),
        ],
    );
    let trace = catalog_scenario(source)
        .tool(echo_tool())
        .chat(&gateway)
        .run();
    let lines: Vec<&str> = trace.text().lines().collect();
    let offer = format!("lua:tools.offer: {FIXED}");
    assert_eq!(
        lines[..5],
        [
            offer.clone(),
            offer.clone(),
            offer.clone(),
            offer.clone(),
            format!("lua:tools.offer_dynamic: {FIXED}"),
        ],
        "a handler's offer during the loop, then an identical re-offer of a tool \
         and of a Pack, an offer of a tool defined after the render, and a \
         dynamic offer"
    );
    assert_eq!(lines.len(), 7, "{lines:?}");
    assert!(
        lines[5].contains(&format!("tools.offer: {FIXED}")),
        "an offer from a required library raises: {}",
        lines[5]
    );
    assert_eq!(lines[6], "true", "the tool defined after the render exists");
    let bodies = gateway.requests();
    assert_eq!(bodies[0].tool_names(), ["tools_echo", "grab"]);
    assert_eq!(tool_bytes(&bodies)[0], tool_bytes(&bodies)[1]);
}

#[test]
fn dynamic_offers_leave_the_rendered_schemas_byte_for_byte_unchanged() {
    let run = |dynamic: &str| {
        let gateway = ScriptedChat::new(vec![text("done")]);
        let source = one_section(&format!(
            "{dynamic}tools.offer('tools/echo')
             tools.offer(tools.adhoc({{ name = 'grab', description = 'Local grab' }}, function() return '' end))
             messages:user('go')
             models.loop()"
        ));
        loop_scenario(source)
            .offer("echo", echo_tool())
            .chat(&gateway)
            .run();
        tool_bytes(&gateway.requests())
    };
    let plain = run("");
    assert_eq!(
        run("tools.offer_dynamic('tools')\ntools.offer_dynamic(tools.packs)\n"),
        plain,
        "a dynamic offer changes nothing the model sees"
    );
}

#[test]
fn a_first_round_refused_over_an_empty_list_renders_nothing() {
    let gateway = ScriptedChat::new(vec![text("done")]);
    let source = one_section(
        "local ok, err = pcall(models.loop)
         assert(not ok, 'an empty list is refused')
         tools.offer('tools/echo')
         messages:user('go')
         models.loop()
         return tostring(err)",
    );
    let trace = loop_scenario(source)
        .offer("echo", echo_tool())
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "messages must not be empty");
    let bodies = gateway.requests();
    assert_eq!(bodies.len(), 1, "the refused round never left");
    assert_eq!(
        bodies[0].tool_names(),
        ["echo"],
        "the offer after the refused round reaches the next round"
    );
}

#[test]
fn a_first_round_refused_for_a_missing_model_renders_nothing() {
    let gateway = ScriptedChat::new(vec![text("done")]);
    let source = one_section(
        "messages:user('go')
         local ok, err = pcall(models.loop)
         assert(not ok, 'a section with no model is refused')
         tools.offer('tools/echo')
         models.get('writer'):loop()
         return tostring(err)",
    );
    let unset = ModelSet::from_parts(
        vec![test_binding(
            "writer",
            "A general model for tests",
            "test-model",
        )],
        None,
    );
    let trace = loop_scenario(source)
        .models(unset)
        .offer("echo", echo_tool())
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "model binding required for section Only");
    let bodies = gateway.requests();
    assert_eq!(bodies.len(), 1, "the refused round never left");
    assert_eq!(bodies[0].tool_names(), ["echo"]);
}

/// A tool whose parameters are not a JSON object, so the round that
/// builds a tool list holding it fails.
#[track_caller]
fn scalar_schema_tool() -> FnTool {
    FnTool::new("tools/shape", json!("scalar"), |_| {
        panic!("no round advertises the scalar-schema tool")
    })
    .description("A tool whose schema no round can advertise.")
}

#[test]
fn a_first_round_whose_tool_build_fails_renders_nothing() {
    let gateway = ScriptedChat::new(vec![text("done")]);
    let source = one_section(
        "tools.offer('tools/shape')
         messages:user('go')
         local ok, err = pcall(models.loop)
         assert(not ok, 'the scalar schema fails the build')
         tools.offer('tools/echo')
         tools.offer(tools.adhoc({ name = 'shape', description = 'Local shape' }, function() return '' end))
         models.loop()
         return err.kind .. ':' .. tostring(err)",
    );
    let trace = loop_scenario(source)
        .offer("shape", scalar_schema_tool())
        .offer("echo", echo_tool())
        .chat(&gateway)
        .run();
    assert_eq!(
        trace.text(),
        "internal:model-facing schema build failure for tool alias \"shape\""
    );
    let bodies = gateway.requests();
    assert_eq!(bodies.len(), 1, "the failed round never left");
    assert_eq!(
        bodies[0].tool_names(),
        ["echo", "shape"],
        "both offers after the failed build reach the next round, the adhoc \
         tool in place of the unusable one"
    );
}

#[test]
fn one_sections_render_never_reaches_another() {
    let gateway = ScriptedChat::new(vec![text("first"), text("second")]);
    let source = prompt(
        "",
        &[
            ("First", "messages:user('one')\nmodels.loop()"),
            (
                "Second",
                "tools.offer('tools/echo')\n\
                 messages:user('two')\n\
                 models.loop()\n\
                 return messages[1].content .. '|' .. #messages",
            ),
        ],
    );
    let trace = loop_scenario(source)
        .offer("echo", echo_tool())
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "two|2", "the second section has its own list");
    let bodies = gateway.requests();
    assert!(
        bodies[0].tools.is_empty(),
        "the first section offered nothing"
    );
    assert_eq!(
        bodies[1].tool_names(),
        ["echo"],
        "the first section's empty render does not reach the second: {bodies:?}"
    );
}

#[test]
fn models_infer_sends_no_tools_and_leaves_messages_untouched_either_side_of_the_render() {
    let gateway = ScriptedChat::new(vec![text("before"), text("looped"), text("after")]);
    let source = one_section(
        "messages:user('go')
         local before = models.infer('ask before')
         assert(#messages == 1, 'an infer before the render leaves messages alone')
         tools.offer('tools/echo')
         models.loop()
         local after = models.infer('ask after')
         assert(#messages == 2, 'an infer after the render leaves messages alone')
         return before .. '|' .. messages[2].content .. '|' .. after",
    );
    let trace = loop_scenario(source)
        .offer("echo", echo_tool())
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "before|looped|after");
    let bodies = gateway.requests();
    assert_eq!(bodies.len(), 3);
    assert!(bodies[0].tools.is_empty() && bodies[2].tools.is_empty());
    assert_eq!(
        bodies[1].tool_names(),
        ["echo"],
        "an infer renders nothing, so the later offer reaches the loop"
    );
    for (body, prompt) in [(&bodies[0], "ask before"), (&bodies[2], "ask after")] {
        assert_eq!(body.messages.len(), 1, "an infer sends only its prompt");
        assert_eq!(body.messages[0].content(), prompt);
    }
}
