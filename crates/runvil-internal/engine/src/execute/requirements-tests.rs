//! Tests for the preflight report's missing services, unavailable
//! Packs, and the reserved Pack ids `adhoc` and `dynamic`.

use runvil_types::packs::PackId;
use runvil_types::timestamp::Timestamp;
use runvil_types::tools::{ToolCatalog, ToolId, ToolInfo};
use serde_json::json;

use super::{MissingService, Requirements, UnavailablePack, UsablePack};
use crate::execute::{RunContext, RunErrorKind};
use crate::parser::Prompt;
use crate::test_support::scenario::TEST_SEED;

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

/// A prompt that declares no Pack and no model.
fn declares_nothing() -> Prompt {
    Prompt::parse(
        "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n# Run\n\n## Only\n\nDone.\n",
        "requirements-test",
    )
    .0
    .expect("the requirements test prompt parses")
}

fn context() -> RunContext {
    RunContext::new("requirements-test", TEST_SEED, Timestamp::UNIX_EPOCH)
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

#[test]
fn a_catalog_or_roster_naming_a_reserved_pack_id_refuses_the_run_naming_the_id_once() {
    for reserved in ["adhoc", "dynamic"] {
        let (grab, put) = (format!("{reserved}/grab"), format!("{reserved}/put"));
        let contexts = [
            (
                "the catalog",
                context().tools(catalog(&["web/fetch", &grab, &put])),
            ),
            (
                "the usable roster",
                context().packs(vec![UsablePack::new(id(reserved), "acme")], Vec::new()),
            ),
            (
                "the unavailable roster",
                context().packs(
                    Vec::new(),
                    vec![UnavailablePack::new(id(reserved), "the server is down")],
                ),
            ),
        ];
        for (named_by, ctx) in contexts {
            let (_, requirements) = ctx.prepare(&declares_nothing());
            assert_eq!(
                requirements.reserved,
                [id(reserved)],
                "{named_by} names {reserved}"
            );
            assert!(!requirements.is_satisfied(), "{named_by} names {reserved}");
            let refusal = requirements
                .refusal()
                .expect("a reserved Pack id refuses the run");
            assert_eq!(
                refusal.kind(),
                RunErrorKind::RequirementsUnmet,
                "{named_by}"
            );
            let notice = refusal.to_string();
            assert_eq!(
                notice.matches(&reserved_line(reserved)).count(),
                1,
                "{named_by}: the notice names {reserved} once: {notice}"
            );
        }
    }
}

#[test]
fn a_context_naming_both_reserved_pack_ids_lists_them_sorted_each_once() {
    let ctx = context()
        .tools(catalog(&[
            "dynamic/search",
            "web/fetch",
            "adhoc/grab",
            "dynamic/call",
        ]))
        .packs(
            vec![UsablePack::new(id("dynamic"), "acme")],
            vec![UnavailablePack::new(id("adhoc"), "the server is down")],
        );
    let (_, requirements) = ctx.prepare(&declares_nothing());
    assert_eq!(requirements.reserved, [id("adhoc"), id("dynamic")]);
    assert_eq!(
        requirements.notice(),
        format!(
            "the environment cannot satisfy this prompt:\n{}\n{}",
            reserved_line("adhoc"),
            reserved_line("dynamic")
        )
    );
}

#[test]
fn a_context_naming_no_reserved_pack_id_leaves_the_reservations_empty() {
    let ctx = context()
        .tools(catalog(&["web/fetch", "dynamics/search", "adhoc2/grab"]))
        .packs(
            vec![UsablePack::new(id("web"), "runvil")],
            vec![UnavailablePack::new(id("mcp"), "the server is down")],
        );
    let (_, requirements) = ctx.prepare(&declares_nothing());
    assert!(requirements.reserved.is_empty(), "{requirements:?}");
    assert!(requirements.is_satisfied(), "{requirements:?}");
}

#[test]
fn merge_keeps_the_union_of_both_reports_reserved_ids_sorted_each_once() {
    let reserving = |ids: &[&str]| Requirements {
        reserved: ids.iter().copied().map(id).collect(),
        ..Requirements::default()
    };
    let mut kept = reserving(&["dynamic"]);
    kept.merge(Requirements::default());
    assert_eq!(
        kept.reserved,
        [id("dynamic")],
        "merging a clear report keeps the ids"
    );
    let mut gained = Requirements::default();
    gained.merge(reserving(&["dynamic"]));
    assert_eq!(
        gained.reserved,
        [id("dynamic")],
        "merging a reserving report takes its ids"
    );
    let mut union = reserving(&["dynamic"]);
    union.merge(reserving(&["dynamic", "adhoc"]));
    assert_eq!(union.reserved, [id("adhoc"), id("dynamic")]);
    assert_eq!(
        union.notice(),
        format!(
            "the environment cannot satisfy this prompt:\n{}\n{}",
            reserved_line("adhoc"),
            reserved_line("dynamic")
        )
    );
}

fn web_down() -> UnavailablePack {
    UnavailablePack::new(id("web"), "the search provider is missing")
}

#[test]
fn an_unavailable_pack_leaves_the_report_unsatisfied() {
    let mut unavailable = Requirements::default();
    unavailable.unavailable.push(web_down());
    assert!(!unavailable.is_satisfied());
    assert!(unavailable.refusal().is_some());
}

#[test]
fn the_notice_names_an_unavailable_packs_reason() {
    let mut requirements = Requirements::default();
    requirements.unavailable.push(web_down());
    assert_eq!(
        requirements.notice(),
        "the environment cannot satisfy this prompt:\n\
         - web is unavailable: the search provider is missing"
    );
}

#[test]
fn merge_drops_a_missing_pack_named_as_unavailable() {
    let mut requirements = Requirements::default();
    requirements.missing_required.push(id("web"));
    requirements.missing_required.push(id("other"));
    let mut activation = Requirements::default();
    activation.unavailable.push(web_down());
    requirements.merge(activation);
    assert_eq!(requirements.missing_required, [id("other")]);
    assert_eq!(requirements.unavailable, [web_down()]);
}

#[test]
fn merge_folds_in_unavailable_packs_without_repeating_one() {
    let mut requirements = Requirements::default();
    requirements.unavailable.push(web_down());
    let mut other = Requirements::default();
    other.unavailable.push(web_down());
    other
        .unavailable
        .push(UnavailablePack::new(id("mcp"), "the server is down"));
    requirements.merge(other);
    assert_eq!(
        requirements.unavailable,
        [
            web_down(),
            UnavailablePack::new(id("mcp"), "the server is down")
        ]
    );
}

fn missing_input(pack: &str) -> MissingService {
    MissingService::new(id(pack), "an input broker")
}

#[test]
fn a_missing_service_leaves_the_report_unsatisfied() {
    let mut requirements = Requirements::default();
    assert!(requirements.is_satisfied());
    requirements
        .missing_services
        .push(missing_input("user-input"));
    assert!(!requirements.is_satisfied());
    assert!(requirements.refusal().is_some());
}

#[test]
fn merge_folds_in_missing_services_without_repeating_one() {
    let mut requirements = Requirements::default();
    requirements
        .missing_services
        .push(missing_input("user-input"));
    let mut activation = Requirements::default();
    activation
        .missing_services
        .push(missing_input("user-input"));
    activation.missing_services.push(missing_input("asker"));
    requirements.merge(activation);
    assert_eq!(
        requirements.missing_services,
        [missing_input("user-input"), missing_input("asker")]
    );
}

#[test]
fn merge_drops_an_incoming_missing_pack_already_named_as_lacking_a_service() {
    let mut requirements = Requirements::default();
    requirements
        .missing_services
        .push(missing_input("user-input"));
    let mut other = Requirements::default();
    other.missing_required.push(id("user-input"));
    other.missing_required.push(id("other"));
    requirements.merge(other);
    assert_eq!(requirements.missing_required, [id("other")]);
    assert_eq!(requirements.missing_services, [missing_input("user-input")]);
}

#[test]
fn merge_drops_a_missing_pack_the_incoming_report_names_as_lacking_a_service() {
    let mut requirements = Requirements::default();
    requirements.missing_required.push(id("user-input"));
    requirements.missing_required.push(id("other"));
    let mut activation = Requirements::default();
    activation
        .missing_services
        .push(missing_input("user-input"));
    requirements.merge(activation);
    assert_eq!(requirements.missing_required, [id("other")]);
    assert_eq!(requirements.missing_services, [missing_input("user-input")]);
}

#[test]
fn the_notice_names_the_pack_and_the_service_it_lacks() {
    let mut requirements = Requirements::default();
    requirements
        .missing_services
        .push(missing_input("user-input"));
    assert_eq!(
        requirements.notice(),
        "the environment cannot satisfy this prompt:\n\
         - user-input needs an input broker, and the environment provides none"
    );
}

#[test]
fn the_notice_lists_missing_services_after_missing_packs() {
    let mut requirements = Requirements::default();
    requirements
        .missing_services
        .push(missing_input("user-input"));
    requirements.missing_required.push(id("web"));
    assert_eq!(
        requirements.notice(),
        "the environment cannot satisfy this prompt:\n\
         - missing required Pack: web\n\
         - user-input needs an input broker, and the environment provides none"
    );
}
