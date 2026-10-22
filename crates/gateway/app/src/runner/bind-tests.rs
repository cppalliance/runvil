use std::net::SocketAddr;

use super::*;

/// A get a port that was is not in use.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("should bind a probe")
        .local_addr()
        .expect("address should exist")
        .port()
}

fn loopback(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

/// create a tokio Runtime for a test.
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .expect("a runtime builds")
}

#[test]
fn a_port_zero_config_binds_the_preferred_port() {
    let runtime = runtime();

    let preferred_port = free_port();

    let listener =
        bind_listener(loopback(0), Some(preferred_port), runtime.handle()).expect("should bind");

    let listener_port = listener
        .local_addr()
        .expect("should have an address")
        .port();

    assert_eq!(listener_port, preferred_port);
}

#[test]
fn a_configured_port_wins_over_the_preferred_port() {
    let runtime = runtime();

    let configured_port = free_port();
    let preferred_port = free_port();

    let listener = bind_listener(
        loopback(configured_port),
        Some(preferred_port),
        runtime.handle(),
    )
    .expect("should bind");

    let listener_port = listener
        .local_addr()
        .expect("should have an address")
        .port();

    assert_eq!(listener_port, configured_port);
}

#[test]
fn a_taken_preferred_port_falls_back_to_a_free_one() {
    let runtime = runtime();

    let holder = std::net::TcpListener::bind("127.0.0.1:0").expect("should bind");
    let used_port = holder.local_addr().expect("should have an address").port();

    let listener =
        bind_listener(loopback(0), Some(used_port), runtime.handle()).expect("should bind");

    let listener_port = listener
        .local_addr()
        .expect("should have an address")
        .port();

    assert_ne!(listener_port, used_port);
}

#[test]
fn no_preferred_port_binds_a_free_port() {
    let runtime = runtime();

    let listener = bind_listener(loopback(0), None, runtime.handle()).expect("binds");

    let listener_port = listener
        .local_addr()
        .expect("should have an address")
        .port();

    assert_ne!(listener_port, 0);
}

/// The server side closing a connection first leaves it in `TIME_WAIT`,
/// which is what a gateway shutting down with clients attached does.
#[tokio::test]
async fn a_released_port_rebinds_while_its_connection_is_in_time_wait() {
    let runtime = tokio::runtime::Handle::current();

    let listener = bind_listener(loopback(0), None, &runtime).expect("should bind");

    let port = listener
        .local_addr()
        .expect("should have an address")
        .port();

    let client = tokio::net::TcpStream::connect(loopback(port))
        .await
        .expect("should connect");
    let (served, _) = listener.accept().await.expect("should accept");
    drop(served);
    drop(listener);
    drop(client);

    let rebound = bind_listener(loopback(0), Some(port), &runtime).expect("should rebind");

    assert_eq!(
        rebound.local_addr().expect("should have an address").port(),
        port
    );
}
