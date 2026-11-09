//! The history gates: the operator's message frames as their own and
//! reaches the model as typed, the conversation accumulates byte-exact
//! across turns, and its rounds advertise the web tools.

use super::*;

/// GATE 11 - the operator's message. The chat reads it through
/// `tasks.receive()`: the log shows each wait on its inbox as
/// `awaiting_input`, the message frames as the operator's `user_message`
/// rather than a tool result, and the model receives the text as typed.
#[tokio::test]
async fn gate_the_operators_message_frames_as_a_user_message_between_wait_frames() {
    let server = spawn_chat_server(&["test-model"]).await;
    let mut socket = connect_chat(&server.ws_base).await;
    let _session = launch_chat(&mut socket).await;

    let typed = "two  words\r\n\"quoted\" 🦀";
    let first = next_wait(&mut socket).await;
    assert_eq!(
        first["event"],
        json!({ "kind": "awaiting_input", "section": "Conversation", "turn": 0, "content": "" }),
        "the chat's first read of its empty inbox frames as the wait for the operator"
    );
    answer(&mut socket, typed).await;
    let turn = collect_turn(&mut socket).await;
    let kinds: Vec<&str> = turn
        .events
        .iter()
        .filter_map(|event| event["event"]["kind"].as_str())
        .collect();
    assert_eq!(
        kinds,
        ["user_message", "agent_thought", "agent_message"],
        "the message shows as the operator's own, never as a tool call update"
    );
    assert_eq!(
        turn.events[0]["event"],
        json!({
            "kind": "user_message", "section": "Conversation", "turn": 0, "content": typed,
        }),
        "the operator's text frames byte-exact"
    );
    assert!(
        turn.events[0].get("reply").is_none(),
        "a user_message settles no deltas, so its reply id is omitted"
    );
    {
        let requests = server.captured.lock().expect("the capture lock is healthy");
        assert_eq!(
            role_content_pairs(&requests[0]),
            vec![pair("user", typed)],
            "the model receives the operator's text as typed"
        );
    }

    // A stop while the chat waits leaves that wait: the next message
    // answers it, and the turn it starts frames like the first.
    let open = next_wait(&mut socket).await;
    assert_ne!(open, first, "each read of the empty inbox is its own wait");
    socket.send_json(&json!({ "type": "cancel" })).await;
    answer(&mut socket, "after stop").await;
    let turn = collect_turn(&mut socket).await;
    assert_eq!(
        turn.events[0]["event"]["content"], "after stop",
        "the wait the stop left answered the next message"
    );
    assert!(turn.waits.is_empty(), "the stop never woke the wait");
    socket.close().await;
}

/// GATE 1 - multi-turn history. The conversation accumulates turn over
/// turn, and what the user typed reaches the model byte-exact with no
/// untrusted envelope around it.
#[tokio::test]
async fn gate_history_accumulates_across_three_turns_byte_exact() {
    let server = spawn_chat_server(&["test-model"]).await;
    let mut socket = connect_chat(&server.ws_base).await;
    let _session = launch_chat(&mut socket).await;

    let gnarly = "line1\r\nline2 \"quoted\" {\"text\":\"decoy\"} \\slash 🦀";
    let inputs = ["first ping", gnarly, "third"];
    for input in inputs {
        next_wait(&mut socket).await;
        answer(&mut socket, input).await;
        let turn = collect_turn(&mut socket).await;
        assert_eq!(delta_text(&turn), format!("echo:{input}"));
    }

    {
        let requests = server.captured.lock().expect("the capture lock is healthy");
        assert_eq!(requests.len(), 3, "three turns are three model rounds");
        assert_eq!(
            role_content_pairs(&requests[0]),
            vec![pair("user", "first ping")],
            "the first round sends exactly the first input"
        );
        assert_eq!(
            role_content_pairs(&requests[1]),
            vec![
                pair("user", "first ping"),
                pair("assistant", "echo:first ping"),
                pair("user", gnarly),
            ],
            "the second round sends the first exchange plus the new input, \
             the gnarly user text byte-exact and envelope-free"
        );
        assert_eq!(
            role_content_pairs(&requests[2]),
            vec![
                pair("user", "first ping"),
                pair("assistant", "echo:first ping"),
                pair("user", gnarly),
                pair("assistant", &format!("echo:{gnarly}")),
                pair("user", "third"),
            ],
            "the third round sends the whole accumulated conversation"
        );
    }
    socket.close().await;
}

/// GATE 13 - file tools. The chat offers the extra Packs, so its rounds
/// advertise the `std` file tools the model reads the language guide with.
#[tokio::test]
async fn gate_a_conversation_round_advertises_the_file_tools() {
    let server = spawn_chat_server(&["test-model"]).await;
    let mut socket = connect_chat(&server.ws_base).await;
    let _session = launch_chat(&mut socket).await;

    next_wait(&mut socket).await;
    answer(&mut socket, "ping").await;
    let turn = collect_turn(&mut socket).await;
    assert_eq!(delta_text(&turn), "echo:ping");
    {
        let requests = server.captured.lock().expect("the capture lock is healthy");
        let names: Vec<&str> = requests[0]["tools"]
            .as_array()
            .expect("the conversation round advertises tools")
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        for wire in ["Read", "ListDir", "Grep"] {
            assert!(
                names.contains(&wire),
                "the conversation round advertises {wire}, got {names:?}"
            );
        }
    }
    socket.close().await;
}

/// GATE 12 - web tools. The chat's `## Conversation` section offers the
/// `web` Pack, so the rounds its model loop sends advertise the
/// Pack's fetch and search tools; an offer in the H1 would not reach
/// them.
#[tokio::test]
async fn gate_a_conversation_round_advertises_the_web_tools() {
    let server = spawn_chat_server(&["test-model"]).await;
    let mut socket = connect_chat(&server.ws_base).await;
    let _session = launch_chat(&mut socket).await;

    next_wait(&mut socket).await;
    answer(&mut socket, "ping").await;
    let turn = collect_turn(&mut socket).await;
    assert_eq!(delta_text(&turn), "echo:ping");
    {
        let requests = server.captured.lock().expect("the capture lock is healthy");
        let names: Vec<&str> = requests[0]["tools"]
            .as_array()
            .expect("the conversation round advertises tools")
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        for wire in ["web_fetch", "web_search"] {
            assert!(
                names.contains(&wire),
                "the conversation round advertises {wire}, got {names:?}"
            );
        }
    }
    socket.close().await;
}
