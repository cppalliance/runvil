//! The render a section entry issues at its first model round: one render
//! before the first Chat effect when the offers name a catalog tool and
//! none otherwise, each Pack asked once, in first-offer order, for only the
//! sets it was offered into, the entry its tool calls name, the binding of
//! a handle round or a raw model id, the same answer fed to a fresh run
//! sending the same tools, and a dropped or broken answer ending the round
//! with nothing fixed.

use runvil_parser::test_support::{one_section, prompt};
use runvil_types::packs::PackId;
use runvil_types::tools::ToolInfo;

use super::fixtures::{echo_tool, scoped_tool};
use super::models_loop::{loop_model_name, loop_scenario};
use super::*;
use crate::execute::run::{AnswerRecord, EffectRecord, PackToolSets};
use crate::model::{ModelId, ThinkingMode, ToolSchema};
use crate::test_support::drive;
use crate::test_support::rig::Rig;
use crate::test_support::scenario::{chat_answer, text_of};

/// The one section body that offers `echo`, loops once over one user
/// message, and returns the reply.
const ECHO_LOOP: &str = "tools.offer('tools/echo')\n\
     messages:user('go')\n\
     models.loop()\n\
     return messages[#messages].content";

/// Each record's effect kind, in order.
fn kinds(issued: &[EffectRecord]) -> Vec<&'static str> {
    issued
        .iter()
        .map(|record| match record {
            EffectRecord::Chat { .. } => "chat",
            EffectRecord::Render { .. } => "render",
            EffectRecord::ToolCall { .. } => "tool_call",
            EffectRecord::Vfs { .. } => "vfs",
            EffectRecord::Timer { .. } => "timer",
        })
        .collect()
}

/// The tool id `id`.
#[track_caller]
fn tool(id: &str) -> ToolId {
    ToolId::parse(id).expect("a fixture tool id")
}

/// The Pack id `id`.
#[track_caller]
fn pack(id: &str) -> PackId {
    PackId::parse(id).expect("a fixture Pack id")
}

#[test]
fn a_section_with_catalog_offers_renders_once_before_its_first_chat_and_every_round_reuses_it() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "echo", r#"{"value":"hi"}"#)]),
        text("first"),
        text("second"),
    ]);
    let source = one_section(
        "tools.offer('tools/echo')
         messages:user('one')
         models.loop()
         messages:user('two')
         models.loop()
         return messages[#messages].content",
    );
    let rig = Rig::run(
        loop_scenario(source)
            .offer("echo", echo_tool())
            .chat(&gateway),
    );
    assert_eq!(rig.trace().text(), "second");
    assert_eq!(
        kinds(rig.issued()),
        ["render", "chat", "tool_call", "chat", "chat"]
    );
    let EffectRecord::Render { alias, offered, .. } = &rig.issued()[0] else {
        panic!("the first effect is the render: {:?}", rig.issued());
    };
    assert_eq!(alias, "writer", "the render names the round's slot");
    assert_eq!(
        *offered,
        vec![PackToolSets::new(
            pack("tools"),
            vec![tool("tools/echo")],
            Vec::new()
        )]
    );
    let bodies = gateway.requests();
    assert_eq!(bodies[0].tool_names(), ["echo"]);
    let bytes: Vec<String> = bodies
        .iter()
        .map(|body| serde_json::to_string(&body.tools).expect("tool schemas serialize"))
        .collect();
    assert!(
        bytes.iter().all(|sent| *sent == bytes[0]),
        "every round sends the rendered tools byte for byte: {bytes:?}"
    );
}

#[test]
fn a_section_whose_offers_name_no_catalog_tool_goes_straight_to_its_chat() {
    let adhoc = "tools.offer(tools.adhoc({ name = 'grab', description = 'Local grab' }, function() \
                 return '' end))\n";
    for (offers, advertised) in [(adhoc, vec!["grab"]), ("", Vec::new())] {
        let gateway = ScriptedChat::new(vec![text("done")]);
        let source = one_section(&format!(
            "{offers}messages:user('go')\nmodels.loop()\nreturn messages[#messages].content"
        ));
        let rig = Rig::run(
            loop_scenario(source)
                .offer("echo", echo_tool())
                .chat(&gateway),
        );
        assert_eq!(rig.trace().text(), "done", "{offers:?}");
        assert_eq!(kinds(rig.issued()), ["chat"], "{offers:?}");
        assert_eq!(gateway.requests()[0].tool_names(), advertised, "{offers:?}");
    }
}

#[test]
fn each_offered_pack_is_asked_once_in_first_offer_order_for_only_the_sets_it_was_offered_into() {
    let gateway = ScriptedChat::new(vec![text("done")]);
    let source = one_section(&format!(
        "tools.offer_dynamic('kit/find')\ntools.offer_dynamic('tools/lookup')\n{ECHO_LOOP}"
    ));
    let find = FnTool::new("kit/find", json!({ "type": "object" }), |_| {
        Ok(ToolOutput::trusted("found"))
    })
    .description("Find a thing.");
    let rig = Rig::run(
        loop_scenario(source)
            .offer("echo", echo_tool())
            .tool(scoped_tool("lookup", "Look up a value.").0)
            .tool(find)
            .chat(&gateway),
    );
    assert_eq!(rig.trace().text(), "done");
    let EffectRecord::Render { offered, .. } = &rig.issued()[0] else {
        panic!("the first effect is the render: {:?}", rig.issued());
    };
    assert_eq!(
        *offered,
        vec![
            PackToolSets::new(pack("kit"), Vec::new(), vec![tool("kit/find")]),
            PackToolSets::new(
                pack("tools"),
                vec![tool("tools/echo")],
                vec![tool("tools/lookup")]
            ),
        ],
        "each Pack comes where its first tool was offered, into either set, and a Pack offered \
         into one set has the other empty"
    );
}

#[test]
fn each_render_names_the_entry_its_sections_tool_calls_name() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "echo", r#"{"value":"one"}"#)]),
        text("first"),
        calls(&[("call_2", "echo", r#"{"value":"two"}"#)]),
        text("second"),
    ]);
    let calling = "tools.offer('tools/echo')\n\
                   messages:user('go')\n\
                   models.loop()\n\
                   tools.call('tools/echo', { value = 'script' })";
    let source = prompt(
        "",
        &[
            ("First", calling),
            (
                "Second",
                &format!("{calling}\nreturn messages[#messages].content"),
            ),
        ],
    );
    let rig = Rig::run(
        loop_scenario(source)
            .offer("echo", echo_tool())
            .chat(&gateway),
    );
    assert_eq!(rig.trace().text(), "second");
    let mut entries: Vec<(String, Vec<Option<String>>)> = Vec::new();
    for record in rig.issued() {
        match record {
            EffectRecord::Render { entry, .. } => entries.push((entry.clone(), Vec::new())),
            EffectRecord::ToolCall { origin, .. } => entries
                .last_mut()
                .expect("a render precedes each section's tool calls")
                .1
                .push(origin.entry.clone()),
            _ => {}
        }
    }
    assert_eq!(
        entries.len(),
        2,
        "each section entry renders once: {entries:?}"
    );
    assert_ne!(entries[0].0, entries[1].0, "two entries, two ids");
    for (entry, origins) in &entries {
        assert_eq!(
            *origins,
            vec![Some(entry.clone()); 2],
            "the model's call and the script's name the render's entry"
        );
    }
}

#[test]
fn a_handle_round_renders_and_then_chats_on_the_handles_binding() {
    let gateway = ScriptedChat::new(vec![text("done")]);
    let source = one_section(
        "tools.offer('tools/echo')
         messages:user('go')
         models.get('other'):loop()
         return messages[#messages].content",
    );
    let rig = Rig::run(
        loop_scenario(source)
            .offer("echo", echo_tool())
            .chat(&gateway),
    );
    assert_eq!(rig.trace().text(), "done");
    let slots: Vec<(&str, &str)> = rig
        .issued()
        .iter()
        .map(|record| match record {
            EffectRecord::Render { alias, .. } => ("render", alias.as_str()),
            EffectRecord::Chat { alias, .. } => ("chat", alias.as_str()),
            other => panic!("only the render and the round are issued: {other:?}"),
        })
        .collect();
    assert_eq!(
        slots,
        [("render", "other"), ("chat", "other")],
        "the round keeps the handle's binding, never the section's model"
    );
    assert_eq!(
        gateway.requests()[0].options.model(),
        loop_model_name("other")
    );
}

#[test]
fn a_raw_gateway_model_id_renders_with_a_model_info_built_from_its_binding() {
    let gateway = ScriptedChat::new(vec![text("done")]);
    let source = one_section(
        "tools.offer('tools/echo')
         messages:user('go')
         models.get('raw-model'):loop()
         return messages[#messages].content",
    );
    let mut driving = loop_scenario(source)
        .context(|ctx| ctx.ui(json!({})))
        .offer("echo", echo_tool())
        .chat(&gateway)
        .start();
    let mut infos = Vec::new();
    while driving.step_with(|effect| {
        if let Effect::Render { binding, .. } = effect {
            infos.push(binding.render_info());
        }
    }) {}
    let (trace, _) = driving.end();
    assert_eq!(trace.text(), "done");
    let raw = ModelInfo::new(
        ModelId::gateway("raw-model").expect("a fixture model id"),
        "raw-model",
        NonZeroU32::new(8192).expect("the raw id window is non-zero"),
        ThinkingMode::Never,
    );
    assert_eq!(infos, [raw]);
}

/// Runs the echo section, answering its render through `render` and its
/// round with `done`, and returns each effect's record beside its
/// answer's, and the tool schemas the round sent.
fn rendered_run(
    render: impl Fn(&[PackToolSets<ToolInfo>]) -> Vec<PackToolSets<ToolInfo>>,
) -> (Vec<(EffectRecord, AnswerRecord)>, Vec<ToolSchema>) {
    let run = loop_scenario(one_section(ECHO_LOOP))
        .offer("echo", echo_tool())
        .into_run();
    let mut log = Vec::new();
    let mut sent = Vec::new();
    let (result, _) = drive(run, |_, effect| {
        let answer = match effect {
            Effect::Render { offered, .. } => EffectAnswer::Render(render(offered)),
            Effect::Chat { tools, .. } => {
                sent.clone_from(tools);
                chat_answer(&text("done"))
            }
            other => panic!("only the render and the round are issued: {other:?}"),
        };
        log.push((effect.record(), answer.record()));
        answer
    });
    assert_eq!(text_of(result), "done");
    (log, sent)
}

#[test]
fn a_recorded_render_answer_fed_to_a_fresh_run_sends_the_same_chat_tools() {
    let (first, first_tools) = rendered_run(|offered| {
        offered
            .iter()
            .map(|sets| {
                let mut shown: Vec<ToolInfo> = sets
                    .static_set
                    .iter()
                    .map(|offered| {
                        let mut renamed = offered.clone().wire_name("shout");
                        renamed.description = "Shout the value back.".to_owned();
                        renamed
                    })
                    .collect();
                shown.push(ToolInfo::new(
                    tool("tools/extra"),
                    "A tool only the model sees.",
                    json!({ "type": "object" }),
                ));
                PackToolSets::new(sets.pack.clone(), shown, sets.dynamic_set.clone())
            })
            .collect()
    });
    let names: Vec<&str> = first_tools.iter().map(ToolSchema::name).collect();
    assert_eq!(names, ["shout", "tools_extra"]);
    let answer = first
        .iter()
        .find_map(|(effect, answer)| {
            matches!(effect, EffectRecord::Render { .. }).then_some(answer)
        })
        .expect("the run renders");
    let line = serde_json::to_value(answer)
        .expect("an answer record converts")
        .to_string();
    let AnswerRecord::Render(replayed) =
        serde_json::from_str::<AnswerRecord>(&line).expect("a logged answer record reads back")
    else {
        panic!("a render's answer records as a render: {line}");
    };
    let (second, second_tools) = rendered_run(move |_| replayed.clone());
    assert_eq!(
        serde_json::to_string(&second_tools).expect("schemas serialize"),
        serde_json::to_string(&first_tools).expect("schemas serialize"),
        "the logged answer rebuilds the round's tools byte for byte"
    );
    assert_eq!(
        second, first,
        "the fresh run issues and records the same effects"
    );
}

#[test]
fn a_dropped_render_answers_its_round_as_cancelled_and_fixes_no_tools() {
    let source = one_section(
        "tools.offer('tools/echo')
         messages:user('asked')
         local ok, err = pcall(models.loop)
         models.loop()
         return tostring(ok) .. ':' .. err.kind .. '|' .. messages[#messages].content",
    );
    let run = loop_scenario(source).offer("echo", echo_tool()).into_run();
    let mut issued = Vec::new();
    let (result, _) = drive(run, |_, effect| {
        issued.push(effect.record());
        match effect {
            Effect::Render { .. } if issued.len() == 1 => EffectAnswer::Dropped,
            Effect::Render { offered, .. } => EffectAnswer::Render(offered.clone()),
            Effect::Chat { .. } => chat_answer(&text("done")),
            other => panic!("only renders and the round are issued: {other:?}"),
        }
    });
    assert_eq!(text_of(result), "false:cancelled|done");
    assert_eq!(
        kinds(&issued),
        ["render", "render", "chat"],
        "the dropped render sends no round, and the next round renders again"
    );
}

#[test]
fn a_render_answer_that_breaks_the_merge_fails_its_round_and_fixes_no_tools() {
    let source = one_section(
        "tools.offer('tools/echo')
         messages:user('asked')
         local ok, err = pcall(models.loop)
         models.loop()
         return err.kind .. ':' .. tostring(err) .. '|' .. messages[#messages].content",
    );
    let run = loop_scenario(source).offer("echo", echo_tool()).into_run();
    let mut renders = 0;
    let (result, _) = drive(run, |_, effect| match effect {
        Effect::Render { offered, .. } => {
            renders += 1;
            let mut answer = offered.clone();
            if renders == 1 {
                answer[0].static_set.push(ToolInfo::new(
                    tool("kit/sneak"),
                    "Another Pack's tool.",
                    json!({ "type": "object" }),
                ));
            }
            EffectAnswer::Render(answer)
        }
        Effect::Chat { .. } => chat_answer(&text("done")),
        other => panic!("only renders and the round are issued: {other:?}"),
    });
    assert_eq!(
        text_of(result),
        "internal:the tools Pack's render returned kit/sneak of the kit Pack in the Static Set; \
         a Pack renders only tools whose id starts with its own Pack id|done"
    );
    assert_eq!(
        renders, 2,
        "the failed merge fixed nothing, so the next round renders again"
    );
}
