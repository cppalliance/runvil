//! Tests for the run's start checks against its config: each declared
//! Pack against the roster, the reserved Pack ids `adhoc` and `dynamic`,
//! and the order of the refusal notice's lines. The model role checks sit
//! in the execution suite's `prepare` module.

use std::collections::BTreeMap;
use std::sync::Arc;

use runvil_types::models::{ModelRole, ThinkingMode};
use runvil_types::packs::PackId;
use runvil_types::timestamp::Timestamp;
use runvil_types::tools::{ToolCatalog, ToolId, ToolInfo};
use serde_json::json;

use crate::execute::{Run, RunConfig, RunErrorKind, RunOutcome, Step};
use crate::lua::RosterEntry;
use crate::prompt::Prompt;
use crate::test_support::scenario::{TEST_SEED, test_model_with};

/// The notice line of a run whose catalog or roster names the reserved
/// Pack id `reserved`.
fn reserved_line(reserved: &str) -> String {
    format!(
        "- the run's tool catalog or Pack roster names the Pack id {reserved}, \
         which the Engine reserves"
    )
}

fn id(text: &str) -> PackId {
    PackId::parse(text).expect("a static valid id")
}

/// A prompt with `front` added to its frontmatter, whose one section
/// returns `done`.
fn prompt(front: &str) -> Prompt {
    let source = format!(
        "---\nname: t\ndescription: d\nrunvil: 0\n{front}---\n\n\
         # Run\n\n## Only\n\n```lua\nreturn 'done'\n```\n"
    );
    Prompt::parse(&source, "requirements-test")
        .0
        .expect("the requirements test prompt parses")
}

fn config() -> RunConfig {
    RunConfig::new("requirements-test", TEST_SEED, Timestamp::UNIX_EPOCH)
}

fn catalog(ids: &[&str]) -> ToolCatalog {
    ToolCatalog::new(ids.iter().map(|tool| {
        ToolInfo::new(
            ToolId::parse(tool).expect("a static valid tool id"),
            format!("{tool} tool"),
            json!({ "type": "object" }),
        )
    }))
    .expect("distinct ids make a valid catalog")
}

/// The roster entry of `pack`, usable from `acme`.
fn usable(pack: &str) -> (PackId, RosterEntry) {
    (
        id(pack),
        RosterEntry::Usable {
            vendor: "acme".to_owned(),
        },
    )
}

/// The roster entry of `pack`, unavailable for `reason`.
fn unavailable(pack: &str, reason: &str) -> (PackId, RosterEntry) {
    (
        id(pack),
        RosterEntry::Unavailable {
            reason: reason.to_owned(),
        },
    )
}

/// The refusal notice the first step of a run of `prompt` over `ctx`
/// fails with, after asserting the step reported no run events, or
/// `None` when the run passes its start checks and completes.
#[track_caller]
fn refusal(prompt: Prompt, ctx: RunConfig) -> Option<String> {
    let mut run = Run::new(Arc::new(prompt), "", ctx);
    let Step::Done { outcome, events } = run.step() else {
        panic!("the fixture run is done at its first step");
    };
    match outcome {
        RunOutcome::Completed { final_text } => {
            assert_eq!(final_text, "done");
            None
        }
        RunOutcome::Failed { kind, message } => {
            assert_eq!(kind, RunErrorKind::RequirementsUnmet, "{message}");
            assert!(
                events.is_empty(),
                "a refused run reports no run events: {events:?}"
            );
            Some(message)
        }
        RunOutcome::Cancelled => panic!("expected a completion or a refusal, got a cancellation"),
    }
}

#[test]
fn a_catalog_or_roster_naming_a_reserved_pack_id_refuses_the_run_naming_the_id_once() {
    for reserved in ["adhoc", "dynamic"] {
        let (grab, put) = (format!("{reserved}/grab"), format!("{reserved}/put"));
        let configs = [
            (
                "the catalog",
                config().with_tools(catalog(&["web/fetch", &grab, &put])),
            ),
            ("the usable roster", config().with_packs([usable(reserved)])),
            (
                "the unavailable roster",
                config().with_packs([unavailable(reserved, "the server is down")]),
            ),
        ];
        for (named_by, ctx) in configs {
            assert_eq!(
                refusal(prompt(""), ctx),
                Some(format!(
                    "the environment cannot satisfy this prompt:\n{}",
                    reserved_line(reserved)
                )),
                "{named_by} names {reserved}"
            );
        }
    }
}

#[test]
fn a_config_naming_both_reserved_pack_ids_lists_them_sorted_each_once() {
    // Two catalog tools and the roster all name `dynamic`.
    let ctx = config()
        .with_tools(catalog(&[
            "dynamic/search",
            "web/fetch",
            "adhoc/grab",
            "dynamic/call",
        ]))
        .with_packs([
            usable("dynamic"),
            unavailable("adhoc", "the server is down"),
        ]);
    assert_eq!(
        refusal(prompt(""), ctx),
        Some(format!(
            "the environment cannot satisfy this prompt:\n{}\n{}",
            reserved_line("adhoc"),
            reserved_line("dynamic")
        ))
    );
}

#[test]
fn a_config_naming_no_reserved_pack_id_passes_the_start_checks() {
    let ctx = config()
        .with_tools(catalog(&["web/fetch", "dynamics/search", "adhoc2/grab"]))
        .with_packs([usable("web"), unavailable("mcp", "the server is down")]);
    assert_eq!(refusal(prompt(""), ctx), None);
}

#[test]
fn a_declared_pack_the_roster_lists_as_unavailable_is_refused_with_its_reason_not_as_missing() {
    let ctx = config().with_packs([unavailable("web", "the search provider is missing")]);
    assert_eq!(
        refusal(prompt("uses: [web]\n"), ctx),
        Some(
            "the environment cannot satisfy this prompt:\n\
             - web is unavailable: the search provider is missing"
                .to_owned()
        )
    );
}

#[test]
fn declared_packs_the_roster_lacks_are_missing_and_list_before_the_unavailable_ones() {
    let ctx = config().with_packs([
        unavailable("web", "the search provider is missing"),
        unavailable("mcp", "the server is down"),
        usable("kit"),
    ]);
    assert_eq!(
        refusal(prompt("uses: [web, other, kit, mcp, gone]\n"), ctx),
        Some(
            "the environment cannot satisfy this prompt:\n\
             - missing required Pack: other\n\
             - missing required Pack: gone\n\
             - web is unavailable: the search provider is missing\n\
             - mcp is unavailable: the server is down"
                .to_owned()
        ),
        "each kind of gap lists in declaration order"
    );
}

#[test]
fn a_run_given_no_roster_checks_no_declared_pack_and_an_empty_roster_lacks_every_one() {
    assert_eq!(
        refusal(prompt("uses: [web]\n"), config()),
        None,
        "a run given no roster checks no declared Pack"
    );
    assert_eq!(
        refusal(prompt("uses: [web]\n"), config().with_packs([])),
        Some(
            "the environment cannot satisfy this prompt:\n- missing required Pack: web".to_owned()
        ),
        "an empty roster is a roster"
    );
}

#[test]
fn the_notice_lists_missing_then_unavailable_packs_then_reserved_ids_then_model_lines() {
    let ctx = config()
        .with_tools(catalog(&["adhoc/grab"]))
        .with_packs([unavailable("kit", "the kit server is down")])
        .with_models(BTreeMap::from([(
            ModelRole::Default,
            test_model_with("small", 32_000, ThinkingMode::Switchable),
        )]));
    let reserved = reserved_line("adhoc");
    let expected = [
        "the environment cannot satisfy this prompt:",
        "- missing required Pack: web",
        "- kit is unavailable: the kit server is down",
        &reserved,
        "- role 'strong': requires a context of at least 200000 tokens; \
         the model filling it provides 32000",
    ];
    let source = "uses: [kit, web]\nmodels: { strong: { min_context: 200000 } }\n";
    assert_eq!(refusal(prompt(source), ctx), Some(expected.join("\n")));
}
