//! The `std` Pack through the crate's `install`: the one Pack it inserts
//! with its config, the six tools `construct` names under the Pack id it
//! receives, and what a call answers.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use plugin_std::{NOT_IMPLEMENTED, install};
use runvil_plugin::testing::TestCall;
use runvil_plugin::{
    HostServices, InstallError, OutputTrust, Pack, PackId, PackRegistry, ToolError, ToolErrorKind,
    ToolId, ToolOutput,
};
use serde_json::{Value, json};

/// Drives a future that never pends to its output.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
}

/// The Pack `install` inserts into a fresh registry.
fn std_pack() -> runvil_plugin::PackInfo {
    let mut packs = PackRegistry::new();
    install(&mut packs, Value::Null).expect("a fresh registry takes the Pack");
    let (info, _id, _config) = packs
        .into_entries()
        .find(|(info, _, _)| info.fqn != "runvil/subagents")
        .expect("install inserts one Pack");
    info
}

/// Each Pack `install` inserts into a fresh registry with `config`, beside
/// the Pack id and config the registry holds it with.
fn inserted(config: Value) -> Vec<(String, String, Value)> {
    let mut packs = PackRegistry::new();
    install(&mut packs, config).expect("a fresh registry takes the Pack");
    packs
        .into_entries()
        .filter(|(info, _, _)| info.fqn != "runvil/subagents")
        .map(|(info, id, config)| (info.fqn.to_owned(), id.to_string(), config))
        .collect()
}

/// The Pack built under the Pack id `name` with `config`.
fn construct(name: &str, config: Value) -> Result<Arc<dyn Pack>, ToolError> {
    let name = PackId::parse(name).expect("a one-segment Pack id parses");
    (std_pack().construct)(&name, config, &HostServices::new())
}

/// The Pack built under `std`.
fn built() -> Arc<dyn Pack> {
    let Ok(pack) = construct("std", Value::Null) else {
        panic!("construct succeeds with no configuration");
    };
    pack
}

/// The tool id `segment` under the Pack id `std`.
fn id(segment: &str) -> ToolId {
    ToolId::parse(&format!("std/{segment}")).expect("a two-segment tool id parses")
}

/// Calls the tool `segment` with `args`.
fn call(segment: &str, args: Value) -> Result<ToolOutput, ToolError> {
    let pack = built();
    let call = TestCall::new(id(segment));
    block_on(pack.call(call.context(), args))
}

#[test]
fn the_pack_is_runvil_std_with_no_prelude_and_no_per_run_needs() {
    let info = std_pack();
    assert_eq!(info.fqn, "runvil/std");
    assert!(info.prelude.is_none());
    assert!(info.needs.is_empty());
    assert_eq!(info.description, "Read, write, search, and delete files");
}

#[test]
fn install_adds_the_std_pack_with_the_config_under_its_pack_id() {
    for (config, expected) in [
        (json!({ "std": {}, "web": { "x": 1 } }), json!({})),
        (json!({ "web": {} }), Value::Null),
        (Value::Null, Value::Null),
    ] {
        let read = inserted(config.clone());
        assert_eq!(
            read,
            [("runvil/std".to_owned(), "std".to_owned(), expected)],
            "{config}"
        );
    }
}

#[test]
fn install_returns_the_registrys_refusal() {
    let mut packs = PackRegistry::new();
    install(&mut packs, Value::Null).expect("the first install inserts std");
    assert_eq!(
        install(&mut packs, Value::Null),
        Err(InstallError::DuplicatePack { fqn: "runvil/std" })
    );
}

#[test]
fn construct_names_six_tools_under_the_pack_id_it_receives() {
    let Ok(pack) = construct("files", Value::Null) else {
        panic!("construct succeeds with no configuration");
    };
    let ids: Vec<String> = pack
        .tools()
        .iter()
        .map(|tool| tool.id.to_string())
        .collect();
    assert_eq!(
        ids,
        [
            "files/read",
            "files/write",
            "files/replace",
            "files/rm",
            "files/glob",
            "files/grep"
        ]
    );
}

#[test]
fn every_tool_carries_cursors_name_for_it_as_its_wire_name() {
    let pack = built();
    let names: Vec<(String, Option<String>)> = pack
        .tools()
        .iter()
        .map(|tool| (tool.id.to_string(), tool.wire_name.clone()))
        .collect();
    assert_eq!(
        names,
        [
            ("std/read".to_owned(), Some("Read".to_owned())),
            ("std/write".to_owned(), Some("Write".to_owned())),
            ("std/replace".to_owned(), Some("StrReplace".to_owned())),
            ("std/rm".to_owned(), Some("Delete".to_owned())),
            ("std/glob".to_owned(), Some("Glob".to_owned())),
            ("std/grep".to_owned(), Some("Grep".to_owned())),
        ]
    );
}

#[test]
fn no_tool_survives_a_stop_or_reports_structured_output() {
    for tool in built().tools() {
        assert!(!tool.survives_stop, "{}: {}", tool.id, tool.survives_stop);
        assert!(
            !tool.structured_output,
            "{}: {}",
            tool.id, tool.structured_output
        );
        assert!(!tool.describes_tools, "{}", tool.id);
    }
}

#[test]
fn construct_accepts_null_or_an_empty_object_and_refuses_any_other_configuration() {
    assert!(construct("std", json!({})).is_ok());
    for config in [json!({ "max_chars": 10 }), json!([]), json!("std")] {
        let Err(error) = construct("std", config.clone()) else {
            panic!("{config} is refused");
        };
        assert_eq!(error.to_string(), "std takes no configuration");
    }
}

#[test]
fn every_tool_answers_the_not_implemented_text_naming_itself() {
    for (segment, wire_name) in [
        ("read", "Read"),
        ("write", "Write"),
        ("replace", "StrReplace"),
        ("rm", "Delete"),
        ("glob", "Glob"),
        ("grep", "Grep"),
    ] {
        let args = match segment {
            "read" | "rm" => json!({ "path": "/notes.md" }),
            "write" => json!({ "path": "/notes.md", "contents": "hi" }),
            "replace" => json!({
                "path": "/notes.md",
                "old_string": "a",
                "new_string": "b"
            }),
            "glob" => json!({ "glob_pattern": "**/*.md" }),
            _ => json!({ "pattern": "TODO" }),
        };
        let output = call(segment, args).expect("a matching call succeeds");
        let expected = NOT_IMPLEMENTED.replace("<name>", wire_name);
        assert_eq!(output.text(), expected, "{segment}");
        assert_eq!(output.trust(), OutputTrust::Trusted, "{segment}");
    }
}

#[test]
fn the_not_implemented_text_says_the_result_never_changes() {
    let text = NOT_IMPLEMENTED.replace("<name>", "Read");
    assert!(
        text.contains("nothing in this session will change that"),
        "{text}"
    );
    assert!(text.contains("Calling Read again is safe"), "{text}");
    assert!(text.contains("this same result every time"), "{text}");
    assert!(!text.contains('<'), "the name is filled in: {text}");
}

#[test]
fn a_missing_required_parameter_fails_naming_it() {
    let error = call("grep", json!({})).expect_err("pattern is required");
    assert_eq!(
        error.to_string(),
        "std_Grep: `pattern` is required, and the call left it out"
    );
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
}

#[test]
fn a_parameter_of_the_wrong_type_fails_naming_it() {
    let error = call("read", json!({ "path": 7 })).expect_err("path is a string");
    assert_eq!(
        error.to_string(),
        "std_Read: `path` must be a JSON string, and the call passed 7"
    );
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
}

#[test]
fn a_value_outside_an_enum_fails_listing_the_allowed_values() {
    let error = call("grep", json!({ "pattern": "x", "output_mode": "lines" }))
        .expect_err("output_mode has three allowed values");
    assert_eq!(
        error.to_string(),
        "std_Grep: `output_mode` must be one of \"content\", \"files_with_matches\", \"count\", \
         and the call passed \"lines\""
    );
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
}

#[test]
fn a_call_of_a_tool_the_pack_lacks_fails_naming_it() {
    let pack = built();
    let unknown = TestCall::new(id("copy"));
    let error =
        block_on(pack.call(unknown.context(), json!({}))).expect_err("std has no copy tool");
    assert_eq!(error.to_string(), "std has no tool named copy");
}
