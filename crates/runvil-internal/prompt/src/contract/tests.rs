//! Tests for the frontmatter contract keys: `uses`, `args`, and
//! `models`.

use std::num::NonZeroU32;

use runvil_types::error::{ErrorCode, ErrorFact};
use runvil_types::models::ModelRole;

use super::{ArgType, ArgsDeclIter};
use crate::test_support::{assert_parse_error, parse_err, parse_ok, prompt};
use crate::tests::location;

#[path = "tests-packs.rs"]
mod packs;

#[test]
fn the_full_contract_declaration_parses_and_round_trips() {
    let prompt = parse_ok(&prompt(
        "
        uses: [web, mcp]
        args: { use_mcp: { type: boolean, default: true, \
                description: Search MCP-connected private sources } }
        models: { strong: { min_context: 200000, thinking: true }, \
                  fast: { thinking: false } }
        ",
        &[],
    ));

    let fm = prompt.frontmatter();

    let packs = fm.uses();
    assert_eq!(packs.len(), 2);
    assert_eq!(packs[0].id().to_string(), "web");
    assert_eq!(packs[1].id().to_string(), "mcp");

    let arg = fm.args().get("use_mcp").expect("the arg is declared");
    assert_eq!(arg.kind(), ArgType::Boolean);
    assert!(!arg.is_optional());
    assert_eq!(arg.default(), Some(&serde_yaml_ng::Value::Bool(true)));
    assert_eq!(
        arg.description(),
        Some("Search MCP-connected private sources")
    );

    let strong = fm
        .role_requirements(ModelRole::Strong)
        .expect("the role is declared");
    assert_eq!(strong.min_context(), NonZeroU32::new(200_000));
    assert_eq!(strong.thinking(), Some(true));
    let fast = fm
        .role_requirements(ModelRole::Fast)
        .expect("the role is declared");
    assert_eq!(fast.min_context(), None);
    assert_eq!(fast.thinking(), Some(false));
}

#[test]
fn a_prompt_without_contract_keys_behaves_exactly_as_today() {
    let prompt = parse_ok(&prompt("", &[]));
    let fm = prompt.frontmatter();
    assert!(fm.uses().is_empty());
    for role in ModelRole::ALL {
        assert!(
            fm.role_requirements(role).is_none(),
            "a prompt with no models key declares nothing for {role:?}"
        );
    }
}

#[test]
fn an_omitted_args_key_yields_the_default_prose_declaration() {
    // There are no freeform prompts: an absent `args:` key is the default
    // declaration of one optional string field named `prose`.
    let prompt = parse_ok(&prompt("", &[]));
    let args = prompt.frontmatter().args();
    assert_eq!(args.len(), 1);
    let prose = args
        .get("prose")
        .expect("the default declaration names `prose`");
    assert_eq!(prose.kind(), ArgType::String);
    assert!(prose.is_optional());
    assert_eq!(prose.default(), None);
    assert_eq!(prose.description(), Some("Freeform input for this prompt"));
}

#[test]
fn an_unknown_key_inside_a_contract_entry_is_rejected() {
    // `deny_unknown_fields` must hold inside each new key's entries too: a
    // typo'd field is an authoring error, not silently ignored.
    for front in [
        "args: { flag: { tipe: boolean } }",
        "models: { strong: { tier: 1 } }",
    ] {
        let error = parse_err(&prompt(front, &[]));
        assert_eq!(error.code, ErrorCode::FrontmatterInvalid, "{error}");
    }
}

#[test]
fn an_arg_name_may_be_a_reserved_name_because_args_are_argv_fields() {
    let front =
        "args: { prose: { type: string }, store: { type: string }, end: { type: boolean } }";
    let prompt = parse_ok(&prompt(front, &[]));
    let args = prompt.frontmatter().args();
    assert_eq!(args.len(), 3);
    assert!(args.get("store").is_some() && args.get("end").is_some());
}

#[test]
fn args_declarations_round_trip() {
    let prompt = parse_ok(&prompt(
        "args: { use_mcp: { type: boolean, default: true, description: Search private sources }, \
         limit: { type: integer, optional: true }, query: { type: string } }",
        &[],
    ));
    let args = prompt.frontmatter().args();
    assert_eq!(args.len(), 3);
    let limit = args.get("limit").expect("declared");
    assert_eq!(limit.kind(), ArgType::Integer);
    assert!(limit.is_optional());
    assert_eq!(limit.description(), None);
    assert_eq!(args.get("query").expect("declared").kind(), ArgType::String);
}

#[test]
fn an_arg_default_must_match_the_declared_type() {
    for (kind, default, actual) in [
        ("boolean", "'true'", "string"),
        ("integer", "1.5", "number"),
        ("string", "42", "integer"),
        ("number", "[1]", "sequence"),
    ] {
        let front = format!("args: {{ flag: {{ type: {kind}, default: {default} }} }}");
        assert_parse_error(
            &prompt(&front, &[]),
            ErrorCode::FrontmatterInvalid,
            &format!(
                "the default has type `{actual}`, which does not match the declared type `{kind}`"
            ),
        );
    }
    // A matching default passes.
    let prompt = parse_ok(&prompt(
        "args: { limit: { type: integer, default: 3 } }",
        &[],
    ));
    assert_eq!(
        prompt
            .frontmatter()
            .args()
            .get("limit")
            .and_then(|a| a.default().cloned()),
        Some(serde_yaml_ng::Value::Number(3.into()))
    );
}

#[test]
fn an_arg_name_must_match_the_alias_grammar() {
    let error = parse_err(&prompt("args: { '1bad': { type: string } }", &[]));
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid);
}

#[test]
fn an_absent_args_key_marks_the_default_declaration() {
    // No `args:` key: the implicit default declaration, whose interface
    // prose wraps into `argv.prose`.
    assert!(
        parse_ok(&prompt("", &[])).frontmatter().args().is_default(),
        "an absent args key yields the default declaration"
    );

    // An explicit `args:` key is a structured declaration, which never
    // wraps.
    let explicit = parse_ok(&prompt("args: { query: { type: string } }", &[]));
    assert!(!explicit.frontmatter().args().is_default());

    // Even an explicit declaration identical to the default shape is
    // structured: declared is not defaulted.
    let shaped = parse_ok(&prompt(
        "args: { prose: { type: string, optional: true, \
         description: Freeform input for this prompt } }",
        &[],
    ));
    let args = shaped.frontmatter().args();
    assert!(!args.is_default());
    assert_ne!(
        *args,
        super::ArgsDecl::default(),
        "an explicit declaration never equals the implicit default"
    );
}

#[test]
fn an_unknown_arg_type_is_rejected() {
    let error = parse_err(&prompt("args: { flag: { type: text } }", &[]));
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid);
}

#[test]
fn each_role_declared_with_both_requirements_reads_them_back() {
    for role in ModelRole::ALL {
        let front = format!(
            "models: {{ {}: {{ min_context: 4096, thinking: true }} }}",
            role.as_str()
        );
        let prompt = parse_ok(&prompt(&front, &[]));
        let declared = prompt
            .frontmatter()
            .role_requirements(role)
            .expect("the role is declared");
        assert_eq!(declared.min_context(), NonZeroU32::new(4096), "{role:?}");
        assert_eq!(declared.thinking(), Some(true), "{role:?}");
    }
}

#[test]
fn an_empty_role_declares_neither_requirement() {
    let prompt = parse_ok(&prompt("models: { fast: {} }", &[]));
    let fast = prompt
        .frontmatter()
        .role_requirements(ModelRole::Fast)
        .expect("an empty entry still declares the role");
    assert_eq!(fast.min_context(), None);
    assert_eq!(fast.thinking(), None);
    assert!(
        prompt
            .frontmatter()
            .role_requirements(ModelRole::Strong)
            .is_none(),
        "a role the prompt does not name has no entry"
    );
}

#[test]
fn args_iterate_in_name_order_through_a_named_iterator() {
    let prompt = parse_ok(&prompt(
        "args: { query: { type: string }, limit: { type: integer } }",
        &[],
    ));
    let fm = prompt.frontmatter();

    let args: ArgsDeclIter<'_> = fm.args().iter();
    assert_eq!(args.len(), 2);
    let names: Vec<&str> = args.map(|(name, _)| name).collect();
    assert_eq!(names, ["limit", "query"]);
    let mut kinds = Vec::new();
    for (_, decl) in fm.args() {
        kinds.push(decl.kind());
    }
    assert_eq!(kinds, [ArgType::Integer, ArgType::String]);
}

#[test]
fn an_unknown_role_name_is_a_parse_error_that_lists_the_five_roles() {
    assert_parse_error(
        &prompt("models: { reviewer: {} }", &[]),
        ErrorCode::FrontmatterInvalid,
        "expected one of `default`, `strong`, `small`, `fast`, `creative`",
    );
}

#[test]
fn an_unknown_field_in_a_role_is_a_parse_error_that_names_the_two_fields() {
    assert_parse_error(
        &prompt("models: { strong: { tier: 1 } }", &[]),
        ErrorCode::FrontmatterInvalid,
        "unknown field `tier`, expected `min_context` or `thinking`",
    );
}

#[test]
fn a_duplicate_role_key_is_rejected() {
    let error = parse_err(&prompt(
        "models: { strong: { min_context: 1 }, strong: { thinking: true } }",
        &[],
    ));
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid, "{error}");
}

#[test]
fn a_zero_min_context_is_rejected() {
    let error = parse_err(&prompt("models: { strong: { min_context: 0 } }", &[]));
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid);
}

#[test]
fn contract_errors_report_their_frontmatter_line_and_column() {
    // The retained serde_yaml_ng location surfaces on the parse
    // error, so a rejection inside a contract key points at its own line
    // and column instead of being a bare message.
    let src = concat!(
        "---\n",               // line 1
        "name: x\n",           // line 2
        "description: d\n",    // line 3
        "uses:\n",             // line 4
        "  - not a Pack id\n", // line 5: the offending scalar
        "---\n",
        "\n# T\n\n## S\n\np\n",
    );
    let error = parse_err(src);
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid);
    assert_eq!(
        location(&error),
        Some(("prompt", 5, 5, 5, 5)),
        "the offending scalar's line and column: {error}"
    );
    assert_eq!(
        error.facts.get(&ErrorFact::Name),
        None,
        "a frontmatter failure predates the prompt's name"
    );
}
