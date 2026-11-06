//! The shared connection pool: clients built separately reuse one
//! connection to the same gateway.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use axum::Router;
use axum::extract::ConnectInfo;
use axum::routing::post;
use runvil::model::Message;

use super::*;

#[tokio::test]
async fn two_clients_built_separately_send_their_rounds_over_one_connection() {
    let peers: Arc<Mutex<Vec<SocketAddr>>> = Arc::default();
    let seen = Arc::clone(&peers);
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |ConnectInfo(peer): ConnectInfo<SocketAddr>| {
            let seen = Arc::clone(&seen);
            async move {
                seen.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(peer);
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    ok_stream(),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let endpoint = GatewayEndpoint::new(&format!("http://{addr}/v1")).expect("valid test endpoint");
    let key = SecretString::new("tok").expect("non-empty test key");

    for _ in 0..2 {
        GatewayChat::new(endpoint.clone(), key.clone())
            .complete(&[Message::user("hi")], None, &openai_options(), |_| {})
            .await
            .expect("the mock gateway answers the round");
    }

    let peers = peers.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!(peers.len(), 2, "both rounds reach the gateway");
    assert_eq!(
        peers[0], peers[1],
        "the second client reuses the first client's connection"
    );
}
