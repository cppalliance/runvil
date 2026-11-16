//! A stalled browser send is canceled within the bounded cleanup.

use super::*;

#[tokio::test]
async fn stalled_browser_cleanup_is_bounded_after_gateway_disconnect() {
    let probe = StalledPeerProbe::default();
    let gateway = spawn_gateway(
        Router::new()
            .route("/v1/realtime", get(send_large_frame_then_disconnect))
            .with_state(probe.clone()),
    )
    .await;
    let server = TestServer::spawn(&gateway);
    let request = server
        .ws_url("/v1/realtime?intent=transcription")
        .into_client_request()
        .expect("the browser request builds");
    let address = request
        .uri()
        .authority()
        .expect("the server URL has an authority")
        .as_str()
        .parse()
        .expect("the server listens on a socket address");
    let tcp = tokio::net::TcpSocket::new_v4().expect("the browser socket opens");
    tcp.set_recv_buffer_size(4096)
        .expect("the browser receive buffer is bounded");
    let stream = tcp.connect(address).await.expect("the browser connects");
    let (mut socket, _) = tokio_tungstenite::client_async(request, stream)
        .await
        .expect("the Workshop Realtime socket upgrades");
    tokio::time::timeout(RECV_TIMEOUT, probe.wait_for_frame())
        .await
        .expect("the Gateway fills the relay's browser send");

    // Keep the browser stalled beyond the relay's 500 ms send deadline;
    // reading immediately would unblock the very send this test must cancel.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let first = tokio::time::timeout(RECV_TIMEOUT, socket.next())
        .await
        .expect("bounded relay cleanup releases the stalled browser");
    assert!(
        !matches!(first, Some(Ok(ClientMessage::Binary(_)))),
        "the stalled send is canceled before peer reads can release it"
    );
}
