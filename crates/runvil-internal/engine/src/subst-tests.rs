//! Regression coverage for `{{ }}` prose substitution: the resolution
//! of the `args`, `argv`, `item`, `var`, `sys`, and bare-global sources, the
//! escape grammar, path segment validation, and the failure reasons.

use std::collections::BTreeMap;

use super::*;
use serde_json::json;

/// The lookup for prose with no section VM behind it: every bare-global
/// name comes back unset.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the signature must match the globals-lookup callback type"
)]
fn no_globals(_: &str) -> Result<Option<Value>> {
    Ok(None)
}

/// Sources with the stock args string and no argv/item, over the given
/// `var`/`sys`; tests override fields with struct-update syntax.
fn test_sources<'a>(var: &'a Value, sys: &'a Value) -> Sources<'a> {
    Sources {
        args: "Acme Corp",
        argv: None,
        item: None,
        var,
        sys,
        globals: &no_globals,
    }
}

fn run(prose: &str) -> Result<String> {
    let var = json!({ "kind": "library", "count": 3, "row": { "a": 1 } });
    let sys = json!({ "when": "2026-07-29T00:00:00Z", "id": 1 });
    substitute(prose, &test_sources(&var, &sys))
}

fn err_of(prose: &str) -> Error {
    let var = json!({ "kind": "library", "row": { "a": 1 }, "arr": [1, 2] });
    let sys = json!({ "id": 1 });
    let sources = Sources {
        item: Some(&json!("i")),
        ..test_sources(&var, &sys)
    };
    substitute(prose, &sources).expect_err("expected substitution failure")
}

/// The `Reason` fact of a substitution failure.
fn reason(error: &Error) -> Option<&str> {
    error.facts.get(&FactKey::Reason)?.as_str()
}

#[test]
fn substitution_diagnostics_escape_and_bound_the_placeholder() {
    let hostile = format!("var.{}", "x".repeat(500));
    let preview = path_preview(&hostile);
    assert!(
        preview.chars().count() <= 83,
        "preview must be bounded, got {} chars",
        preview.chars().count()
    );
    assert!(preview.ends_with("..."), "over-long preview must be elided");

    let with_controls = path_preview("var.a\nb\tc");
    assert!(
        !with_controls.contains('\n') && !with_controls.contains('\t'),
        "control characters must be escaped, got: {with_controls}"
    );
    assert!(with_controls.contains("\\n") && with_controls.contains("\\t"));
}

#[test]
fn resolves_args() {
    assert_eq!(run("hi {{ args }}!").unwrap(), "hi Acme Corp!");
}

// --- argv: the parsed-args namespace ------------------------------------

#[test]
fn resolves_argv_whole_value() {
    let var = json!({});
    let sys = json!({});
    let argv = json!({ "query": "papers", "n": 2 });
    let sources = Sources {
        argv: Some(&argv),
        ..test_sources(&var, &sys)
    };
    let out = substitute("got {{ argv }}", &sources).unwrap();
    assert_eq!(out, "got {\"n\":2,\"query\":\"papers\"}");
}

#[test]
fn resolves_argv_dotted_path() {
    let var = json!({});
    let sys = json!({});
    let argv = json!({ "query": "papers", "row": { "a": 1 } });
    let sources = Sources {
        argv: Some(&argv),
        ..test_sources(&var, &sys)
    };
    let out = substitute("q={{ argv.query }} cell={{ argv.row.a }}", &sources).unwrap();
    assert_eq!(out, "q=papers cell=1");
}

#[test]
fn scalar_argv_renders_whole() {
    let var = json!({});
    let sys = json!({});
    let argv = json!(42);
    let sources = Sources {
        argv: Some(&argv),
        ..test_sources(&var, &sys)
    };
    let out = substitute("{{ argv }}", &sources).unwrap();
    assert_eq!(out, "42");
}

#[test]
fn nil_argv_is_an_error() {
    let var = json!({});
    let sys = json!({});
    for prose in ["{{ argv }}", "{{ argv.x }}"] {
        let e = substitute(prose, &test_sources(&var, &sys)).unwrap_err();
        assert_eq!(reason(&e), Some("nil_argv"), "path {prose:?}");
        assert!(e.to_string().contains("argv"), "names argv: {e}");
        assert!(
            e.to_string().contains("is nil (argv holds no value)"),
            "names the nil cause: {e}"
        );
    }
}

#[test]
fn dotted_index_into_a_scalar_argv_is_an_error() {
    // Never a silent empty string: the existing missing-key failure.
    let var = json!({});
    let sys = json!({});
    let argv = json!("scalar");
    let sources = Sources {
        argv: Some(&argv),
        ..test_sources(&var, &sys)
    };
    let e = substitute("{{ argv.x }}", &sources).unwrap_err();
    assert_eq!(reason(&e), Some("missing_key"));
}

#[test]
fn resolves_var_scalar() {
    assert_eq!(run("a {{ var.kind }} paper").unwrap(), "a library paper");
    assert_eq!(run("{{ var.count }}").unwrap(), "3");
}

#[test]
fn resolves_sys() {
    assert_eq!(run("id {{ sys.id }}").unwrap(), "id 1");
    assert_eq!(run("at {{ sys.when }}").unwrap(), "at 2026-07-29T00:00:00Z");
}

#[test]
fn table_renders_as_json() {
    assert_eq!(run("{{ var.row }}").unwrap(), "{\"a\":1}");
}

#[test]
fn missing_key_is_error() {
    assert!(run("{{ var.nope }}").is_err());
    assert!(run("{{ ghost.x }}").is_err());
    let sys_error = run("{{ sys.bogus }}").expect_err("unknown sys field must fail");
    assert!(
        sys_error.to_string().contains("missing {{ sys.bogus }}"),
        "error was {sys_error}"
    );
}

#[test]
fn no_placeholders_passthrough() {
    assert_eq!(run("plain text").unwrap(), "plain text");
}

#[test]
fn unclosed_is_error() {
    assert_eq!(reason(&err_of("open {{ args")), Some("unclosed"));
}

// --- Escape grammar ------------------------------------------------------

#[test]
fn escaped_delimiters_are_literal() {
    assert_eq!(
        run(r"literal \{{ args }} here").unwrap(),
        "literal {{ args }} here"
    );
    assert_eq!(run(r"close \}} brace").unwrap(), "close }} brace");
    assert_eq!(run(r"back \\ slash").unwrap(), r"back \ slash");
}

#[test]
fn escape_then_real_placeholder_adjacent() {
    // First delimiter escaped, second one live and resolved.
    assert_eq!(run(r"\{{x}}{{ args }}").unwrap(), "{{x}}Acme Corp");
}

#[test]
fn lone_backslash_is_literal() {
    assert_eq!(run(r"a\b").unwrap(), r"a\b");
    assert_eq!(run("trailing\\").unwrap(), "trailing\\");
}

#[test]
fn replacement_produced_delimiters_are_not_resubstituted() {
    let var = json!({ "payload": "{{ args }}" });
    let sys = json!({});
    // `var.payload` renders text that looks like a placeholder; it must be
    // emitted verbatim, never resolved against args.
    let sources = Sources {
        args: "SECRET",
        ..test_sources(&var, &sys)
    };
    let out = substitute("value: {{ var.payload }}", &sources).unwrap();
    assert_eq!(out, "value: {{ args }}");
}

// --- Path segment grammar ------------------------------------------------

#[test]
fn empty_or_padded_segments_are_rejected() {
    for bad in ["var.", "var..x", "var. .x", "var.x.", "var. .x .y"] {
        let prose = format!("{{{{ {bad} }}}}");
        let e = err_of(&prose);
        assert_eq!(
            reason(&e),
            Some("empty_segment"),
            "path {bad:?} must be an empty-segment error, got {e:?}"
        );
    }
}

#[test]
fn valid_nested_segment_still_resolves() {
    assert_eq!(run("{{ var.row.a }}").unwrap(), "1");
}

// --- Failure reason/offset/cause -----------------------------------------

#[test]
fn error_has_reason_and_offset() {
    let e = err_of("prefix {{ ghost.x }}");
    assert_eq!(reason(&e), Some("unknown_namespace"));
    assert!(
        e.message.ends_with("[UnknownNamespace at byte 7]"),
        "offset must point at the '{{{{': {e}"
    );
    assert!(e.message.contains("ghost.x"));
}

#[test]
fn a_failure_is_a_substitution_error_with_its_reason_fact() {
    let e = err_of("see {{ var.nope }}");
    let detail = "missing {{ var.nope }}";
    assert_eq!(e.code, ErrorCode::Substitution);
    assert_eq!(e.message, format!("{detail} [MissingKey at byte 4]"));
    assert_eq!(
        e.prose,
        format!("{detail}. {}", ErrorCode::Substitution.prose())
    );
    assert_eq!(
        e.facts,
        BTreeMap::from([(FactKey::Reason, Value::from("missing_key"))])
    );
    assert_eq!(e.at, None);
}

#[test]
fn a_failure_classifies_as_the_authors_program_failing_and_lua_reads_it_as_lua() {
    let error = run("{{ var.nope }}").expect_err("a missing key fails the substitution");
    assert_eq!(error.code, ErrorCode::Substitution);
    assert_eq!(reason(&error), Some("missing_key"));
    assert_eq!(
        crate::execute::run_error_kind(error.code),
        crate::RunErrorKind::Lua
    );
    assert_eq!(runvil_lua::error_kind(&error), runvil_lua::ErrorKind::Lua);
    assert_eq!(runvil_lua::error_fields(&error), Vec::new());
}

// --- bare globals (section-local Lua globals in prose) -------------------

#[test]
fn bare_global_resolves_a_scalar() {
    let var = json!({});
    let sys = json!({});
    let globals = |name: &str| Ok((name == "answer").then(|| json!(42)));
    let sources = Sources {
        globals: &globals,
        ..test_sources(&var, &sys)
    };
    let out = substitute("the answer is {{ answer }}", &sources).unwrap();
    assert_eq!(out, "the answer is 42");
}

#[test]
fn bare_global_dotted_path_indexes_the_json() {
    let var = json!({});
    let sys = json!({});
    let globals = |name: &str| Ok((name == "row").then(|| json!({ "a": { "b": 2 } })));
    let sources = Sources {
        globals: &globals,
        ..test_sources(&var, &sys)
    };
    let out = substitute("cell {{ row.a.b }}", &sources).unwrap();
    assert_eq!(out, "cell 2");
}

#[test]
fn bare_global_table_renders_as_json() {
    let var = json!({});
    let sys = json!({});
    let globals = |name: &str| Ok((name == "row").then(|| json!({ "a": 1 })));
    let sources = Sources {
        globals: &globals,
        ..test_sources(&var, &sys)
    };
    let out = substitute("{{ row }}", &sources).unwrap();
    assert_eq!(out, "{\"a\":1}");
}

#[test]
fn missing_bare_global_is_unknown_namespace() {
    let e = err_of("{{ ghost }}");
    assert_eq!(reason(&e), Some("unknown_namespace"));
    assert!(e.to_string().contains("global 'ghost'"));
}

#[test]
fn non_json_bare_global_is_an_error() {
    let var = json!({});
    let sys = json!({});
    let globals = |name: &str| {
        assert_eq!(name, "f");
        Err(crate::error::lua_runtime("global `f` is a function"))
    };
    let sources = Sources {
        globals: &globals,
        ..test_sources(&var, &sys)
    };
    let e = substitute("{{ f }}", &sources).unwrap_err();
    assert_eq!(reason(&e), Some("serialize"));
    assert_eq!(
        e.message,
        "global 'f' in {{ f }} is not JSON data [Serialize at byte 0]: global `f` is a function",
        "the lookup failure must be preserved in the message"
    );
    assert!(
        !e.prose.contains("is a function"),
        "the lookup failure must stay out of the prose: {e:?}"
    );
}

#[test]
fn bare_namespaces_still_require_a_key() {
    // `{{ var }}` and `{{ sys }}` stay `bad_path`; only bare globals render
    // whole values.
    for prose in ["{{ var }}", "{{ sys }}"] {
        let e = err_of(prose);
        assert_eq!(reason(&e), Some("bad_path"), "path {prose:?}");
    }
}

#[test]
fn reply_is_no_longer_a_namespace() {
    // `{{ reply }}` resolves as a bare global like any other name, and is
    // unset here.
    let e = err_of("{{ reply }}");
    assert_eq!(reason(&e), Some("unknown_namespace"));
}

#[test]
fn null_value_and_nil_item_reasons() {
    let var = json!({ "n": Value::Null });
    let sys = json!({});
    let e = substitute("{{ var.n }}", &test_sources(&var, &sys)).unwrap_err();
    assert_eq!(reason(&e), Some("null_value"));

    let e = substitute("{{ item }}", &test_sources(&var, &sys)).unwrap_err();
    assert_eq!(reason(&e), Some("nil_item"));
}

#[test]
fn not_a_table_reason() {
    let e = err_of("{{ args.x }}");
    assert_eq!(reason(&e), Some("not_a_table"));
    assert!(e.to_string().contains("not a table"));
}

// --- Null, arrays, trust-neutral passthrough -----------------------------

#[test]
fn array_renders_as_json() {
    let var = json!({ "arr": [1, 2, 3] });
    let sys = json!({});
    let out = substitute("{{ var.arr }}", &test_sources(&var, &sys)).unwrap();
    assert_eq!(out, "[1,2,3]");
}

#[test]
fn resolves_item_when_present() {
    let var = json!({});
    let sys = json!({});
    let sources = Sources {
        item: Some(&json!("the angle")),
        ..test_sources(&var, &sys)
    };
    let out = substitute("topic: {{ item }}", &sources).unwrap();
    assert_eq!(out, "topic: the angle");
}

#[test]
fn item_nil_is_error() {
    let var = json!({});
    let sys = json!({});
    let err = substitute("{{ item }}", &test_sources(&var, &sys)).expect_err("nil item must fail");
    assert!(
        err.to_string()
            .contains("{{ item }} is nil (no item is seeded into this section)"),
        "error must say no item is seeded: {err}"
    );
}

#[test]
fn item_dot_path_is_error() {
    let var = json!({});
    let sys = json!({});
    let sources = Sources {
        item: Some(&json!("text")),
        ..test_sources(&var, &sys)
    };
    let err = substitute("{{ item.x }}", &sources).expect_err("item takes no key path");
    assert!(
        err.to_string()
            .contains("item takes no key path; {{ item }} renders the whole item"),
        "error must say item renders whole: {err}"
    );
}
