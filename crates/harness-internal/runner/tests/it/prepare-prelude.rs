//! A Pack's prelude through preparation: the prelude its `PackInfo` names
//! reaches every section VM of the prepared run, and each tool call a
//! prelude function makes is recorded as a `ToolCall` effect attributed to
//! the calling script, its execution, and its section. Preludes run in
//! the prompt's declaration order.

use super::*;

/// The prelude the speaker contributes: one table global whose function
/// calls the speaker's echo tool under its Pack id.
const SPEAKER_PRELUDE: &str = "local pack = ...\n\
    speaker = {}\n\
    function speaker.say(value)\n\
      return tools.call(pack .. '/echo', { value = value })\n\
    end\n";

/// A prompt declaring the speaker, binding none of its tools, and calling
/// its prelude from two sections; the second also tries to replace the
/// prelude's function and reports whether the table refused.
const SPEAKS: &str = "---\nname: speaks\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/speaker\n---\n\n# Title\n\n\
    ## First\n\n```lua\nspeaker.say('one')\n```\n\n\
    ## Second\n\n```lua\n\
    local sealed = not pcall(function() speaker.say = nil end)\n\
    return speaker.say('two') .. '|' .. tostring(sealed)\n```\n";

/// The fixture Pack `speaker`: the echo tool and a prelude that calls
/// it.
const SPEAKER: PackInfo = echo_pack("tests/speaker", Some(SPEAKER_PRELUDE));

/// The logged record of the speaker's echo called with `value` from the
/// entry `entry` of `section` by the section's script.
fn script_call(value: &str, section: &str, entry: &str) -> serde_json::Value {
    json!({
        "ToolCall": {
            "tool": "speaker/echo",
            "alias": "speaker/echo",
            "args": { "value": value },
            "origin": {
                "execution": "session-1", "section": section, "caller": "script", "entry": entry
            },
        }
    })
}

#[tokio::test]
async fn a_preludes_tool_calls_are_recorded_as_script_calls_from_the_section_that_made_each() {
    let recorder = recorder();
    let prepared = prepare(SPEAKS, &"".into(), services(&recorder, building(SPEAKER)))
        .await
        .unwrap();
    let run_id = prepared.run_id;
    let outcome = drive_run(
        prepared.run,
        prepared.performers,
        recorder.clone(),
        run_id,
        CancelHandle::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        completed(outcome),
        "two|true",
        "both sections reached the prelude's function, and its table refused a write"
    );

    let effects: Vec<serde_json::Value> = recorder
        .records(run_id)
        .into_iter()
        .filter(|record| record.kind == RecordKind::Effect)
        .map(|record| record.payload)
        .collect();
    assert_eq!(
        effects,
        [
            script_call("one", "First", "0.1"),
            script_call("two", "Second", "0.2")
        ],
        "each prelude call is one recorded ToolCall effect naming its caller and section"
    );
}

#[tokio::test]
async fn preludes_run_in_the_prompts_declaration_order_not_the_registry_order() {
    let infos = ["tests/first", "tests/second"].map(|fqn| echo_pack(fqn, Some("shared = 1\n")));
    let host = HostContext::new(registry(&infos), HostServices::new());
    let declares_second_first = "---\nname: order\ndescription: d\nrunvil: 0\n\
        uses:\n  - tests/second\n  - tests/first\n---\n\n# Title\n\n## Only\n\n```lua\nreturn 'ran'\n```\n";
    let outcome = drive_over(declares_second_first, host).await;
    let RunOutcome::Failed { kind, message } = outcome else {
        panic!("the two preludes collide: {outcome:?}");
    };
    assert_eq!(kind, "Lua");
    assert!(
        message.contains(
            "Pack `first`: its prelude defines the global `shared`, \
             which Pack `second`'s prelude already defines"
        ),
        "the second-declared Pack's prelude ran second: {message}"
    );
}
