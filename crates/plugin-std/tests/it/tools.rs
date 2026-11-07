//! The `std` Pack through the crate's `install`: the one Pack it inserts
//! with its config, the seven tools `construct` names under the Pack id it
//! receives, and what a call answers.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use plugin_std::{NOT_IMPLEMENTED, install};
use runvil_plugin::testing::TestCall;
use runvil_plugin::{
    Error, ErrorCode, ErrorFact, HostServices, OutputTrust, Pack, PackId, PackRegistry, ToolId,
    ToolOutput,
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
fn construct(name: &str, config: Value) -> Result<Arc<dyn Pack>, Error> {
    let name = PackId::parse(name).expect("a one-segment Pack id parses");
    (std_pack().construct)(&name, config, &HostServices::new())
}

/// The Pack built under `std`.
pub(crate) fn built() -> Arc<dyn Pack> {
    let Ok(pack) = construct("std", Value::Null) else {
        panic!("construct succeeds with no configuration");
    };
    pack
}

/// The tool id `segment` under the Pack id `std`.
pub(crate) fn id(segment: &str) -> ToolId {
    ToolId::parse(&format!("std/{segment}")).expect("a two-segment tool id parses")
}

/// Calls the tool `segment` with `args`.
fn call(segment: &str, args: Value) -> Result<ToolOutput, Error> {
    call_seeded(segment, &[], args)
}

/// Calls the tool `segment` with `args` after writing each of `files`, a
/// path beside its bytes, through the call's own access.
fn call_seeded(segment: &str, files: &[(&str, &[u8])], args: Value) -> Result<ToolOutput, Error> {
    let pack = built();
    let call = TestCall::new(id(segment));
    for (path, bytes) in files {
        call.context()
            .access()
            .write(path, bytes)
            .expect("a seeded file is written");
    }
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
    let error = install(&mut packs, Value::Null).expect_err("a second install is refused");
    assert_eq!(error.code, ErrorCode::InvalidValue);
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        Some(&Value::from("duplicate_pack"))
    );
    assert_eq!(
        error.facts.get(&ErrorFact::Name),
        Some(&Value::from("runvil/std"))
    );
}

#[test]
fn construct_names_seven_tools_under_the_pack_id_it_receives() {
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
            "files/grep",
            "files/dir"
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
            ("std/dir".to_owned(), Some("ListDir".to_owned())),
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
        ("write", "Write"),
        ("replace", "StrReplace"),
        ("rm", "Delete"),
        ("glob", "Glob"),
    ] {
        let args = match segment {
            "rm" => json!({ "path": "/notes.md" }),
            "write" => json!({ "path": "/notes.md", "contents": "hi" }),
            "replace" => json!({
                "path": "/notes.md",
                "old_string": "a",
                "new_string": "b"
            }),
            _ => json!({ "glob_pattern": "**/*.md" }),
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
    assert_eq!(error.code, ErrorCode::ToolInvalidArguments);
}

#[test]
fn a_parameter_of_the_wrong_type_fails_naming_it() {
    let error = call("read", json!({ "path": 7 })).expect_err("path is a string");
    assert_eq!(
        error.to_string(),
        "std_Read: `path` must be a JSON string, and the call passed 7"
    );
    assert_eq!(error.code, ErrorCode::ToolInvalidArguments);
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
    assert_eq!(error.code, ErrorCode::ToolInvalidArguments);
}

#[test]
fn a_call_of_a_tool_the_pack_lacks_fails_naming_it() {
    let pack = built();
    let unknown = TestCall::new(id("copy"));
    let error =
        block_on(pack.call(unknown.context(), json!({}))).expect_err("std has no copy tool");
    assert_eq!(error.to_string(), "std has no tool named copy");
}

#[test]
fn read_answers_the_numbered_lines_of_a_seeded_file_as_untrusted_output() {
    let output = call_seeded(
        "read",
        &[("/notes.md", b"hello\nworld\n")],
        json!({ "path": "/notes.md" }),
    )
    .expect("a seeded file reads");
    assert_eq!(output.text(), "     1|hello\n     2|world\n");
    assert_eq!(output.trust(), OutputTrust::Untrusted);
}

#[test]
fn list_dir_answers_the_tree_of_a_seeded_directory_as_untrusted_output() {
    let output = call_seeded(
        "dir",
        &[
            ("/proj/README.md", b"hello"),
            ("/proj/src/lib.rs", b"fn main() {}"),
            ("/proj/.git/HEAD", b"ref"),
        ],
        json!({ "target_directory": "/proj" }),
    )
    .expect("a seeded directory lists");
    assert_eq!(
        output.text(),
        "/proj/\n  - README.md\n  - src/\n    - lib.rs"
    );
    assert_eq!(output.trust(), OutputTrust::Untrusted);
}

#[test]
fn list_dir_of_a_missing_path_fails_with_the_does_not_exist_text() {
    let error =
        call("dir", json!({ "target_directory": "/nope" })).expect_err("the directory is missing");
    assert_eq!(error.to_string(), "Path does not exist: /nope");
}

#[test]
fn list_dir_leaves_out_what_ignore_globs_match() {
    let output = call_seeded(
        "dir",
        &[("/proj/a.md", b"x"), ("/proj/b.log", b"y")],
        json!({ "target_directory": "/proj", "ignore_globs": ["*.log"] }),
    )
    .expect("a seeded directory lists");
    assert_eq!(output.text(), "/proj/\n  - a.md");
}

#[test]
fn read_answers_file_is_empty_for_a_zero_byte_file() {
    let output = call_seeded(
        "read",
        &[("/empty.md", b"")],
        json!({ "path": "/empty.md" }),
    )
    .expect("an empty file reads");
    assert_eq!(output.text(), "File is empty.");
    assert_eq!(output.trust(), OutputTrust::Untrusted);
}

#[test]
fn read_of_a_missing_path_fails_with_the_not_found_text() {
    let error = call("read", json!({ "path": "/nope.md" })).expect_err("the file is missing");
    assert_eq!(error.to_string(), "File not found: /nope.md");
}

#[test]
fn grep_answers_the_matching_lines_of_a_seeded_tree_as_untrusted_output() {
    let output = call_seeded(
        "grep",
        &[
            (
                "/src/a.ts",
                b"// one\n// two\nconst alpha = 1\nlet beta = 2\n// five\n// six\n// seven\n\
                  // eight\nconst gamma = 3\nexport default gamma\n",
            ),
            ("/src/b.ts", b"const first = 0\n"),
        ],
        json!({ "pattern": "const", "-A": 1 }),
    )
    .expect("a seeded tree searches");
    assert_eq!(
        output.text(),
        "/src/a.ts\n  3:const alpha = 1\n  4-let beta = 2\n  --\n  9:const gamma = 3\n  \
         10-export default gamma\n\n/src/b.ts\n  1:const first = 0"
    );
    assert_eq!(output.trust(), OutputTrust::Untrusted);
}

#[test]
fn grep_of_a_missing_path_fails_with_the_does_not_exist_text() {
    let error =
        call("grep", json!({ "pattern": "x", "path": "/nope" })).expect_err("the path is missing");
    assert_eq!(error.to_string(), "Path does not exist: /nope");
}

#[test]
fn grep_refuses_an_undeclared_key_by_name_while_the_stubs_let_it_through() {
    let error = call("grep", json!({ "pattern": "x", "context": 3 }))
        .expect_err("context is not a parameter");
    assert_eq!(error.code, ErrorCode::ToolInvalidArguments);
    assert!(
        error
            .to_string()
            .starts_with("std_Grep: `context` is not a parameter of this tool."),
        "{error}"
    );
    let stub = call("glob", json!({ "glob_pattern": "*.md", "extra": 1 }))
        .expect("a stub ignores an undeclared key");
    assert_eq!(stub.text(), NOT_IMPLEMENTED.replace("<name>", "Glob"));
}
