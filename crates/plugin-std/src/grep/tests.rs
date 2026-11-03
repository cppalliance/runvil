//! Unit tests for the `Grep` tool. This file holds the helpers the topic
//! files share: a call over a fresh in-memory filesystem, a seeded tree,
//! and the ways to drive Grep, through its arguments and through lowered
//! limits over any [`Source`].

mod arguments;
mod fake;
mod filters;
mod paging;
mod patterns;
mod render;
mod search;
mod skips;
mod text;
mod window;

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use runvil_plugin::testing::TestCall;
use runvil_plugin::{Error, OutputTrust, ToolId, ToolOutput};
use serde_json::Value;

use super::args;
use super::found::Limits;
use super::source::Source;
use crate::spec::{self, Definition};

/// Drives a future to its output, polling again after each `Pending`.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
}

/// Drives a future to its output and counts how many times it returned
/// `Pending` on the way.
fn count_pendings<F: Future>(future: F) -> (F::Output, usize) {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    let mut pending = 0;
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return (output, pending);
        }
        pending += 1;
    }
}

/// A call of `std/grep` over a fresh in-memory filesystem.
fn new_call() -> TestCall {
    TestCall::new(ToolId::parse("std/grep").expect("a two-segment tool id parses"))
}

/// A call with one file at each of `files`, a path beside its text, which
/// makes every directory on the way to it.
fn seeded(files: &[(&str, &str)]) -> TestCall {
    let call = new_call();
    for (path, text) in files {
        call.context()
            .access()
            .write(path, text.as_bytes())
            .expect("a seeded file is written");
    }
    call
}

/// The `grep` definition the Pack builds the tool from.
fn definition() -> &'static Definition {
    spec::all()
        .expect("every embedded definition parses")
        .iter()
        .find(|definition| definition.segment == "grep")
        .expect("the grep definition exists")
}

/// Calls Grep with `args` as the Pack does: the schema check first, then
/// the tool.
fn grep(call: &TestCall, args: &Value) -> Result<ToolOutput, Error> {
    let definition = definition();
    crate::check::check(&definition.wire_name, &definition.parameters, args)?;
    block_on(super::call(call.context().access(), definition, args))
}

/// Calls Grep over `source` with `args`, under `limits`.
fn grep_over(source: &impl Source, args: &Value, limits: &Limits) -> Result<ToolOutput, Error> {
    let definition = definition();
    crate::check::check(&definition.wire_name, &definition.parameters, args)?;
    let request = args::parse(definition, args)?;
    block_on(super::search(source, "Grep", &request, limits))
}

/// Calls Grep over the call's filesystem under `limits`.
fn grep_limited(call: &TestCall, args: &Value, limits: &Limits) -> Result<ToolOutput, Error> {
    grep_over(call.context().access(), args, limits)
}

/// The text of a successful search, which must be untrusted output.
fn text_of(result: Result<ToolOutput, Error>) -> String {
    let output = result.expect("the search succeeds");
    assert_eq!(output.trust(), OutputTrust::Untrusted, "{}", output.text());
    output.text().to_owned()
}

/// The message of a failed search.
fn failure_of(result: Result<ToolOutput, Error>) -> String {
    match result {
        Ok(output) => panic!("the search fails, but it answered {:?}", output.text()),
        Err(error) => error.to_string(),
    }
}
