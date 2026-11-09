//! The turn cycle over `/agents/ws`: the agent's waits on its inbox and a
//! full turn's deltas and indexed durable events, a message that steers a
//! turn while its tool call runs, reconnect replay ending on the agent's
//! wait, and a stop that keeps the conversation's run and its wait.

use axum::http::header;
use axum::response::IntoResponse;
use tokio::sync::Notify;

use crate::common::{sse_chunk, sse_line};

use super::*;

/// The steered agent: it offers the web tools, and loops reading the
/// operator's next message and running the model loop over it.
const STEER_MD: &str = r"---
name: steer
description: The steered test agent on the unified runtime.
runvil: 0
uses:
  - user
  - web
---

# Steer

## Conversation

```lua
tools.offer('web')
while true do
    messages:user(tasks.receive())
    models.loop(models.get('test-model'))
end
```
";

/// What the agent's first `awaiting_input` frames as: the log's first
/// entry, in the agent's `Conversation` section.
fn first_wait() -> serde_json::Value {
    json!({
        "type": "agent_event",
        "index": 0,
        "event": { "kind": "awaiting_input", "section": "Conversation", "turn": 0, "content": "" },
    })
}

#[tokio::test]
async fn a_full_turn_streams_deltas_and_indexed_events_sharing_the_reply_id() {
    let (base, _dir, _state) = spawn_agent_server().await;
    let mut socket = connect(&base).await;
    let _session = launch_echo(&mut socket).await;

    // Turn one, after the agent's first wait on its inbox.
    assert_eq!(
        next_wait(&mut socket).await,
        first_wait(),
        "the agent parking on its empty inbox is the log's first entry"
    );
    answer(&mut socket, "ping").await;
    let turn = collect_turn(&mut socket).await;

    assert_eq!(
        delta_text(&turn),
        "echo:ping",
        "the live text deltas assemble the reply"
    );
    assert!(
        turn.deltas
            .iter()
            .filter(|delta| delta["kind"] == "text")
            .count()
            >= 2,
        "the mock splits content, so the turn streams multiple live chunks"
    );
    assert!(
        turn.deltas.iter().all(|delta| delta["reply"] == 0),
        "every first-turn delta is stamped with superseding reply id 0: {:?}",
        turn.deltas
    );
    let kinds: Vec<&str> = turn
        .events
        .iter()
        .filter_map(|event| event["event"]["kind"].as_str())
        .collect();
    assert_eq!(
        kinds,
        ["user_message", "agent_thought", "agent_message"],
        "the durable record of one turn: the operator's message, thinking, reply - the \
         message frames as the operator's own, so no tool_call_update exists"
    );
    let indices: Vec<u64> = turn
        .events
        .iter()
        .filter_map(|event| event["index"].as_u64())
        .collect();
    assert_eq!(
        indices,
        [1, 2, 3],
        "durable frames are stamped with monotonically increasing log indices"
    );
    assert_eq!(turn.events[0]["event"]["content"], "ping");
    assert!(
        turn.events[0].get("reply").is_none(),
        "a user_message settles no deltas, so its reply id is omitted"
    );
    assert_eq!(
        turn.events[1]["reply"], 0,
        "the thinking event supersedes the reasoning deltas of its round"
    );
    assert_eq!(turn.events[2]["event"]["content"], "echo:ping");
    assert_eq!(
        turn.events[2]["reply"], 0,
        "deltas and the completed reply share the superseding event id"
    );

    // The next message works: the agent waits again, and the full turn
    // cycle repeats with the next reply id and continuing indices.
    assert_eq!(next_wait(&mut socket).await["index"], 4);
    answer(&mut socket, "pong").await;
    let turn = collect_turn(&mut socket).await;
    assert_eq!(delta_text(&turn), "echo:pong");
    assert!(
        turn.deltas.iter().all(|delta| delta["reply"] == 1),
        "the second round's deltas are stamped with the next reply id"
    );
    let indices: Vec<u64> = turn
        .events
        .iter()
        .filter_map(|event| event["index"].as_u64())
        .collect();
    assert_eq!(indices, [5, 6, 7], "indices continue across turns");
    assert_eq!(turn.events[2]["reply"], 1);
    socket.close().await;
}

/// Streams one tool-call round attributed to `model`: a call to the web
/// Pack's search, by its wire name, for `docs`.
fn search_round(model: &str) -> Response {
    let null = serde_json::Value::Null;
    let call = json!({ "tool_calls": [{
        "index": 0, "id": "call_1", "type": "function",
        "function": { "name": "web_search", "arguments": "{\"query\":\"docs\"}" },
    }] });
    let mut sse = String::new();
    for event in [
        sse_chunk(model, &json!({ "role": "assistant" }), &null),
        sse_chunk(model, &call, &null),
        sse_chunk(model, &json!({}), &json!("tool_calls")),
    ] {
        sse.push_str(&sse_line(&event));
    }
    sse.push_str("data: [DONE]\n\n");
    ([(header::CONTENT_TYPE, "text/event-stream")], sse).into_response()
}

/// The `(role, content)` of each message one captured request sent, with
/// a tool-call turn's empty content read as empty.
fn roles_and_contents(request: &serde_json::Value) -> Vec<(String, String)> {
    request["messages"]
        .as_array()
        .expect("a captured request holds its messages")
        .iter()
        .map(|message| {
            (
                message["role"].as_str().unwrap_or_default().to_owned(),
                message["content"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

/// Serves the steered agent's gateway and returns its base URL. The model
/// streams a search round for its first request and echoes each later
/// one, and `requests` captures every request; each search notifies
/// `searching`, then holds until `release` notifies.
async fn steer_gateway(
    requests: &Arc<Mutex<Vec<serde_json::Value>>>,
    searching: &Arc<Notify>,
    release: &Arc<Notify>,
) -> String {
    let (captured, started, gate) = (
        Arc::clone(requests),
        Arc::clone(searching),
        Arc::clone(release),
    );
    spawn_gateway(with_typed_catalog(
        Router::new()
            .route(
                "/v1/chat/completions",
                post(move |body: String| {
                    let captured = Arc::clone(&captured);
                    async move {
                        record_request(&captured, &body);
                        let first = captured
                            .lock()
                            .expect("the request capture lock is healthy")
                            .len()
                            == 1;
                        if first {
                            search_round("test-model")
                        } else {
                            echo_completions(body).await
                        }
                    }
                }),
            )
            .route(
                "/v1/tools/web_search",
                post(move || {
                    let (started, gate) = (Arc::clone(&started), Arc::clone(&gate));
                    async move {
                        started.notify_one();
                        gate.notified().await;
                        axum::Json(json!({ "query": "docs", "results": [] }))
                    }
                }),
            ),
    ))
    .await
}

#[tokio::test]
async fn a_message_sent_while_a_tool_call_runs_is_a_user_message_the_next_request_holds() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (searching, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let gateway = steer_gateway(&requests, &searching, &release).await;
    let (base, dir, _state) = spawn_agent_server_for_gateway(gateway).await;
    std::fs::write(dir.path().join("agents").join("steer.md"), STEER_MD)
        .expect("the steer agent writes");
    let mut socket = JsonSocket::connect(&format!("{base}/agents/ws")).await;
    assert_eq!(socket.recv_json().await["type"], "agents");
    let _session = launch(&mut socket, "steer").await;

    next_wait(&mut socket).await;
    answer(&mut socket, "find the docs").await;
    tokio::time::timeout(Duration::from_secs(10), searching.notified())
        .await
        .expect("the model's search call is running");
    answer(&mut socket, "also check the tests").await;

    let steered = socket
        .recv_until(Duration::from_secs(10), |frame| {
            assert_ne!(
                frame["type"], "error",
                "no error frame may come between: {frame}"
            );
            assert_ne!(
                frame["event"]["kind"], "tool_call_update",
                "the message is a user_message before the held search answers: {frame}"
            );
            frame["type"] == "agent_event"
                && frame["event"]["kind"] == "user_message"
                && frame["event"]["content"] == "also check the tests"
        })
        .await;
    assert!(
        steered.get("reply").is_none(),
        "the steering message is the operator's, framed as typed"
    );
    assert_eq!(
        requests
            .lock()
            .expect("the request capture lock is healthy")
            .len(),
        1,
        "the message asks for no model round while the search runs"
    );

    release.notify_one();
    let reply = socket
        .recv_until(Duration::from_secs(10), |frame| {
            assert_ne!(
                frame["type"], "error",
                "no error frame may come between: {frame}"
            );
            frame["type"] == "agent_event" && frame["event"]["kind"] == "agent_message"
        })
        .await;
    let held = "This message arrived while you were working:\n\nalso check the tests";
    assert_eq!(reply["event"]["content"], format!("echo:{held}"));
    let requests = requests
        .lock()
        .expect("the request capture lock is healthy")
        .clone();
    assert_eq!(
        requests.len(),
        2,
        "the search round, then the one that holds the message"
    );
    let sent = roles_and_contents(&requests[1]);
    let roles: Vec<&str> = sent.iter().map(|(role, _)| role.as_str()).collect();
    assert_eq!(
        roles[roles.len() - 3..],
        ["assistant", "tool", "user"],
        "the search call finished and its result went first: {sent:?}"
    );
    assert_eq!(
        sent.last().map(|(_, content)| content.as_str()),
        Some(held),
        "the next request holds the message under its arrival line"
    );
    socket.close().await;
}

#[tokio::test]
async fn reconnect_replays_the_log_and_ends_on_the_agents_wait() {
    let (base, _dir, _state) = spawn_agent_server().await;
    let mut socket = connect(&base).await;
    let session = launch_echo(&mut socket).await;
    let opening = next_wait(&mut socket).await;
    answer(&mut socket, "ping").await;
    let live = collect_turn(&mut socket).await;
    let pending = next_wait(&mut socket).await;
    // The socket dies mid-session; the session survives.
    socket.close().await;

    let mut socket = connect(&base).await;
    socket
        .send_json(&json!({ "type": "attach", "session": session }))
        .await;
    let frame = socket.recv_json().await;
    assert_eq!(
        frame["type"], "agent_session",
        "attach is acknowledged: {frame}"
    );
    let replayed = collect_turn(&mut socket).await;
    assert_eq!(
        replayed.waits,
        [opening],
        "the replay opens with the wait the first message answered"
    );
    assert_eq!(
        replayed.events, live.events,
        "reconnect replays the persisted entries byte-alike: same indices, stamps, events"
    );
    assert_eq!(
        next_wait(&mut socket).await,
        pending,
        "the replayed log ends on the agent's wait for its next message"
    );

    // The reattached session is live: the next message runs a full turn.
    answer(&mut socket, "again").await;
    let turn = collect_turn(&mut socket).await;
    assert_eq!(delta_text(&turn), "echo:again");
    socket.close().await;
}

#[tokio::test]
async fn a_cancel_while_the_agent_waits_leaves_the_wait_and_the_next_message_answers_it() {
    let (base, _dir, state) = spawn_agent_server().await;
    let mut socket = connect(&base).await;
    let session = launch_echo(&mut socket).await;
    next_wait(&mut socket).await;
    let run = state
        .agents()
        .run_id(&session)
        .expect("the conversation's run has begun");

    socket.send_json(&json!({ "type": "cancel" })).await;
    answer(&mut socket, "after cancel").await;
    let turn = collect_turn(&mut socket).await;
    assert_eq!(
        delta_text(&turn),
        "echo:after cancel",
        "the wait the stop left answers the next message with a full turn"
    );
    assert!(
        turn.waits.is_empty(),
        "the stop never woke the wait, so it did not wait again: {:?}",
        turn.waits
    );
    assert_eq!(
        state.agents().run_id(&session),
        Some(run),
        "the stop keeps the conversation's one run"
    );
    socket.close().await;
}

#[tokio::test]
async fn a_stop_keeps_the_conversations_one_run_while_transcript_indices_continue() {
    let (base, _dir, state) = spawn_agent_server().await;
    let mut socket = connect(&base).await;
    let session = launch_echo(&mut socket).await;
    next_wait(&mut socket).await;
    answer(&mut socket, "ping").await;
    let first = collect_turn(&mut socket).await;
    let run = state
        .agents()
        .run_id(&session)
        .expect("the conversation's run has begun");

    assert_eq!(next_wait(&mut socket).await["index"], 4);
    socket.send_json(&json!({ "type": "cancel" })).await;
    answer(&mut socket, "pong").await;
    let second = collect_turn(&mut socket).await;

    assert_eq!(delta_text(&second), "echo:pong");
    let indices: Vec<u64> = first
        .events
        .iter()
        .chain(&second.events)
        .filter_map(|event| event["index"].as_u64())
        .collect();
    assert_eq!(
        indices,
        [1, 2, 3, 5, 6, 7],
        "the transcript keeps numbering across the stop, around the agent's waits"
    );
    assert_eq!(
        state.agents().run_id(&session),
        Some(run),
        "the stop keeps the conversation's one run"
    );
    socket.close().await;
}
