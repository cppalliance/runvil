//! Tests for the model catalog fetch against an axum mock gateway: the body
//! bounds, the failure causes kept in the message, the entry filter, and
//! failure prose that carries no Gateway-supplied text.

use runvil_engine::ErrorCode;

use super::*;

/// The text the fetch appended to `err`'s message after its prose: the
/// provider detail or the cause, which the prose never holds.
fn appended(err: &Error) -> &str {
    err.message
        .strip_prefix(err.prose.as_str())
        .and_then(|rest| rest.strip_prefix(": "))
        .unwrap_or_else(|| panic!("the message extends the prose: {err:?}"))
}

async fn spawn_models(app: axum::Router) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

#[tokio::test]
async fn fetch_model_catalog_bounds_and_reports_non_success_body() {
    use axum::Router;
    use axum::routing::get;

    async fn models() -> (axum::http::StatusCode, String) {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "e".repeat(MAX_CATALOG_ERROR_BODY * 4),
        )
    }
    let app = Router::new().route("/models", get(models));
    let addr = spawn_models(app).await;

    let err = fetch_model_catalog(&format!("http://{addr}"), "tok")
        .await
        .expect_err("a 500 response must surface as an error");
    assert_eq!(err.code, ErrorCode::ServerError);
    assert_eq!(
        err.prose,
        "the model backend reported a fault of its own (status 500)"
    );
    let detail = appended(&err);
    assert!(
        detail.len() < MAX_CATALOG_ERROR_BODY + 128,
        "the error-path body must be bounded, got {} bytes",
        detail.len()
    );
}

#[tokio::test]
async fn fetch_model_catalog_bounds_an_oversized_success_body() {
    use axum::Router;
    use axum::routing::get;

    // A 200 response whose body exceeds the success cap must be refused
    // BEFORE decoding, not buffered unbounded. The body is deliberately not
    // valid JSON: the bound must trip first, regardless of contents.
    async fn models() -> (axum::http::StatusCode, String) {
        let oversized = usize::try_from(MAX_CATALOG_BODY).unwrap() + 1;
        (axum::http::StatusCode::OK, "e".repeat(oversized))
    }
    let app = Router::new().route("/models", get(models));
    let addr = spawn_models(app).await;

    let err = fetch_model_catalog(&format!("http://{addr}"), "tok")
        .await
        .expect_err("an oversized success body must be refused");
    assert_eq!(err.code, ErrorCode::MalformedResponse);
    assert!(
        err.to_string().contains("exceeds"),
        "the bound must report the size limit, got {err:?}"
    );
    assert_eq!(
        err.message, err.prose,
        "our own wording is not provider text"
    );
}

#[tokio::test]
async fn fetch_model_catalog_keeps_the_json_decode_error_in_the_message() {
    use axum::Router;
    use axum::routing::get;

    // MODEL-009: a 200 body that is not a valid model list is classified as
    // MalformedResponse, and the underlying `serde_json::Error` is appended
    // to the message and kept out of the prose.
    async fn models() -> (axum::http::StatusCode, String) {
        (axum::http::StatusCode::OK, "{ this is not json".to_owned())
    }
    let app = Router::new().route("/models", get(models));
    let addr = spawn_models(app).await;

    let err = fetch_model_catalog(&format!("http://{addr}"), "tok")
        .await
        .expect_err("an undecodable body must surface as an error");
    assert_eq!(err.code, ErrorCode::MalformedResponse);
    let decode = serde_json::from_slice::<ModelsListResponse>(b"{ this is not json")
        .expect_err("the body is not JSON")
        .to_string();
    assert_eq!(appended(&err), decode);
    assert!(!err.prose.contains(&decode), "got {err:?}");
}

#[tokio::test]
async fn fetch_model_catalog_skips_entries_without_a_context_window() {
    use axum::Router;
    use axum::routing::get;

    // A gateway with speech-to-text lists its transcription models beside
    // the inference models, and those entries omit `context` and
    // `thinking` (they answer no completion request). The fetch must keep
    // the inference models' `ModelInfo` values instead of rejecting the whole
    // list, otherwise the Harness binds each run on such a gateway to a
    // fallback model info and the context precheck refuses real conversations.
    async fn models() -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({
            "object": "list",
            "data": [
                { "id": "chat-model", "object": "model", "kind": "chat",
                  "description": "a chat model", "context": 1_000_000, "thinking": "never" },
                { "id": "whisper-base-en", "object": "model", "kind": "transcription" },
                { "id": "whisper-small-en", "object": "model", "kind": "transcription" }
            ]
        }))
    }
    let app = Router::new().route("/models", get(models));
    let addr = spawn_models(app).await;

    let catalog = fetch_model_catalog(&format!("http://{addr}"), "tok")
        .await
        .expect("transcription entries must not fail the inference catalog");
    assert_eq!(
        catalog.models().len(),
        1,
        "only the inference model gets a model info"
    );
    let chat = catalog
        .get(&ModelId::gateway("chat-model").expect("valid id"))
        .expect("the inference model survives the filter");
    assert_eq!(
        chat.context(),
        NonZeroU32::new(1_000_000).expect("non-zero")
    );
    assert_eq!(chat.thinking(), ThinkingMode::Never);
}

#[tokio::test]
async fn fetch_model_catalog_reads_a_provider_only_when_the_entry_names_one() {
    use axum::Router;
    use axum::routing::get;

    async fn models() -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({
            "object": "list",
            "data": [
                { "id": "grok", "object": "model", "kind": "chat", "description": "d",
                  "context": 4096, "thinking": "never", "provider": "xai" },
                { "id": "local", "object": "model", "kind": "chat", "description": "d",
                  "context": 4096, "thinking": "never" }
            ]
        }))
    }
    let app = Router::new().route("/models", get(models));
    let addr = spawn_models(app).await;

    let catalog = fetch_model_catalog(&format!("http://{addr}"), "tok")
        .await
        .expect("an optional provider decodes");
    let provider = |name: &str| {
        catalog
            .get(&ModelId::gateway(name).expect("valid id"))
            .expect("the model is in the catalog")
            .provider()
            .map(str::to_owned)
    };
    assert_eq!(provider("grok").as_deref(), Some("xai"));
    assert_eq!(provider("local"), None);
}

#[tokio::test]
async fn fetch_model_catalog_still_rejects_a_zero_context_window() {
    use axum::Router;
    use axum::routing::get;

    // Skipping applies only to entries with no context field at all; an
    // inference entry that declares a zero window is still malformed.
    async fn models() -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({
            "object": "list",
            "data": [
                { "id": "broken", "object": "model", "kind": "chat",
                  "description": "d", "context": 0, "thinking": "never" }
            ]
        }))
    }
    let app = Router::new().route("/models", get(models));
    let addr = spawn_models(app).await;

    let err = fetch_model_catalog(&format!("http://{addr}"), "tok")
        .await
        .expect_err("a zero context window is malformed");
    assert_eq!(err.code, ErrorCode::MalformedResponse);
    assert!(err.prose.contains("zero-token"), "got {err:?}");
    assert!(!err.prose.contains("broken"), "got {err:?}");
    assert!(
        appended(&err).contains("broken"),
        "the model name belongs in the message only, got {err:?}"
    );
}

/// Serves `data` as the gateway model list and returns the fetch failure.
async fn fetch_rejection(data: serde_json::Value) -> Error {
    let list = serde_json::json!({ "object": "list", "data": data });
    let app = axum::Router::new().route(
        "/models",
        axum::routing::get(move || async move { axum::Json(list) }),
    );
    let addr = spawn_models(app).await;
    fetch_model_catalog(&format!("http://{addr}"), "tok")
        .await
        .expect_err("the model list is malformed")
}

#[tokio::test]
async fn fetch_model_catalog_keeps_the_name_of_a_model_without_a_thinking_mode_out_of_the_prose() {
    let err = fetch_rejection(serde_json::json!([
        { "id": "thoughtless", "object": "model", "kind": "chat",
          "description": "d", "context": 4096 }
    ]))
    .await;
    assert_eq!(err.code, ErrorCode::MalformedResponse);
    assert!(!err.prose.contains("thoughtless"), "got {err:?}");
    assert!(
        appended(&err).contains("thoughtless"),
        "the model name belongs in the message only, got {err:?}"
    );
}

#[tokio::test]
async fn fetch_model_catalog_keeps_a_duplicate_id_out_of_the_prose() {
    let entry = serde_json::json!(
        { "id": "twin", "object": "model", "kind": "chat",
          "description": "d", "context": 4096, "thinking": "never" }
    );
    let err = fetch_rejection(serde_json::json!([entry.clone(), entry])).await;
    assert_eq!(err.code, ErrorCode::MalformedResponse);
    assert!(!err.prose.contains("twin"), "got {err:?}");
    assert!(
        appended(&err).contains("gateway/twin"),
        "the duplicate identity belongs in the message only, got {err:?}"
    );
}

#[tokio::test]
async fn fetch_model_catalog_bounds_a_gateway_supplied_detail() {
    let long = "n".repeat(MAX_CATALOG_ERROR_BODY * 4);
    let entry = |context: u32, thinking: Option<&str>| {
        serde_json::json!({ "id": long, "object": "model", "kind": "chat",
            "description": "d", "context": context, "thinking": thinking })
    };
    let rejections = [
        serde_json::json!([entry(0, Some("never"))]),
        serde_json::json!([entry(4096, None)]),
        serde_json::json!([entry(4096, Some("never")), entry(4096, Some("never"))]),
    ];
    for data in rejections {
        let err = fetch_rejection(data).await;
        assert_eq!(err.code, ErrorCode::MalformedResponse);
        let detail = appended(&err);
        assert!(
            detail.chars().count() <= MAX_CATALOG_ERROR_BODY,
            "a Gateway-supplied detail must be bounded, got {} chars",
            detail.chars().count()
        );
    }
}

#[tokio::test]
async fn fetch_model_catalog_keeps_an_invalid_id_out_of_the_message() {
    let err = fetch_rejection(serde_json::json!([
        { "id": "smuggled\u{7}id", "object": "model", "kind": "chat",
          "description": "d", "context": 4096, "thinking": "never" }
    ]))
    .await;
    assert_eq!(err.code, ErrorCode::MalformedResponse);
    let message = err.to_string();
    assert!(!message.contains("smuggled"), "got {err:?}");
    assert!(!message.contains('\u{7}'), "got {err:?}");
}

#[tokio::test]
async fn fetch_model_catalog_keeps_a_body_read_failure_in_the_message() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // MODEL-010: a non-success response whose body cannot be fully read
    // (the server promises a large body then drops the connection) must
    // surface as a typed transport failure whose message, not its prose,
    // holds the `reqwest::Error`.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let header = "HTTP/1.1 500 Internal Server Error\r\n\
                 Content-Length: 1000000\r\n\r\n";
            let _ = sock.write_all(header.as_bytes()).await;
            let _ = sock.write_all(b"abc").await;
            // Socket drops here: the promised body never completes.
        }
    });

    let err = fetch_model_catalog(&format!("http://{addr}"), "tok")
        .await
        .expect_err("a truncated error body must surface as an error");
    assert_eq!(err.code, ErrorCode::Transport);
    assert_eq!(err.prose, "the connection to the model backend failed");
    assert!(
        !appended(&err).is_empty(),
        "the message holds the reqwest read error, got {err:?}"
    );
}
