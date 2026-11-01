//! Unit tests for the `Read` tool. This file holds the helpers the topic
//! files share: a call over a fresh in-memory filesystem, a seeded file,
//! and the two ways to drive Read, through its arguments and through a
//! lowered byte cap.

mod arguments;
mod failures;
mod paging;
mod render;
mod window;

use runvil_plugin::testing::TestCall;
use runvil_plugin::{OutputTrust, ToolError, ToolId, ToolOutput};
use serde_json::Value;

use super::args::Request;
use crate::spec::{self, Definition};

/// A call of `std/read` over a fresh in-memory filesystem.
fn new_call() -> TestCall {
    TestCall::new(ToolId::parse("std/read").expect("a two-segment tool id parses"))
}

/// Writes `bytes` to `path` through the call's own access.
fn seed(call: &TestCall, path: &str, bytes: &[u8]) {
    call.context()
        .access()
        .write(path, bytes)
        .expect("a seeded file is written");
}

/// A call with `bytes` already at `path`.
fn seeded(path: &str, bytes: &[u8]) -> TestCall {
    let call = new_call();
    seed(&call, path, bytes);
    call
}

/// The `read` definition the Pack builds the tool from.
fn definition() -> &'static Definition {
    spec::all()
        .expect("every embedded definition parses")
        .iter()
        .find(|definition| definition.segment == "read")
        .expect("the read definition exists")
}

/// Calls Read with `args` as the Pack does: the schema check first, then
/// the tool.
fn read(call: &TestCall, args: &Value) -> Result<ToolOutput, ToolError> {
    let definition = definition();
    crate::check::check(&definition.wire_name, &definition.parameters, args)?;
    super::call(call.context().access(), definition, args)
}

/// Reads `path` with the window `offset` and `limit` under the byte cap
/// `cap`.
fn read_capped(
    call: &TestCall,
    path: &str,
    offset: Option<i64>,
    limit: Option<usize>,
    cap: usize,
) -> Result<ToolOutput, ToolError> {
    let request = Request {
        path,
        offset,
        limit,
    };
    super::read_file(call.context().access(), "Read", &request, cap)
}

/// The text of a successful read, which must be untrusted output.
fn text_of(result: Result<ToolOutput, ToolError>) -> String {
    let output = result.expect("the read succeeds");
    assert_eq!(output.trust(), OutputTrust::Untrusted, "{}", output.text());
    output.text().to_owned()
}

/// The message of a failed read.
fn failure_of(result: Result<ToolOutput, ToolError>) -> String {
    match result {
        Ok(output) => panic!("the read fails, but it answered {:?}", output.text()),
        Err(error) => error.to_string(),
    }
}

/// A file of `count` lines, `line 1` to `line {count}`, with or without a
/// final newline.
fn numbered_file(count: usize, final_newline: bool) -> String {
    let mut text = (1..=count)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    if final_newline && count > 0 {
        text.push('\n');
    }
    text
}
