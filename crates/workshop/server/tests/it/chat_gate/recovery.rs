//! Recovery gates: a live chat's wait answering on a replacement Gateway,
//! a failed completion the chat survives, and a selection lost while the
//! chat waits.

use super::*;

#[tokio::test]
async fn a_live_chat_wait_answers_on_the_replacement_port_and_key() {
    let server = spawn_chat_server(&["test-model"]).await;
    let mut socket = connect_chat(&server.ws_base).await;
    let session = launch_chat(&mut socket).await;
    next_wait(&mut socket).await;

    let replacement_captured = CapturedRequests::default();
    let captured = Arc::clone(&replacement_captured);
    let replacement = spawn_gateway(Router::new().route(
        "/v1/chat/completions",
        post(move |headers: axum::http::HeaderMap, body: String| {
            let captured = Arc::clone(&captured);
            async move {
                if headers
                    .get(header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    != Some("Bearer replacement-key")
                {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                gate_completions(&captured, &body)
            }
        }),
    ))
    .await;
    replace_gateway(
        &gateway_updater(&server.state),
        &replacement,
        "replacement-key",
    )
    .expect("the replacement publishes");

    // The run the original gateway served still waits on its inbox, and
    // the next message's round goes to the replacement port under the
    // replacement key.
    answer(&mut socket, "after gateway recovery").await;
    let turn = collect_turn(&mut socket).await;
    assert_eq!(delta_text(&turn), "echo:after gateway recovery");
    assert!(
        server
            .captured
            .lock()
            .expect("the original capture lock is healthy")
            .is_empty(),
        "the old endpoint receives no post-publication completion"
    );
    assert_eq!(
        replacement_captured
            .lock()
            .expect("the replacement capture lock is healthy")
            .len(),
        1,
        "the replacement endpoint and bearer complete the next turn"
    );
    assert_eq!(
        server.state.agents().run_id(&session),
        Some(workshop_run_log::RunId::from_raw(1)),
        "the replacement keeps the conversation's one run, the first the log began"
    );
    socket.close().await;
}

/// GATE 6 - error survival. A failed completion surfaces an error to
/// the operator and the chat keeps working.
#[tokio::test]
async fn gate_model_failure_surfaces_an_error_and_the_next_input_works() {
    let server = spawn_chat_server(&["test-model"]).await;
    let mut socket = connect_chat(&server.ws_base).await;
    let _session = launch_chat(&mut socket).await;

    next_wait(&mut socket).await;
    answer(&mut socket, "fail").await;
    let error = socket
        .recv_until(Duration::from_secs(10), |frame| frame["type"] == "error")
        .await;
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("Model turn failed")),
        "the failed model call surfaces as an error frame naming the boundary: {error}"
    );

    // The pcall'd failure never kills the program: the loop returns to
    // tasks.receive() and the next turn is a normal one. The failed input
    // stays in the retained message list, so the projection joins the two
    // consecutive user utterances with a blank line.
    next_wait(&mut socket).await;
    answer(&mut socket, "recovered").await;
    let turn = collect_turn(&mut socket).await;
    assert_eq!(
        delta_text(&turn),
        "echo:fail\n\nrecovered",
        "the next input still works after the failure, with the failed input retained"
    );
    let reply = turn.events.last().expect("the recovery turn completes");
    assert_eq!(reply["event"]["content"], "echo:fail\n\nrecovered");
    socket.close().await;
}

/// GATE 7 - selection-loss recovery, unified-runtime semantics: the run's
/// model is the dropdown selection bound at launch, so a selection that
/// vanishes while the chat waits skips nothing - the frozen binding drives
/// the next turn to completion, and the same run keeps serving turns
/// until a close ends it.
#[tokio::test]
async fn gate_selection_loss_leaves_the_runs_frozen_binding_untouched() {
    let server = spawn_chat_server(&["test-model"]).await;
    let mut socket = connect_chat(&server.ws_base).await;
    let session = launch_chat(&mut socket).await;

    next_wait(&mut socket).await;
    server.state.catalog().publish(Vec::new());
    server.state.menu().reconcile_catalog();
    answer(&mut socket, "accepted before loss").await;

    // The selection is gone, but the run's binding was frozen at launch:
    // the turn after the loss dispatches and completes, and no error frame
    // surfaces.
    let turn = collect_turn(&mut socket).await;
    assert_eq!(delta_text(&turn), "echo:accepted before loss");
    assert!(
        server.state.agents().run_id(&session).is_some(),
        "the launched session remains registered"
    );

    server
        .state
        .catalog()
        .publish(vec![json!({ "id": "test-model", "object": "model" })]);
    server
        .state
        .menu()
        .set_selected("test-model")
        .expect("the retained model can be selected for recovery");
    next_wait(&mut socket).await;
    answer(&mut socket, "recovered after selection").await;
    let turn = collect_turn(&mut socket).await;
    assert_eq!(
        delta_text(&turn),
        "echo:recovered after selection",
        "the same run answers the next input on its frozen binding"
    );
    {
        let requests = server.captured.lock().expect("the capture lock is healthy");
        assert_eq!(
            requests.len(),
            2,
            "both turns reach the gateway on the frozen model"
        );
        assert_eq!(requests[0]["model"], "test-model");
        assert_eq!(requests[1]["model"], "test-model");
        assert_eq!(
            role_content_pairs(&requests[1]),
            vec![
                pair("user", "accepted before loss"),
                pair("assistant", "echo:accepted before loss"),
                pair("user", "recovered after selection"),
            ],
            "the same run retains its message list across the selection loss"
        );
    }
    socket.close().await;
}
