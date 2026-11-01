//! Unit tests for the `ListDir` tool. This file holds the helpers the
//! topic files share: a call over a fresh in-memory filesystem, a seeded
//! tree, and the two ways to drive ListDir, through its arguments and
//! through a lowered byte cap.

mod arguments;
mod cap;
mod ignore;
mod tree;

use runvil_plugin::testing::TestCall;
use runvil_plugin::{OutputTrust, ToolError, ToolId, ToolOutput};
use serde_json::Value;

use super::args::Request;
use crate::matcher::Matcher;
use crate::spec::{self, Definition};

/// A call of `std/dir` over a fresh in-memory filesystem.
fn new_call() -> TestCall {
    TestCall::new(ToolId::parse("std/dir").expect("a two-segment tool id parses"))
}

/// A call with one small file at each of `files`, which makes every
/// directory on the way to it.
fn seeded(files: &[&str]) -> TestCall {
    let call = new_call();
    for path in files {
        call.context()
            .access()
            .write(path, b"x")
            .expect("a seeded file is written");
    }
    call
}

/// The `dir` definition the Pack builds the tool from.
fn definition() -> &'static Definition {
    spec::all()
        .expect("every embedded definition parses")
        .iter()
        .find(|definition| definition.segment == "dir")
        .expect("the dir definition exists")
}

/// Calls ListDir with `args` as the Pack does: the schema check first,
/// then the tool.
fn list(call: &TestCall, args: &Value) -> Result<ToolOutput, ToolError> {
    let definition = definition();
    crate::check::check(&definition.wire_name, &definition.parameters, args)?;
    super::call(call.context().access(), definition, args)
}

/// Lists `target`, leaving out `ignore`, under the byte cap `cap`.
fn list_capped(
    call: &TestCall,
    target: &str,
    ignore: &[&str],
    cap: usize,
) -> Result<ToolOutput, ToolError> {
    let request = Request {
        target,
        ignore: ignore
            .iter()
            .map(|pattern| Matcher::new(pattern).expect("the pattern is valid"))
            .collect(),
    };
    super::list_directory(call.context().access(), "ListDir", &request, cap)
}

/// The text of a successful listing, which must be untrusted output.
fn text_of(result: Result<ToolOutput, ToolError>) -> String {
    let output = result.expect("the listing succeeds");
    assert_eq!(output.trust(), OutputTrust::Untrusted, "{}", output.text());
    output.text().to_owned()
}

/// The message of a failed listing.
fn failure_of(result: Result<ToolOutput, ToolError>) -> String {
    match result {
        Ok(output) => panic!("the listing fails, but it answered {:?}", output.text()),
        Err(error) => error.to_string(),
    }
}
