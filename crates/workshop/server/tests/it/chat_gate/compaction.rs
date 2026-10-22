//! The compaction gate: a conversation that outgrows the model's context
//! window summarizes its older turns, and every later turn still answers.

use super::*;

/// The most messages the compacting mock admits in one request; a longer
/// one is refused as a context overflow.
const MESSAGE_LIMIT: usize = 4;

/// The opening of the chat compactor's summary request.
const SUMMARIZE: &str = "Summarize this conversation:";

/// The opening of the user record a summary replaces older turns with.
const SUMMARY: &str = "Summary of the earlier conversation: ";

/// Whether `request` is the compactor's summary request: its last message
/// opens with [`SUMMARIZE`].
fn is_summary_request(request: &serde_json::Value) -> bool {
    request["messages"]
        .as_array()
        .and_then(|messages| messages.last())
        .and_then(|message| message["content"].as_str())
        .is_some_and(|content| content.starts_with(SUMMARIZE))
}

/// Streams `reply` as an SSE completion attributed to `model`, shaped like
/// [`echo_stream`] without the echo prefix.
fn reply_stream(model: &str, reply: &str) -> Response {
    let null = serde_json::Value::Null;
    let mut sse = String::new();
    for event in [
        sse_chunk(model, &json!({ "role": "assistant" }), &null),
        sse_chunk(model, &json!({ "content": reply }), &null),
        sse_chunk(model, &json!({}), &json!("stop")),
    ] {
        sse.push_str(&sse_line(&event));
    }
    sse.push_str("data: [DONE]\n\n");
    ([(header::CONTENT_TYPE, "text/event-stream")], sse).into_response()
}

/// The compacting mock: a request carrying more than [`MESSAGE_LIMIT`]
/// messages is a 400 naming `context_length_exceeded`, a summary request
/// streams `summary`, and every other request streams
/// `echo:<last user message>`. Every request body is captured.
fn compacting_completions(captured: &CapturedRequests, body: &str) -> Response {
    let request: serde_json::Value = serde_json::from_str(body).expect("the request is JSON");
    captured
        .lock()
        .expect("the capture lock is healthy")
        .push(request.clone());
    let model = request["model"].as_str().unwrap_or("test-model").to_owned();
    let messages = request["messages"]
        .as_array()
        .expect("the request includes a messages array");
    if messages.len() > MESSAGE_LIMIT {
        let refusal = json!({ "error": {
            "message": "This request exceeds the model's maximum context length.",
            "code": "context_length_exceeded",
        } });
        return (StatusCode::BAD_REQUEST, axum::Json(refusal)).into_response();
    }
    if is_summary_request(&request) {
        return reply_stream(&model, "summary");
    }
    let last = messages
        .last()
        .and_then(|message| message["content"].as_str())
        .expect("the request includes a user message");
    echo_stream(&model, last)
}

/// Spawns the gate server over the compacting mock, with `test-model` in
/// the retained catalog and selected in the menu.
async fn spawn_compacting_server() -> GateServer {
    let captured = CapturedRequests::default();
    let mock = Arc::clone(&captured);
    let gateway_url = spawn_gateway(
        Router::new()
            .route(
                "/v1/chat/completions",
                post(move |body: String| {
                    let captured = Arc::clone(&mock);
                    async move { compacting_completions(&captured, &body) }
                }),
            )
            .route("/v1/models", typed_catalog(&["test-model"])),
    )
    .await;
    let dir = tempfile::TempDir::new().expect("tempdir");
    let config = Config {
        agents: AgentsConfig {
            path: dir.path().join("missing-agents"),
            mcp: None,
        },
        ..test_config(&gateway_url, dir.path())
    };
    let (state, ws_base) = spawn_router(&config).await;
    state
        .catalog()
        .publish(vec![json!({ "id": "test-model", "object": "model" })]);
    state
        .menu()
        .set_selected("test-model")
        .expect("the selected model is in the retained catalog");
    GateServer {
        ws_base,
        state,
        captured,
        dir,
    }
}

/// Collects frames until the `agent_message` event holding `reply`
/// arrives, refusing error frames, and returns the next wait's token when
/// it was announced on the way. Any other message, such as the summary
/// round's own reply, is skipped.
async fn collect_reply(socket: &mut JsonSocket, reply: &str) -> Option<String> {
    let mut wait = None;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frame = socket.recv_json().await;
            match frame["type"].as_str() {
                Some("agent_event")
                    if frame["event"]["kind"] == "agent_message"
                        && frame["event"]["content"] == reply =>
                {
                    break;
                }
                Some("input_required") => {
                    wait = frame["token"].as_str().map(str::to_owned);
                }
                Some("error") => panic!("no error frame may interrupt the reply: {frame}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the turn delivers the reply `{reply:.40}` within the deadline"));
    wait
}

/// GATE 13 - context overflow. Once the conversation outgrows the
/// model's context window, each overflowing turn reports its failed round,
/// summarizes the turns before the last two into one user record, and
/// answers on the retry; every later turn answers the same way.
#[tokio::test]
async fn gate_context_overflow_summarizes_older_turns_and_later_turns_answer() {
    let server = spawn_compacting_server().await;
    let mut socket = connect_chat(&server.ws_base).await;
    let _session = launch_chat(&mut socket).await;

    // The typed catalog's 200,000-token window keeps the precheck quiet,
    // so only the mock's message limit overflows: from turn 3 on, the
    // uncompacted history carries more than four messages. The long turn
    // is two-byte characters, so its capped excerpt must end on a
    // character boundary for the summary request to be valid UTF-8.
    let long = "é".repeat(50_000);
    let inputs = [long.as_str(), "second", "third", "fourth"];
    let mut token = next_wait_token(&mut socket).await;
    for (index, input) in inputs.into_iter().enumerate() {
        answer(&mut socket, &token, input).await;
        if index >= 2 {
            let error = socket
                .recv_until(Duration::from_secs(10), |frame| frame["type"] == "error")
                .await;
            assert!(
                error["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("Model turn failed")),
                "the overflowed round surfaces as a failed model turn: {error}"
            );
        }
        let wait = collect_reply(&mut socket, &format!("echo:{input}")).await;
        token = match wait {
            Some(token) => token,
            None => next_wait_token(&mut socket).await,
        };
    }

    {
        let requests = server.captured.lock().expect("the capture lock is healthy");
        assert_eq!(
            requests.len(),
            8,
            "two plain turns, then an overflow, a summary, and a retry for each of turns 3 and 4"
        );
        let summaries: Vec<usize> = (0..requests.len())
            .filter(|&index| is_summary_request(&requests[index]))
            .collect();
        assert_eq!(summaries.len(), 2, "each overflowing turn summarizes once");
        for index in summaries {
            for (_, content) in role_content_pairs(&requests[index]) {
                assert!(
                    content.len() < 4_000,
                    "each summarized record is capped, so the summary request stays small: \
                     {} characters",
                    content.len()
                );
            }
            let retry = requests
                .get(index + 1)
                .expect("the retried round follows the summary request");
            let first = role_content_pairs(retry)
                .into_iter()
                .next()
                .expect("the retried round sends messages");
            assert_eq!(first.0, "user", "the retry opens with the summary record");
            assert!(
                first.1.starts_with(SUMMARY),
                "the retry opens with the summary of the earlier turns: {}",
                first.1
            );
        }
    }
    socket.close().await;
}
