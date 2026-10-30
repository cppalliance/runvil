//! Same-origin policy for the Workshop Realtime socket.
//!
//! A browser `Origin` on `/v1/realtime` must match the request's own
//! authority: same host (IP compared by value, domain compared ASCII
//! case-insensitively) and the same explicit port. An absent `Origin`
//! denotes a native client, which is admitted as long as the request
//! authority is present and valid. Missing or malformed values fail closed.
//!
//! The cross-site guard is the security boundary for the loopback socket
//! as a whole; this is the stricter rule for the one route a browser page
//! reaches with credentials, and it is deliberately narrower than the
//! guard.

use axum::http::uri::Authority;

/// Whether a Workshop WebSocket `Origin` matches its request authority.
///
/// An absent `Origin` denotes a native client, but the request authority
/// must still be present and valid. A browser `Origin` must be an exact
/// HTTP origin whose normalized authority equals the validated request
/// authority. Missing or malformed values and host or port mismatches fail
/// closed.
pub(super) fn same_origin_authority_allowed(
    origin: Option<&str>,
    request_authority: Option<&str>,
) -> bool {
    let Some(request_authority) = request_authority.and_then(parse_authority) else {
        return false;
    };
    origin.is_none_or(|origin| {
        parse_http_origin_authority(origin).is_some_and(|origin_authority| {
            same_origin_authority(&origin_authority, &request_authority)
        })
    })
}

/// Compares normalized hosts while preserving explicit port equality.
fn same_origin_authority(left: &Authority, right: &Authority) -> bool {
    left.port_u16() == right.port_u16() && same_authority_host(left.host(), right.host())
}

/// Compares IP hosts by value and domain hosts ASCII case-insensitively.
fn same_authority_host(left: &str, right: &str) -> bool {
    let parse_ip = |host: &str| {
        host.strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(host)
            .parse::<std::net::IpAddr>()
            .ok()
    };
    match (parse_ip(left), parse_ip(right)) {
        (Some(left), Some(right)) => left == right,
        (None, None) => left.eq_ignore_ascii_case(right),
        _ => false,
    }
}

/// Parses an exact HTTP origin and returns its authority.
fn parse_http_origin_authority(origin: &str) -> Option<Authority> {
    let (scheme, authority) = origin.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") {
        return None;
    }
    parse_authority(authority)
}

/// Parses an authority and rejects ports outside the `u16` range.
fn parse_authority(authority: &str) -> Option<Authority> {
    let port = if let Some(bracketed) = authority.strip_prefix('[') {
        let close = bracketed.find(']')?;
        match &bracketed[close + 1..] {
            "" => None,
            suffix => Some(suffix.strip_prefix(':')?),
        }
    } else {
        match authority.split_once(':') {
            Some((host, port)) if !host.is_empty() && !port.contains(':') => Some(port),
            Some(_) => return None,
            None => None,
        }
    };
    if authority.contains('@')
        || port.is_some_and(|port| port.is_empty() || port.parse::<u16>().is_err())
    {
        return None;
    }
    let authority = authority.parse::<Authority>().ok()?;
    if authority.host().is_empty() {
        return None;
    }
    Some(authority)
}

#[cfg(test)]
#[path = "realtime-origin-tests.rs"]
mod tests;
