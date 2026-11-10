//! Gateway response handling: a refused connection or a stalled gateway is
//! a transport failure with its cause, a success body with the wrong shape
//! or invalid UTF-8 is a backend failure with its cause, an oversized
//! success body is rejected rather than truncated, a success body that
//! fails mid-read is a transport failure with its cause, and an error body
//! is bounded, sanitized, and keeps a mid-read failure as the cause. A
//! cause is in the message after the prose, never in the prose.

use super::*;

use std::time::Duration;

use runvil_engine::{Error, ErrorCode};

use crate::search::{MAX_ERROR_BODY, MAX_RESPONSE_BODY};

/// Asserts `err` failed as `code` with `prose`, and that its message holds
/// the prose followed by a cause the prose leaves out.
fn assert_caused(err: &Error, code: ErrorCode, prose: &str) {
    assert_eq!(err.code, code);
    assert_eq!(err.prose, prose);
    let cause = err.message.strip_prefix(&format!("{prose}: "));
    assert!(
        cause.is_some_and(|cause| !cause.is_empty()),
        "the cause follows the prose in the message only: {err:?}"
    );
}

#[tokio::test]
async fn a_refused_connection_is_a_transport_failure_with_its_cause() {
    // Bind then drop the listener so the port is closed and the connection
    // is refused deterministically.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let err = search_at(&format!("http://{addr}"))
        .search(&query("hi"))
        .await
        .expect_err("a refused connection must surface as an error");
    assert_caused(&err, ErrorCode::ToolTransport, "request failed");
}

#[tokio::test]
async fn stalling_gateway_times_out_as_transport() {
    async fn web_search() -> Json<Value> {
        std::future::pending().await
    }
    let mock = MockServer::spawn(Router::new().route("/tools/web_search", post(web_search))).await;
    let search = GatewaySearch::with_timeout(
        GatewayEndpoint::new(&mock.url()).expect("valid test endpoint"),
        SecretString::new("tok").expect("non-empty test key"),
        Duration::from_millis(200),
    );

    let err = search
        .search(&query("hi"))
        .await
        .expect_err("a stalled gateway must surface as an error");
    assert_caused(&err, ErrorCode::ToolTransport, "request failed");
    assert!(
        err.message.contains("timed out"),
        "the timeout must be kept in the message: {err:?}"
    );
}

#[tokio::test]
async fn malformed_success_json_is_a_backend_failure_with_its_cause() {
    async fn web_search() -> Json<Value> {
        // Missing the required `results` array: valid JSON, wrong shape.
        Json(serde_json::json!({ "unexpected": true }))
    }
    let mock = MockServer::spawn(Router::new().route("/tools/web_search", post(web_search))).await;

    let err = search_at(&mock.url())
        .search(&query("hi"))
        .await
        .expect_err("a wrong-shaped success body must be rejected");
    assert_caused(&err, ErrorCode::ToolBackend, "malformed search response");
}

#[tokio::test]
async fn non_utf8_success_body_is_a_backend_failure_with_its_cause() {
    async fn web_search() -> Vec<u8> {
        vec![b'{', 0xff, 0xfe, b'}']
    }
    let mock = MockServer::spawn(Router::new().route("/tools/web_search", post(web_search))).await;

    let err = search_at(&mock.url())
        .search(&query("hi"))
        .await
        .expect_err("a success body that is not UTF-8 must be rejected");
    assert_caused(
        &err,
        ErrorCode::ToolBackend,
        "response body was not valid UTF-8",
    );
}

#[tokio::test]
async fn oversized_success_body_is_rejected() {
    async fn web_search() -> String {
        "x".repeat(MAX_RESPONSE_BODY + 4096)
    }
    let mock = MockServer::spawn(Router::new().route("/tools/web_search", post(web_search))).await;

    let err = search_at(&mock.url())
        .search(&query("hi"))
        .await
        .expect_err("an oversized success body must be rejected, not truncated");
    assert_eq!(err.code, ErrorCode::ToolBackend);
    assert_eq!(
        err.prose,
        format!("response body exceeded {MAX_RESPONSE_BODY} bytes"),
        "the error must name the cap overflow"
    );
    assert_eq!(err.message, err.prose);
}

#[tokio::test]
async fn oversized_error_body_is_bounded_and_sanitized() {
    async fn web_search() -> (axum::http::StatusCode, String) {
        // Oversized and control-laden so both bounding and sanitization run.
        let mut body = "line-one\nline-two\ttab".to_owned();
        body.push_str(&"e".repeat(MAX_ERROR_BODY * 4));
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, body)
    }
    let mock = MockServer::spawn(Router::new().route("/tools/web_search", post(web_search))).await;

    let err = search_at(&mock.url())
        .search(&query("hi"))
        .await
        .expect_err("a 500 response must surface as an error");
    assert_eq!(err.code, ErrorCode::ToolBackend);
    assert_eq!(err.message, err.prose);
    let message = err.prose;
    assert!(
        message.starts_with("backend returned 500: line-one\\nline-two\\ttab"),
        "error must name the status and escape the body: {message}"
    );
    assert!(
        !message.contains('\n') && !message.contains('\t'),
        "control characters must be escaped, got: {message}"
    );
    assert!(
        message.len() < MAX_ERROR_BODY + 128,
        "the error-path body must be bounded, got {} bytes",
        message.len()
    );
}

/// A raw TCP mock that answers with `status`, promises a large body via
/// `Content-Length`, sends a few bytes, then drops the connection so the
/// body read fails partway.
async fn spawn_truncated_reply(status: &'static str) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;
            let header = format!("HTTP/1.1 {status}\r\nContent-Length: 100000\r\n\r\n");
            let _ = socket.write_all(header.as_bytes()).await;
            let _ = socket.write_all(b"partial").await;
            let _ = socket.flush().await;
        }
    });
    (addr, handle)
}

#[tokio::test]
async fn success_body_read_failure_is_a_transport_failure_with_its_cause() {
    let (addr, handle) = spawn_truncated_reply("200 OK").await;

    let err = search_at(&format!("http://{addr}"))
        .search(&query("hi"))
        .await
        .expect_err("a truncated 200 body must surface as an error");
    assert_caused(&err, ErrorCode::ToolTransport, "reading response failed");
    handle.abort();
}

#[tokio::test]
async fn error_body_read_failure_is_kept_as_the_cause() {
    let (addr, handle) = spawn_truncated_reply("500 Internal Server Error").await;

    let err = search_at(&format!("http://{addr}"))
        .search(&query("hi"))
        .await
        .expect_err("a truncated 500 body must surface as an error");
    assert_caused(
        &err,
        ErrorCode::ToolBackend,
        "backend returned 500, and its error body could not be read",
    );
    handle.abort();
}
