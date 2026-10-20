//! Tests for the preflight report's missing services and unavailable
//! Packs.

use runvil_types::packs::PackId;

use super::{MissingService, Requirements, UnavailablePack};

fn id(text: &str) -> PackId {
    PackId::parse(text).expect("a static valid id")
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
