//! Unit tests for the Gateway version comparison.

use super::*;

use std::sync::Arc;

use tokio::sync::broadcast;

/// A recording status sink standing in for the status bus, and the push
/// facade the report writes through.
fn status_recorder() -> (
    Push,
    broadcast::Receiver<workshop_protocol::StatusBarUpdate>,
    workshop_registry::Registration,
) {
    let registry = workshop_registry::Registry::new();
    let (tx, rx) = broadcast::channel(16);
    let sink = registry.register_sink::<dyn workshop_registry::StatusSink>(Arc::new(
        workshop_registry::StatusSinkAdapter::new(move |update| {
            let _ = tx.send(update);
        }),
    ));
    (registry.push(), rx, sink)
}

#[test]
fn parses_a_plain_three_part_version() {
    assert_eq!(
        Version::parse("0.4.1"),
        Some(Version {
            major: 0,
            minor: 4,
            patch: 1
        })
    );
}

#[test]
fn parses_a_leading_v_and_a_prerelease_suffix() {
    let expected = Version::parse("1.2.3");
    assert_eq!(Version::parse("v1.2.3"), expected);
    assert_eq!(Version::parse("1.2.3-rc.1"), expected);
    assert_eq!(Version::parse("1.2.3+build.7"), expected);
}

#[test]
fn a_two_part_version_treats_the_missing_patch_as_zero() {
    assert_eq!(Version::parse("2.5"), Version::parse("2.5.0"));
}

#[test]
fn a_version_that_is_not_numbers_is_unreadable() {
    for text in ["", "  ", "unknown", "1.x.0", "1.2.3.4", "-1.0.0"] {
        assert_eq!(Version::parse(text), None, "{text}");
    }
}

#[test]
fn versions_compare_numerically_not_as_strings() {
    let older = Version::parse("0.9.0").expect("a version");
    let newer = Version::parse("0.10.0").expect("a version");
    assert!(newer > older, "0.10.0 is newer than 0.9.0");
    assert!(newer.is_at_least(older));
    assert!(!older.is_at_least(newer));
    assert_eq!(older.to_string(), "0.9.0");
}

#[test]
fn an_equal_version_satisfies_the_minimum() {
    assert_eq!(
        VersionCheck::compare("0.4.0", "0.4.0"),
        VersionCheck::Satisfied {
            running: Version::parse("0.4.0").expect("a version"),
            minimum: Version::parse("0.4.0").expect("a version"),
        }
    );
}

#[test]
fn an_older_gateway_is_reported_too_old() {
    let check = VersionCheck::compare("0.3.9", "0.4.0");
    assert!(matches!(check, VersionCheck::TooOld { .. }), "{check:?}");
    assert!(check.is_problem());
}

#[test]
fn a_newer_gateway_satisfies_the_minimum() {
    assert!(!VersionCheck::compare("2.0.0", "0.4.0").is_problem());
}

#[test]
fn an_empty_minimum_means_the_pin_recorded_none() {
    let check = VersionCheck::compare("0.1.0", "");
    assert_eq!(check, VersionCheck::NoMinimumRecorded);
    assert!(
        !check.is_problem(),
        "a placeholder pin must not warn about every Gateway"
    );
}

#[test]
fn an_unreadable_version_is_reported_rather_than_assumed() {
    let check = VersionCheck::compare("nightly", "0.4.0");
    assert!(
        matches!(check, VersionCheck::Unreadable { .. }),
        "an unreadable running version is a problem, not a pass: {check:?}"
    );
    assert!(check.is_problem());
    assert!(VersionCheck::compare("0.4.0", "not a version").is_problem());
}

#[test]
fn a_too_old_gateway_warns_on_the_status_bar_and_names_both_versions() {
    let (push, mut rx, _sink) = status_recorder();
    announce(&VersionCheck::compare("0.3.9", "0.4.0"), &push);
    let frame = rx.try_recv().expect("the warning is pushed");
    assert_eq!(frame.label, "Gateway too old");
    assert!(
        frame.description.contains("0.3.9") && frame.description.contains("0.4.0"),
        "both versions are named: {}",
        frame.description
    );
}

#[test]
fn an_unreadable_version_warns_on_the_status_bar() {
    let (push, mut rx, _sink) = status_recorder();
    announce(&VersionCheck::compare("nightly", "0.4.0"), &push);
    let frame = rx.try_recv().expect("the warning is pushed");
    assert_eq!(frame.label, "Gateway version unknown");
    assert!(
        frame.description.contains("nightly"),
        "{}",
        frame.description
    );
}

#[test]
fn a_satisfied_check_says_nothing_on_the_status_bar() {
    let (push, mut rx, _sink) = status_recorder();
    announce(&VersionCheck::compare("0.4.0", "0.4.0"), &push);
    assert!(
        rx.try_recv().is_err(),
        "a healthy boot pushes no status line"
    );
}

#[test]
fn an_unfilled_pin_warns_on_the_status_bar_in_a_release_build() {
    let (push, mut rx, _sink) = status_recorder();
    announce_for(&VersionCheck::NoMinimumRecorded, &push, Build::Release);
    let frame = rx.try_recv().expect("the unfilled pin is pushed");
    assert_eq!(frame.label, "Gateway version unchecked");
    assert!(
        frame.description.contains("no minimum version"),
        "the description names the cause: {}",
        frame.description
    );
    assert!(
        frame.description.contains("nothing checks"),
        "the description names the consequence: {}",
        frame.description
    );
}

#[test]
fn an_unfilled_pin_stays_off_the_status_bar_in_a_debug_build() {
    let (push, mut rx, _sink) = status_recorder();
    announce_for(&VersionCheck::NoMinimumRecorded, &push, Build::Debug);
    assert!(
        rx.try_recv().is_err(),
        "a developer's checkout must not repeat this line on every launch"
    );
}

#[test]
fn the_public_entry_point_follows_the_build_it_was_compiled_into() {
    let (push, mut rx, _sink) = status_recorder();
    announce(&VersionCheck::NoMinimumRecorded, &push);
    if cfg!(debug_assertions) {
        assert!(
            rx.try_recv().is_err(),
            "this test build is a debug build, so the pin reaches the log only"
        );
    } else {
        let frame = rx
            .try_recv()
            .expect("a release build shows the unfilled pin");
        assert_eq!(frame.label, "Gateway version unchecked");
    }
}

#[test]
fn the_pin_minimum_is_the_compiled_default() {
    // Whatever the pin holds, the constant is its value, never a guess. A
    // placeholder becomes an empty minimum, which the comparison reads as
    // "no minimum recorded" rather than as a version to enforce.
    assert!(
        MIN_GATEWAY.is_empty() || Version::parse(MIN_GATEWAY).is_some(),
        "the compiled minimum is empty or a version: {MIN_GATEWAY}"
    );
    assert!(
        !MIN_GATEWAY.contains("PLACEHOLDER"),
        "a placeholder must never become a minimum to compare against"
    );
    assert_eq!(
        VersionCheck::against_pin("0.0.1"),
        VersionCheck::compare("0.0.1", MIN_GATEWAY)
    );
}
