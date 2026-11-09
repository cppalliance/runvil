//! Session lifecycle over `/agents/ws`: session isolation, status-bus
//! ordering with the backoff reset, a close ending a waiting session, a
//! program that returns framing its run's end and refusing a later
//! message, a terminal agent failure and a parse failure each surfacing
//! as an error frame, and the run log under the state directory.

use workshop_run_log::{RunId, RunLog};

use super::*;

#[tokio::test]
async fn two_sessions_do_not_cross_talk() {
    let (base, _dir, _state) = spawn_agent_server().await;
    let mut first = connect(&base).await;
    let mut second = connect(&base).await;
    let first_id = launch_echo(&mut first).await;
    let second_id = launch_echo(&mut second).await;
    assert_ne!(first_id, second_id, "every launch is its own session");

    next_wait(&mut first).await;
    next_wait(&mut second).await;
    answer(&mut first, "alpha").await;
    answer(&mut second, "beta").await;
    let first_turn = collect_turn(&mut first).await;
    let second_turn = collect_turn(&mut second).await;

    assert_eq!(delta_text(&first_turn), "echo:alpha");
    assert_eq!(delta_text(&second_turn), "echo:beta");
    for turn in [&first_turn, &second_turn] {
        let indices: Vec<u64> = turn
            .events
            .iter()
            .filter_map(|event| event["index"].as_u64())
            .collect();
        assert_eq!(
            indices,
            [1, 2, 3],
            "each session's log is its own: after its own wait at 0, no foreign entries \
             shift the indices"
        );
    }
    assert_eq!(
        first_turn.events[0]["event"]["content"], "alpha",
        "the first session's history holds only its own input"
    );
    assert_eq!(
        second_turn.events[0]["event"]["content"], "beta",
        "the second session's history holds only its own input"
    );
    first.close().await;
    second.close().await;
}

#[tokio::test]
async fn status_frames_fire_in_order_and_a_completed_reply_resets_the_backoff() {
    let (base, _dir, state) = spawn_agent_server().await;
    // The backoff stands escalated, as after an outage; the completed
    // reply is the useful work that returns it to base.
    let _ = state.backoff().next_delay();
    let _ = state.backoff().next_delay();
    assert!(state.backoff().is_escalated_for_test());

    // Status updates arrive on the main `/ws` socket as unsolicited frames.
    let mut status = JsonSocket::connect(&format!("{base}/ws")).await;
    let mut socket = connect(&base).await;
    let _session = launch_echo(&mut socket).await;
    next_wait(&mut socket).await;
    answer(&mut socket, "ping").await;
    let turn = collect_turn(&mut socket).await;
    assert_eq!(delta_text(&turn), "echo:ping");

    // Thinking on turn dispatch, Generating on the first answer delta,
    // idle at the agent's next wait on its inbox - scanned in order
    // through the status stream.
    let deadline = Duration::from_secs(10);
    let thinking = status
        .recv_until(deadline, |frame| {
            frame["type"] == "status" && frame["activity"] == "thinking"
        })
        .await;
    assert_eq!(thinking["label"], "Running agent turn");
    let generating = status
        .recv_until(deadline, |frame| {
            frame["type"] == "status" && frame["activity"] == "generating"
        })
        .await;
    assert_eq!(generating["label"], "Streaming response...");
    let idle = status
        .recv_until(deadline, |frame| {
            frame["type"] == "status" && frame["label"] == "Ready"
        })
        .await;
    assert_eq!(idle["activity"], "general");
    assert!(
        !state.backoff().is_escalated_for_test(),
        "a completed reply records useful work and resets the backoff"
    );
    socket.close().await;
    status.close().await;
}

#[tokio::test]
async fn a_close_ends_a_waiting_session_and_cancels_its_run() {
    let (base, _dir, state) = spawn_agent_server().await;
    let mut socket = connect(&base).await;
    let session = launch_echo(&mut socket).await;
    next_wait(&mut socket).await;
    assert!(
        state.agents().run_id(&session).is_some(),
        "the session waiting on its inbox holds its begun run"
    );

    assert!(state.agents().close(&session), "the session closes");
    assert!(
        state.agents().run_id(&session).is_none(),
        "a closed session leaves the registry at once"
    );
    let canceled = socket
        .recv_until(Duration::from_secs(10), |frame| frame["type"] == "error")
        .await;
    assert!(
        canceled["message"]
            .as_str()
            .is_some_and(|message| message.contains("canceled")),
        "the client learns the waiting run was canceled: {canceled}"
    );
    assert!(
        !state.agents().close(&session),
        "closing an already-closed session is a no-op"
    );
    socket.close().await;
}

#[tokio::test]
async fn a_program_that_returns_frames_run_ended_and_a_later_message_is_refused() {
    let (base, _dir, _state) = spawn_agent_server().await;
    let mut socket = connect(&base).await;
    let _session = launch_echo(&mut socket).await;
    next_wait(&mut socket).await;

    answer(&mut socket, "quit").await;
    let ended = socket
        .recv_until(Duration::from_secs(10), |frame| {
            assert_ne!(
                frame["type"], "error",
                "a program that returns ends its run without an error: {frame}"
            );
            frame["type"] == "agent_event" && frame["event"]["kind"] == "run_ended"
        })
        .await;
    assert_eq!(
        ended,
        json!({
            "type": "agent_event",
            "index": 2,
            "event": { "kind": "run_ended", "section": "Echo", "turn": 0, "content": "" },
        }),
        "the run's end frames right after the message it returned on, under the prompt's title"
    );

    answer(&mut socket, "too late").await;
    let refusal = socket
        .recv_until(Duration::from_secs(10), |frame| frame["type"] == "error")
        .await;
    assert_eq!(
        refusal["message"], "The agent's run has ended, so your message was not sent.",
        "a message after the run's end is refused out loud: {refusal}"
    );
    socket.close().await;
}

#[tokio::test]
async fn a_terminal_agent_failure_reaches_the_socket_as_an_error_frame() {
    let (base, dir, state) = spawn_agent_server().await;
    // An agent that dies after its first input, so the socket is attached
    // and subscribed long before the failure fires.
    std::fs::write(
        dir.path().join("agents").join("boom.md"),
        r"---
name: boom
description: The terminally failing test agent.
runvil: 0
uses:
  - user
---

# Boom

## Conversation

```lua
tasks.receive()
error('kaboom')
```
",
    )
    .expect("the boom agent writes");
    let mut socket = JsonSocket::connect(&format!("{base}/agents/ws")).await;
    assert_eq!(
        socket.recv_json().await,
        json!({ "type": "agents", "agents": ["boom", "chat", "echo"] }),
        "the freshly written agent is discovered on this connect"
    );
    socket
        .send_json(&json!({ "type": "launch", "agent": "boom" }))
        .await;
    let frame = socket.recv_json().await;
    assert_eq!(frame["type"], "agent_session");
    let session = frame["session"]
        .as_str()
        .expect("the acknowledgment includes the session id")
        .to_owned();

    next_wait(&mut socket).await;
    answer(&mut socket, "go").await;
    let error = socket
        .recv_until(Duration::from_secs(10), |frame| frame["type"] == "error")
        .await;
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("kaboom")),
        "the run's own failure reaches the SPA as an error frame, not just \
         the status bus: {error}"
    );
    // The failed run ends the session; the registry lets it go.
    tokio::time::timeout(Duration::from_secs(10), async {
        while state.agents().run_id(&session).is_some() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("a failed run leaves the registry");
    socket.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_that_fails_to_parse_reaches_the_socket_as_an_error_frame() {
    let (base, dir, _state) = spawn_agent_server().await;
    // Readable, so the launch is accepted, but with no frontmatter, so
    // the run fails at once, before the socket has attached.
    std::fs::write(dir.path().join("agents").join("broken.md"), "# Broken\n")
        .expect("the broken agent writes");
    let mut socket = JsonSocket::connect(&format!("{base}/agents/ws")).await;
    assert_eq!(
        socket.recv_json().await,
        json!({ "type": "agents", "agents": ["broken", "chat", "echo"] }),
        "the freshly written agent is discovered on this connect"
    );
    socket
        .send_json(&json!({ "type": "launch", "agent": "broken" }))
        .await;
    let frame = socket.recv_json().await;
    assert_eq!(frame["type"], "agent_session", "the launch is acknowledged");

    let error = socket
        .recv_until(Duration::from_secs(10), |frame| frame["type"] == "error")
        .await;
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("frontmatter delimiter")),
        "the parse failure reaches the SPA as an error frame: {error}"
    );
    socket.close().await;
}

#[tokio::test]
async fn a_launched_run_is_recorded_in_the_run_log_under_the_state_directory() {
    let (base, dir, state) = spawn_agent_server().await;
    let log_file = dir.path().join("harness").join("runs.db");
    assert!(
        !log_file.exists(),
        "the server opens no run log until a run starts"
    );
    let mut socket = connect(&base).await;
    let session = launch_echo(&mut socket).await;
    next_wait(&mut socket).await;
    answer(&mut socket, "ping").await;
    let turn = collect_turn(&mut socket).await;
    assert_eq!(delta_text(&turn), "echo:ping");
    assert!(state.agents().close(&session), "the session closes");

    // The first run in a fresh log is run 1. Its end is the last write,
    // so the row reads closed once the run has been recorded whole.
    let run = RunId::from_raw(1);
    let (log, row) = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(log) = RunLog::open(&log_file).await
                && let Ok(row) = log.run(run).await
                && row.result.is_some()
            {
                return (log, row);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the run is recorded and closed within the deadline");
    assert_eq!(row.agent, "echo", "Workshop writes the launched agent");
    let events = log.transcript(run).await.expect("the events read back");
    assert!(
        events
            .iter()
            .any(|stored| stored.record.payload.to_string().contains("echo:ping")),
        "the recorded events hold the reply: {events:?}"
    );
    socket.close().await;
}
