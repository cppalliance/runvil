//! The same-origin rule's cases: exact match, absent Origin, normalization,
//! and every mismatch that must fail closed.

use super::*;

#[test]
fn a_native_client_with_a_valid_request_authority_is_admitted() {
    assert!(same_origin_authority_allowed(None, Some("127.0.0.1:7910")));
    assert!(!same_origin_authority_allowed(None, None));
    assert!(!same_origin_authority_allowed(None, Some("localhost:bad")));
}

#[test]
fn matching_normalized_authorities_are_admitted() {
    for (origin, authority) in [
        ("http://127.0.0.1:7910", "127.0.0.1:7910"),
        ("http://localhost:7910", "LOCALHOST:7910"),
        ("http://[::1]:7910", "[::1]:7910"),
        ("http://[0:0:0:0:0:0:0:1]:7910", "[::1]:7910"),
    ] {
        assert!(
            same_origin_authority_allowed(Some(origin), Some(authority)),
            "{origin} must match {authority}"
        );
    }
}

#[test]
fn mismatches_wrong_ports_and_malformed_values_are_refused() {
    for (origin, authority) in [
        ("http://127.0.0.1:7910", "localhost:7910"),
        ("http://localhost:7910", "localhost:7911"),
        ("http://[0:0:0:0:0:0:0:1]:7910", "[::1]:7911"),
        ("http://[0:0:0:0:0:0:0:2]:7910", "[::1]:7910"),
        ("http://evil.example:7910", "localhost:7910"),
        ("https://localhost:7910", "localhost:7910"),
        ("http://localhost:bad", "localhost:7910"),
        ("http://localhost:7910/path", "localhost:7910"),
        ("null", "localhost:7910"),
        ("", "localhost:7910"),
    ] {
        assert!(
            !same_origin_authority_allowed(Some(origin), Some(authority)),
            "{origin} must not match {authority}"
        );
    }
}

#[test]
fn a_userinfo_bearing_authority_is_refused() {
    assert!(!same_origin_authority_allowed(
        Some("http://user@localhost:7910"),
        Some("localhost:7910")
    ));
    assert!(!same_origin_authority_allowed(
        Some("http://localhost:7910"),
        Some("user@localhost:7910")
    ));
}
