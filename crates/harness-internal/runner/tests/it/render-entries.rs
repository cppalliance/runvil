//! Each section entry's render: once per entry, with the entry and its
//! first round's model, and logged; dropped when it panics; and the
//! model's calls of both rendered tools, each origin naming the entry and
//! the round's model.

use super::*;

#[tokio::test]
async fn each_section_entry_renders_once_with_its_entry_and_its_first_rounds_model() {
    let seen = Seen::new();
    let broker = calling(vec![(
        "echo",
        vec![call("c1", "shout_m", json!({ "value": "a" }))],
    )]);
    let source = prompt(&[
        (
            "First",
            "tools.offer('render/echo')\nmessages:user('echo')\nmodels.loop()\nvar.first = sys.id",
        ),
        (
            "Second",
            "tools.offer('render/echo')\nmessages:user('reply')\nmodels.loop()\n\
             return var.first .. ' ' .. sys.id",
        ),
    ]);
    let (text, records) = run(&source, broker, &seen, Value::Null).await;
    let (first, second) = text.split_once(' ').expect("the run returns both entries");
    assert_ne!(first, second, "two entries, two ids");
    let rendered = |entry: &str| Rendered {
        entry: entry.to_owned(),
        model: scripted_model(),
        offered: vec![ECHO.to_owned()],
    };
    assert_eq!(
        seen.renders(),
        [rendered(first), rendered(second)],
        "each entry renders once, the first entry's two rounds included, with its entry and \
         the catalog's info for its round's model"
    );
    let logged: Vec<Value> = answered(&records, "Render")
        .into_iter()
        .map(|(effect, _)| effect["entry"].clone())
        .collect();
    assert_eq!(logged, [json!(first), json!(second)]);
    let sent: Vec<Value> = answered(&records, "Chat")
        .into_iter()
        .map(|(effect, _)| effect["tools"].clone())
        .collect();
    assert_eq!(
        sent,
        vec![json!(["shout_m", "secret_m"]); 3],
        "every round of both entries sends its entry's render"
    );
}

#[tokio::test]
async fn a_panicking_render_answers_dropped_and_the_log_holds_the_render_and_its_answer() {
    let seen = Seen::new();
    let source = prompt(&[(
        "Only",
        "tools.offer('render/echo')\nmessages:user('go')\n\
         local ok, err = pcall(models.loop)\n\
         return sys.id .. ' ' .. tostring(ok) .. ':' .. err.kind",
    )]);
    let (text, records) = run(
        &source,
        ScriptedBroker::replying(),
        &seen,
        json!({ "panics": true }),
    )
    .await;
    let (entry, ended) = text.split_once(' ').expect("the run returns its entry");
    assert_eq!(
        ended, "false:cancelled",
        "the dropped render ends its round cancelled, and the pcall catches it"
    );
    assert_eq!(
        answered(&records, "Render"),
        [(
            json!({
                "alias": "writer",
                "entry": entry,
                "offered": [{ "pack": "render", "static_set": [ECHO], "dynamic_set": [] }],
            }),
            json!("Dropped"),
        )]
    );
    assert!(
        answered(&records, "Chat").is_empty(),
        "the dropped render sends no round"
    );
}

#[tokio::test]
async fn the_model_calls_both_rendered_tools_and_each_origin_names_the_entry_and_the_model() {
    let seen = Seen::new();
    let broker = calling(vec![(
        "call both",
        vec![
            call("c1", "shout_m", json!({ "value": "a" })),
            call("c2", "secret_m", json!({ "value": "b" })),
        ],
    )]);
    let source = prompt(&[(
        "Only",
        "tools.offer('render/echo')\nmessages:user('call both')\nmodels.loop()\nreturn sys.id",
    )]);
    let (entry, records) = run(&source, broker, &seen, Value::Null).await;
    assert_eq!(
        seen.calls(),
        [
            Called::new(ECHO, &entry, MODEL),
            Called::new(SECRET, &entry, MODEL)
        ],
        "the renamed tool and the model-only tool each reach the Pack under their own id, \
         their origins naming the entry and the round's model"
    );
    let issued: Vec<(Value, Value, String)> = answered(&records, "ToolCall")
        .into_iter()
        .map(|(effect, answer)| {
            (
                effect["tool"].clone(),
                effect["alias"].clone(),
                outcome_of(&answer),
            )
        })
        .collect();
    assert_eq!(
        issued,
        [
            (
                json!(ECHO),
                json!("shout_m"),
                "render/echo a on m".to_owned()
            ),
            (
                json!(SECRET),
                json!("secret_m"),
                "render/secret b on m".to_owned()
            ),
        ]
    );
    let rendered = serde_json::to_value(shown(vec![echo()], MODEL, false)).unwrap();
    let answers: Vec<Value> = answered(&records, "Render")
        .into_iter()
        .map(|(_, answer)| answer)
        .collect();
    assert_eq!(
        answers,
        [json!({ "Render": [{ "pack": "render", "static_set": rendered, "dynamic_set": [] }] })],
        "the log keeps the rendered tools whole"
    );
}
