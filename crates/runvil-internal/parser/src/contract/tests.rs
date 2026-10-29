//! Tests for the frontmatter contract keys: `uses`, `inputSchema`, and
//! `models`.

use std::num::NonZeroU32;

use serde_json::json;

use super::{ModelKeyword, ModelRolesIter};
use crate::ParseErrorKind;
use crate::test_support::{assert_parse_error, parse_err, parse_ok, prompt};

#[path = "tests-input-schema.rs"]
mod input_schema;
#[path = "tests-packs.rs"]
mod packs;

#[test]
fn the_full_contract_declaration_parses_and_round_trips() {
    let prompt = parse_ok(&prompt(
        "
        uses: [web, mcp]
        inputSchema: { type: object, properties: { use_mcp: { type: boolean, default: true, \
                description: Search MCP-connected private sources } } }
        models: { analyst: { keywords: [frontier, thinking], min_context: 200000, \
                  description: deep reasoning }, \
                  triage: { keywords: [fast, small], description: quick triage of search results } }
        ",
        &[],
    ));

    let fm = prompt.frontmatter();

    let packs = fm.uses();
    assert_eq!(packs.len(), 2);
    assert_eq!(packs[0].id().to_string(), "web");
    assert_eq!(packs[1].id().to_string(), "mcp");

    let schema = fm.input_schema().expect("the schema is declared");
    assert_eq!(
        schema["properties"]["use_mcp"],
        json!({
            "type": "boolean",
            "default": true,
            "description": "Search MCP-connected private sources",
        })
    );

    let analyst = fm.models().get("analyst").expect("the role is declared");
    assert_eq!(
        analyst.keywords(),
        &[ModelKeyword::Frontier, ModelKeyword::Thinking]
    );
    assert_eq!(analyst.min_context(), NonZeroU32::new(200_000));
    assert_eq!(analyst.description(), Some("deep reasoning"));
    let triage = fm.models().get("triage").expect("the role is declared");
    assert_eq!(
        triage.keywords(),
        &[ModelKeyword::Fast, ModelKeyword::Small]
    );
    assert_eq!(triage.min_context(), None);
}

#[test]
fn a_prompt_without_contract_keys_behaves_exactly_as_today() {
    let prompt = parse_ok(&prompt("", &[]));
    let fm = prompt.frontmatter();
    assert!(fm.uses().is_empty());
    assert!(fm.models().is_empty());
}

#[test]
fn an_unknown_key_inside_a_contract_entry_is_rejected() {
    // `deny_unknown_fields` must hold inside each new key's entries too: a
    // typo'd field is an authoring error, not silently ignored.
    for front in [
        "argz: { flag: { type: boolean } }",
        "models: { analyst: { keyword: [fast] } }",
    ] {
        let error = parse_err(&prompt(front, &[]));
        assert_eq!(error.kind(), ParseErrorKind::Frontmatter, "{error}");
    }
}

#[test]
fn every_reserved_name_the_grammar_allows_parses_as_a_model_role_label() {
    for (name, _) in runvil_lua::RESERVED_NAMES {
        if name.starts_with('_') {
            continue;
        }
        let prompt = parse_ok(&prompt(&format!("models: {{ '{name}': {{}} }}"), &[]));
        assert!(prompt.frontmatter().models().get(name).is_some(), "{name}");
    }
}

#[test]
fn model_roles_round_trip_with_defaults() {
    let prompt = parse_ok(&prompt(
        "models: { analyst: { keywords: [no-thinking, creative, chat], min_context: 32000, \
         description: deep reasoning }, spare: {} }",
        &[],
    ));
    let roles = prompt.frontmatter().models();
    assert_eq!(roles.len(), 2);
    let analyst = roles.get("analyst").expect("declared");
    assert_eq!(
        analyst.keywords(),
        &[
            ModelKeyword::NoThinking,
            ModelKeyword::Creative,
            ModelKeyword::Chat
        ]
    );
    assert_eq!(analyst.min_context(), NonZeroU32::new(32_000));
    let spare = roles.get("spare").expect("declared");
    assert!(spare.keywords().is_empty());
    assert_eq!(spare.min_context(), None);
    assert_eq!(spare.description(), None);
}

#[test]
fn model_roles_iterate_in_name_order_through_a_named_iterator() {
    let prompt = parse_ok(&prompt("models: { triage: {}, analyst: {} }", &[]));
    let fm = prompt.frontmatter();

    let roles: ModelRolesIter<'_> = fm.models().iter();
    assert_eq!(roles.len(), 2);
    let labels: Vec<&str> = roles.map(|(label, _)| label).collect();
    assert_eq!(labels, ["analyst", "triage"]);
    let by_reference: Vec<&str> = fm.models().into_iter().map(|(label, _)| label).collect();
    assert_eq!(by_reference, labels);
}

#[test]
fn an_unknown_model_keyword_is_a_parse_error() {
    // The keyword vocabulary is closed; `multimodal` is a non-goal and a
    // typo must fail at parse rather than being silently ignored.
    let error = parse_err(&prompt(
        "models: { analyst: { keywords: [multimodal] } }",
        &[],
    ));
    assert_eq!(error.kind(), ParseErrorKind::Frontmatter);
}

#[test]
fn model_role_labels_must_match_the_alias_grammar() {
    let error = parse_err(&prompt("models: { '1analyst': { keywords: [fast] } }", &[]));
    assert_eq!(error.kind(), ParseErrorKind::Frontmatter);
}

#[test]
fn a_zero_min_context_is_rejected() {
    let error = parse_err(&prompt("models: { analyst: { min_context: 0 } }", &[]));
    assert_eq!(error.kind(), ParseErrorKind::Frontmatter);
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
    assert_eq!(error.kind(), ParseErrorKind::Frontmatter);
    assert_eq!(
        error.line(),
        NonZeroU32::new(5),
        "the offending scalar's line: {error}"
    );
    assert_eq!(
        error.column(),
        NonZeroU32::new(5),
        "the offending scalar's column: {error}"
    );
    assert_eq!(
        error.name(),
        None,
        "a frontmatter failure predates the prompt's name"
    );
}
