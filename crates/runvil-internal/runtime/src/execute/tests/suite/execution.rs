//! Section execution over offline fixtures: author checkpoints, prologue
//! return, store fall-through, concurrent run isolation, and the typed
//! execution-error contract.

use runvil_types::error::ErrorCode;

use crate::test_support::recording::detail;
use crate::test_support::scenario::{Record, Scenario};

const LOG_CHECKPOINTS: &str =
    include_str!("../../../../tests/prompts/execution/log-checkpoints.md");
const PROLOGUE_RETURN: &str =
    include_str!("../../../../tests/prompts/execution/prologue-return.md");
const STORE_FALLTHROUGH: &str =
    include_str!("../../../../tests/prompts/execution/store-fallthrough.md");
const STORE_TRIAD: &str = include_str!("../../../../tests/prompts/execution/store-triad.md");
const REPLY_SUBSTITUTION_NIL: &str =
    include_str!("../../../../tests/prompts/invalid/reply-substitution-nil.md");
const ITEM_OUTSIDE_FANOUT: &str =
    include_str!("../../../../tests/prompts/invalid/item-outside-fanout.md");

struct ExecutionErrorFixture {
    name: &'static str,
    source: &'static str,
    code: ErrorCode,
    message_fragment: &'static str,
}

const EXECUTION_ERROR_FIXTURES: &[ExecutionErrorFixture] = &[
    // Both fixtures fail at the lazy-`prose` read site inside the section's
    // Lua, so the run ends with the substitution's own code: the
    // substitution error is an ordinary, pcall-able Lua error there, not a
    // run-phase failure.
    ExecutionErrorFixture {
        name: "invalid/reply-substitution-nil.md",
        source: REPLY_SUBSTITUTION_NIL,
        code: ErrorCode::Substitution,
        message_fragment: "reply",
    },
    ExecutionErrorFixture {
        name: "invalid/item-outside-fanout.md",
        source: ITEM_OUTSIDE_FANOUT,
        code: ErrorCode::Substitution,
        message_fragment: "nil",
    },
];

fn checkpoints(records: &[Record]) -> Vec<Record> {
    records
        .iter()
        .filter(|record| record.detail.starts_with("Lua: "))
        .cloned()
        .collect()
}

#[test]
fn log_fixture_reports_exact_author_checkpoints() {
    let trace = Scenario::new(LOG_CHECKPOINTS).run();
    assert_eq!(trace.text(), "logged");
    assert_eq!(
        trace
            .read("state.txt")
            .expect("the prepare section writes state"),
        "prepared"
    );
    assert_eq!(
        checkpoints(&trace.records()),
        [
            Record::new("Log Checkpoints", "Lua: H1 ran"),
            Record::new("Prepare", "Lua: prepare started"),
            Record::new("Prepare", "Lua: prepare finished"),
            Record::new("Finish", "Lua: finish started"),
        ]
    );
}

#[test]
fn prologue_return_fixture_skips_model_and_epilog() {
    let trace = Scenario::new(PROLOGUE_RETURN).args("early result").run();
    assert_eq!(trace.text(), "early result");
    assert!(
        trace.read("unreachable.txt").is_err(),
        "the epilog after a scalar prologue return must not run"
    );
    assert_eq!(
        checkpoints(&trace.records()),
        [Record::new("Stop Early", "Lua: returning early")]
    );
}

#[test]
fn store_fixture_persists_state_across_fall_through() {
    let trace = Scenario::new(STORE_FALLTHROUGH).args("handoff value").run();
    assert_eq!(trace.text(), "handoff value");
    assert_eq!(
        trace
            .read("handoff.txt")
            .expect("the handoff remains stored"),
        "handoff value"
    );
    assert_eq!(
        checkpoints(&trace.records()),
        [
            Record::new("Write", "Lua: writing state"),
            Record::new("Read", "Lua: reading state"),
        ]
    );
}

#[test]
fn concurrent_runs_keep_their_records_separate() {
    let start = |args| Scenario::new(STORE_FALLTHROUGH).args(args).start();
    let (mut first, mut second) = (start("first result"), start("second result"));
    // Both runs take every step on this thread, one step of each in turn.
    assert!(first.step(), "the first run is pending on its store write");
    assert!(
        second.step(),
        "the second run starts and parks while the first is still pending"
    );
    while first.step() | second.step() {}
    let (first, second) = (first.finish(), second.finish());
    assert_eq!(first.text(), "first result");
    assert_eq!(second.text(), "second result");

    for (name, trace) in [("first", &first), ("second", &second)] {
        let records = trace.records();
        assert_eq!(
            checkpoints(&records),
            [
                Record::new("Write", "Lua: writing state"),
                Record::new("Read", "Lua: reading state"),
            ],
            "each interleaved run must retain its own checkpoints"
        );
        let terminals = records
            .iter()
            .filter(|record| {
                record.section == "Store Fall-through"
                    && record.detail == detail::RUN_SUCCEEDED.to_string()
            })
            .count();
        assert_eq!(
            terminals, 1,
            "the {name} run must retain exactly its own terminal run record"
        );
    }
}

#[test]
fn store_triad_numbered_vs_verbatim_vs_wrapped() {
    let trace = Scenario::new(STORE_TRIAD).run();
    assert_eq!(trace.text(), "1| alpha\n2| beta|alpha\nbeta");
}

#[test]
fn execution_error_fixtures_report_their_typed_contract() {
    for fixture in EXECUTION_ERROR_FIXTURES {
        let trace = Scenario::new(fixture.source).run();
        let error = trace.error();
        assert_eq!(
            error.code, fixture.code,
            "fixture {} returned the wrong error code: {error:?}",
            fixture.name
        );
        assert!(
            error.to_string().contains(fixture.message_fragment),
            "fixture {} error did not contain {:?}: {error}",
            fixture.name,
            fixture.message_fragment
        );
    }
}
