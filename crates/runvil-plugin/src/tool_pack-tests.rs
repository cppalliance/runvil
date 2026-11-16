//! Typed tools validate at their boundary and retain schema and trust semantics.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use runvil_types::tools::{ToolCallOrigin, ToolCaller};
use runvil_vfs::{Origin, VfsRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::ToolPack;
use crate::{Error, ErrorCode, OutputTrust, Pack, PackId, ToolContext, ToolId, ToolOutput};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Args {
    /// Text to echo.
    text: String,
    note: Option<String>,
}

fn pack() -> ToolPack {
    ToolPack::new(PackId::parse("demo").unwrap())
}

fn call(pack: &ToolPack, name: &str, args: Value) -> Result<ToolOutput, Error> {
    let id = ToolId::parse(name).unwrap();
    let access = VfsRef::default()
        .acquire(Origin::new("typed test"))
        .unwrap();
    let origin = ToolCallOrigin::new("Test", ToolCaller::Script);
    let mut future = pin!(pack.call(ToolContext::new(&id, &access, &origin), args));
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("this handler completes in one poll"),
    }
}

#[test]
fn schemas_preserve_optional_nullability_and_field_docs() {
    let mut pack = pack();
    pack.tool(
        "echo",
        "Echoes",
        OutputTrust::Trusted,
        |args: Args| async move { Ok((args.text, args.note)) },
    )
    .unwrap();
    let tools = pack.tools();
    let schema = &tools[0].parameters_schema;
    assert_eq!(schema["properties"]["text"]["description"], "Text to echo.");
    assert_eq!(
        schema["properties"]["note"]["type"],
        json!(["string", "null"])
    );
    assert_eq!(schema["required"], json!(["text"]));
    assert_eq!(schema["additionalProperties"], false);
    assert!(tools[0].structured_output);
    assert_eq!(
        call(&pack, "demo/echo", json!({"text":"yes"}))
            .unwrap()
            .text(),
        "[\"yes\",null]"
    );
    assert_eq!(
        call(&pack, "demo/echo", json!({"text":"yes","note":null}))
            .unwrap()
            .text(),
        "[\"yes\",null]"
    );
}

#[test]
fn invalid_names_and_duplicate_tools_do_not_replace_existing_handlers() {
    let mut pack = pack();
    pack.tool(
        "echo",
        "Original",
        OutputTrust::Trusted,
        |args: Args| async move { Ok(args.text) },
    )
    .unwrap();
    let duplicate = pack
        .tool(
            "echo",
            "Replacement",
            OutputTrust::Trusted,
            |_: Args| async { Ok("wrong") },
        )
        .unwrap_err();
    assert_eq!(duplicate.code, ErrorCode::InvalidValue);
    assert!(duplicate.message.contains("demo/echo"));
    assert_eq!(
        call(&pack, "demo/echo", json!({"text":"original"}))
            .unwrap()
            .text(),
        "\"original\""
    );
    assert!(
        pack.tool("/bad", "Bad", OutputTrust::Trusted, |_: Args| async {
            Ok(())
        })
        .is_err()
    );
    assert!(
        pack.tool("scalar", "Bad", OutputTrust::Trusted, |_: String| async {
            Ok(())
        })
        .is_err()
    );
    assert_eq!(pack.tools().len(), 1);
}

#[test]
fn malformed_arguments_are_tool_errors_and_output_keeps_its_trust() {
    let mut pack = pack();
    pack.tool(
        "echo",
        "Echoes",
        OutputTrust::Untrusted,
        |args: Args| async move { Ok(args.text) },
    )
    .unwrap();
    for bad in [
        json!([]),
        json!({}),
        json!({"text":2}),
        json!({"text":"ok","extra":true}),
    ] {
        assert_eq!(
            call(&pack, "demo/echo", bad).unwrap_err().code,
            ErrorCode::ToolInvalidArguments
        );
    }
    let output = call(&pack, "demo/echo", json!({"text":"external"})).unwrap();
    assert_eq!(output.trust(), OutputTrust::Untrusted);
    assert!(!pack.tools()[0].structured_output);
    assert_eq!(
        call(&pack, "demo/absent", json!({})).unwrap_err().code,
        ErrorCode::ToolOther
    );
}

struct CannotSerialize;

impl Serialize for CannotSerialize {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("output refused"))
    }
}

#[test]
fn output_serialization_failures_are_reported_as_tool_errors() {
    let mut pack = pack();
    pack.tool("fail", "Fails", OutputTrust::Trusted, |_: Args| async {
        Ok(CannotSerialize)
    })
    .unwrap();
    assert_eq!(
        call(&pack, "demo/fail", json!({"text":"ok"}))
            .unwrap_err()
            .code,
        ErrorCode::ToolOther
    );
}
