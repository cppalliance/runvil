//! Grep's cooperative yield, seen from the Pack's own future: a search over
//! many files hands control back to the executor before it completes, and
//! dropping it at a pause leaves the filesystem usable.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use runvil_plugin::testing::TestCall;
use runvil_plugin::{Error, OutputTrust, ToolOutput};
use serde_json::{Value, json};

use crate::tools::{built, id};

/// How many files the yield check seeds.
const FILES: usize = 1000;

/// A call with `FILES` small files, each holding one matching line.
fn seeded_call() -> TestCall {
    let call = TestCall::new(id("grep"));
    for index in 0..FILES {
        call.context()
            .access()
            .write(&format!("/f{index:04}.txt"), b"x\n")
            .expect("a seeded file is written");
    }
    call
}

/// The search arguments the yield check uses: one count row per file.
fn args() -> Value {
    json!({ "pattern": "x", "output_mode": "count" })
}

#[test]
fn a_search_over_a_thousand_files_yields_to_the_executor_before_it_completes() {
    let pack = built();
    let call = seeded_call();
    let mut future = pin!(pack.call(call.context(), args()));
    let mut cx = Context::from_waker(Waker::noop());
    let mut pending = 0;
    let result: Result<ToolOutput, Error> = loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(result) => break result,
            Poll::Pending => pending += 1,
        }
    };
    assert!(
        pending >= 31,
        "a pause after each 32 of {FILES} files, and the search paused {pending} times"
    );
    let output = result.expect("the search completes with its result");
    assert_eq!(output.trust(), OutputTrust::Untrusted);
    let rows: Vec<&str> = output.text().lines().collect();
    assert_eq!(
        rows.len(),
        FILES,
        "every file has its row and there is no notice"
    );
    assert_eq!(rows.first(), Some(&"/f0000.txt:1"));
    assert_eq!(rows.last(), Some(&"/f0999.txt:1"));
}

#[test]
fn dropping_a_search_after_its_first_pause_is_safe_and_the_access_still_works() {
    let pack = built();
    let call = seeded_call();
    {
        let mut dropped = pin!(pack.call(call.context(), args()));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(
            dropped.as_mut().poll(&mut cx).is_pending(),
            "the search pauses before it can finish"
        );
    }
    let bytes = call
        .context()
        .access()
        .read("/f0000.txt")
        .expect("the access still reads after the drop");
    assert_eq!(bytes, b"x\n");
    let mut again = pin!(pack.call(call.context(), args()));
    let mut cx = Context::from_waker(Waker::noop());
    let output = loop {
        if let Poll::Ready(result) = again.as_mut().poll(&mut cx) {
            break result.expect("a fresh search runs to its result");
        }
    };
    assert_eq!(output.text().lines().count(), FILES);
}
