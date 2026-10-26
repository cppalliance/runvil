//! The rendered tools' calls: each call's events name the vendor of its
//! tool id's Pack, a model-only tool rendered into the Dynamic Set runs
//! through `CallDynamicTool`, and two entries on two models render their
//! own names and flags, so a stop spares only the call whose rendered tool
//! survives stops.

use runvil_plugin::ModelId;

use super::*;
use crate::harness::{run_beside, until};

#[tokio::test]
async fn each_calls_events_name_the_vendor_of_its_tool_ids_pack_and_an_adhoc_call_none() {
    let seen = Seen::new();
    let broker = calling(vec![(
        "call every kind",
        vec![
            call("c1", "shout_m", json!({ "value": "a" })),
            call("c2", "secret_m", json!({ "value": "b" })),
            call("c3", "grab", json!({})),
            call("c4", "GetDynamicTools", json!({})),
        ],
    )]);
    let source = prompt(&[(
        "Only",
        "tools.offer('render/echo')\n\
         tools.offer(tools.adhoc({ name = 'grab', description = 'Grab a value' }, \
         function() return 'grabbed' end))\n\
         tools.offer_dynamic(tools.adhoc({ name = 'later', description = 'A later tool' }, \
         function() return 'later' end))\n\
         messages:user('call every kind')\nmodels.loop()\nreturn 'done'",
    )]);
    let (_, records) = run(&source, broker, &seen, Value::Null).await;
    let named = |id: &str, call: &Value| {
        (
            call[id].clone(),
            call["tool"].clone(),
            call["vendor"].clone(),
        )
    };
    let expected = [
        (json!("c1"), json!(ECHO), json!("tests")),
        (json!("c2"), json!(SECRET), json!("tests")),
        (json!("c3"), json!("adhoc/grab"), Value::Null),
        (json!("c4"), json!("dynamic/search"), json!("runvil")),
    ];
    let requested: Vec<(Value, Value, Value)> = events(&records, "assistant_tool_calls")
        .iter()
        .flat_map(|event| event["calls"].as_array().cloned().unwrap_or_default())
        .map(|call| named("id", &call))
        .collect();
    assert_eq!(
        requested, expected,
        "the renamed and the model-only call name the render Pack's vendor, the adhoc call \
         none, and the discovery call runvil"
    );
    let mut results: Vec<(Value, Value, Value)> = events(&records, "tool_result")
        .iter()
        .map(|event| named("tool_call_id", event))
        .collect();
    results.sort_by_key(|(id, _, _)| id.as_str().map(str::to_owned));
    assert_eq!(results, expected, "each result names the same vendor");
}

#[tokio::test]
async fn call_dynamic_tool_runs_a_model_only_tool_the_render_put_in_the_dynamic_set() {
    let seen = Seen::new();
    let broker = calling(vec![(
        "call it dynamically",
        vec![call(
            "c1",
            "CallDynamicTool",
            json!({ "toolName": "secret_m", "arguments": { "value": "x" } }),
        )],
    )]);
    let source = prompt(&[(
        "Only",
        "tools.offer_dynamic('render/echo')\nmessages:user('call it dynamically')\n\
         models.loop()\nreturn sys.id .. ' ' .. messages[#messages - 1].content",
    )]);
    let (text, records) = run(&source, broker, &seen, Value::Null).await;
    let (entry, result) = text.split_once(' ').expect("the run returns its entry");
    assert_eq!(
        result, "render/secret x on m",
        "the model reads the model-only tool's answer"
    );
    assert_eq!(seen.calls(), [Called::new(SECRET, entry, MODEL)]);
    let sent: Vec<Value> = answered(&records, "Chat")
        .into_iter()
        .map(|(effect, _)| effect["tools"].clone())
        .collect();
    assert_eq!(
        sent,
        vec![json!(["GetDynamicTools", "CallDynamicTool"]); 2],
        "the rendered tools sit in the Dynamic Set, behind the discovery tools"
    );
    let issued: Vec<(Value, Value)> = answered(&records, "ToolCall")
        .into_iter()
        .map(|(effect, _)| (effect["tool"].clone(), effect["alias"].clone()))
        .collect();
    assert_eq!(
        issued,
        [(json!(SECRET), json!("secret_m"))],
        "the call reaches the Harness under the model-only id and its rendered wire name"
    );
}

/// `Main` spawns one task per model and joins both: `OnM` loops on the
/// default role, bound to the Host's model `m`, and `OnN` on the raw
/// model id `n`. Each offers the render Pack's tool, asks for a held call
/// of the model-only tool, and reports how its loop ended.
const ON_TWO_MODELS: [(&str, &str); 3] = [
    (
        "Main",
        "local on_m = tasks.spawn('## OnM')\nlocal on_n = tasks.spawn('## OnN')\n\
         local ended = tasks.join({ on_m, on_n })\n\
         return ended[1].result .. '|' .. ended[2].result",
    ),
    (
        "OnM",
        "tools.offer('render/echo')\nmessages:user('hold on m')\n\
         local ok, err = pcall(models.loop)\n\
         return tostring(ok) .. ':' .. (ok and messages[#messages].content or err.kind)",
    ),
    (
        "OnN",
        "tools.offer('render/echo')\nmessages:user('hold on n')\n\
         local n = models.get('n')\nlocal ok, err = pcall(n.loop, n)\n\
         return tostring(ok) .. ':' .. (ok and messages[#messages].content or err.kind)",
    ),
];

#[tokio::test]
async fn two_models_get_their_own_renders_and_a_stop_spares_only_the_surviving_call() {
    let seen = Seen::new();
    let recorder = Arc::new(MemoryRecorder::new());
    let broker = calling(vec![
        (
            "hold on m",
            vec![call(
                "c1",
                "secret_m",
                json!({ "value": "m", "hold": true }),
            )],
        ),
        (
            "hold on n",
            vec![call(
                "c1",
                "secret_n",
                json!({ "value": "n", "hold": true }),
            )],
        ),
    ]);
    let harness = render_harness(&recorder, broker, &seen, json!({ "surviving": [MODEL] }));
    let watched = Arc::clone(&seen);
    let report = run_beside(
        harness,
        request(&prompt(&ON_TWO_MODELS)),
        |control| async move {
            until("both held calls are in flight", || {
                watched.calls().len() == 2
            })
            .await;
            control.stop_round();
            until("the stop drops a call", || !watched.dropped().is_empty()).await;
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
            assert_eq!(
                watched.dropped(),
                ["n"],
                "the stop drops only the call whose rendered tool does not survive stops"
            );
            watched.open();
        },
    )
    .await
    .expect("the run reaches an outcome");

    assert_eq!(
        report.outcome,
        RunOutcome::Completed {
            final_text: "true:done|false:cancelled".to_owned()
        },
        "the spared call answers and its loop goes on, and the dropped call's loop raises"
    );
    let mut models: Vec<ModelId> = seen
        .renders()
        .iter()
        .map(|rendered| rendered.model.id().clone())
        .collect();
    models.sort();
    assert_eq!(
        models,
        [
            ModelId::gateway("m").unwrap(),
            ModelId::gateway("n").unwrap()
        ],
        "each entry renders for its own round's model"
    );
    let records = recorder.records(report.run_id.expect("the run began"));
    let mut rendered: Vec<Value> = answered(&records, "Render")
        .into_iter()
        .map(|(_, answer)| answer["Render"][0]["static_set"].clone())
        .collect();
    rendered.sort_by_key(|set| set[0]["wire_name"].as_str().map(str::to_owned));
    let expected = |model: &str, surviving: bool| {
        serde_json::to_value(shown(vec![echo()], model, surviving)).unwrap()
    };
    assert_eq!(
        rendered,
        [expected("m", true), expected("n", false)],
        "each render names the tools for its model, and only m's marks the model-only tool \
         to survive stops"
    );
    let mut calls: Vec<(Value, String)> = answered(&records, "ToolCall")
        .into_iter()
        .map(|(effect, answer)| (effect["origin"]["model"].clone(), outcome_of(&answer)))
        .collect();
    calls.sort_by_key(|(model, _)| model.as_str().map(str::to_owned));
    assert_eq!(
        calls,
        [
            (json!("m"), "render/secret m on m".to_owned()),
            (json!("n"), "Dropped".to_owned()),
        ]
    );
}
