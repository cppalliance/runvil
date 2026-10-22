//! Binding the gateway's listener to:
//! - The configured address, if it is set
//! - The preferred port if it is set and available
//! - Any available port

use std::net::SocketAddr;

use tokio::{
    net::{TcpListener, TcpSocket},
    runtime::EnterGuard,
};

const MAX_BACKLOG_CONNECTIONS: u32 = 1024;

/// Binds the gateway's listener to `addr`.
///
/// When `addr` asks for any free port (port 0) and `preferred_port` names one,
/// that port is tried first, enabling a gateway relaunched by its supervisor to
/// come back on the address its clients already hold. If the preferred port is
/// taken, a free port is bound instead and a warning is logged. `preferred_port` is
/// ignored if `addr` asks for a port other than 0.
pub(super) fn bind_listener(
    addr: SocketAddr,
    preferred_port: Option<u16>,
    runtime: &tokio::runtime::Handle,
) -> std::io::Result<TcpListener> {
    let context = runtime.enter();

    if addr.port() == 0
        && let Some(preferred_port) = preferred_port
    {
        let preferred_addr = SocketAddr::new(addr.ip(), preferred_port);

        match bind_addr(preferred_addr, &context) {
            Ok(listener) => return Ok(listener),
            Err(error) => tracing::warn!(
                "could not bind the preferred port {preferred_port}: {error}; binding a free port instead"
            ),
        }
    }

    bind_addr(addr, &context)
}

/// Binds `address`, reusing a port even if its previous connections are still in
/// `TIME_WAIT`, so a restarted gateway can take back the port it just released.
///
/// Must run inside a tokio runtime context, which the caller supplies via the
/// `context` param
fn bind_addr(address: SocketAddr, _context: &EnterGuard<'_>) -> std::io::Result<TcpListener> {
    let socket = if address.is_ipv4() {
        TcpSocket::new_v4()?
    } else {
        TcpSocket::new_v6()?
    };

    // Unix only: on Windows `SO_REUSEADDR` lets a second socket take a port that is in use
    #[cfg(unix)]
    socket.set_reuseaddr(true)?;

    socket.bind(address)?;

    // backlog connections defines the maximum number of pending connections
    // queued by the operating system at any given time. Connection are removed
    // from the queue with `TcpListener::accept`. When the queue is full, the
    // operating-system will start rejecting connections.
    socket.listen(MAX_BACKLOG_CONNECTIONS)
}

#[cfg(test)]
#[path = "bind-tests.rs"]
mod tests;
