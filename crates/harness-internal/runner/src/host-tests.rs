//! Tests for `Host::parse`: a prompt found by its name parses and names
//! itself by that name, a finder or parse failure comes back as given, a
//! `uses:` entry naming a Pack not installed or inserted unavailable is
//! refused naming the Pack and its reason, and one naming a Pack whose
//! factory refuses parses, since parsing makes no Pack.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use runvil::{Error, ErrorCode, Prompt, make_tool_error};
use runvil_plugin::{
    Pack, PackFuture, PackId, PackInfo, PackRegistry, ToolContext, ToolInfo, ToolOutput,
};
use serde_json::Value;

use super::Host;
use crate::packs::Packs;
use crate::performers::{BoxFuture, Timer};

/// A Pack with no tools that answers no call.
struct Idle;

impl Pack for Idle {
    fn tools(&self) -> Vec<ToolInfo> {
        Vec::new()
    }

    fn call<'a>(
        &'a self,
        _cx: ToolContext<'a>,
        _args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async {
            Err(make_tool_error(
                ErrorCode::ToolOther,
                "idle answers no call",
            ))
        })
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "the PackFactory signature fixes the return type"
)]
fn builds(_id: &PackId) -> Result<Arc<dyn Pack>, Error> {
    Ok(Arc::new(Idle))
}

fn fails(_id: &PackId) -> Result<Arc<dyn Pack>, Error> {
    Err(make_tool_error(
        ErrorCode::ToolOther,
        "the broken backend is down",
    ))
}

/// A timer whose every sleep ends at once.
struct Instant;

impl Timer for Instant {
    fn sleep(&self, _duration: Duration) -> BoxFuture<()> {
        Box::pin(async {})
    }
}

/// A Host holding `acme/kit`, whose factory makes it, `acme/broken`,
/// whose factory refuses, and `acme/offline`, inserted unavailable, that
/// finds each prompt in `sources` by its name.
fn host(sources: &[(&str, String)]) -> Host {
    let mut registry = PackRegistry::new();
    registry.insert(PackInfo::new("acme/kit"), builds).unwrap();
    registry
        .insert(PackInfo::new("acme/broken"), fails)
        .unwrap();
    registry
        .insert_unavailable(
            PackInfo::new("acme/offline"),
            make_tool_error(ErrorCode::ToolOther, "offline takes no configuration"),
        )
        .unwrap();
    let sources: BTreeMap<String, String> = sources
        .iter()
        .map(|(name, source)| ((*name).to_owned(), source.clone()))
        .collect();
    Host {
        packs: Packs::new(registry),
        prompts: Arc::new(move |name: &str| {
            sources
                .get(name)
                .cloned()
                .ok_or_else(|| missing_prompt(name))
        }),
        timer: Arc::new(Instant),
    }
}

/// The finder's failure for a prompt it has no source for.
fn missing_prompt(name: &str) -> Error {
    let code = ErrorCode::NotFound;
    Error::new(code, format!("no prompt named {name}"), code.prose())
}

/// A prompt declaring the given `uses:` lines.
fn declaring(uses: &str) -> String {
    format!(
        "---\nname: research\ndescription: d\nrunvil: 0\n{uses}---\n\n\
         # Research\n\n## Only\n\nDone.\n"
    )
}

#[test]
fn a_prompt_whose_declared_packs_are_all_installed_parses_under_its_name() {
    let host = host(&[("research.md", declaring("uses:\n  - acme/kit\n"))]);
    let prompt = host
        .parse("research.md")
        .expect("every declared Pack is installed");
    assert_eq!(prompt.file(), "research.md");
    assert_eq!(prompt.title(), "Research");
}

#[test]
fn a_uses_entry_naming_an_uninstalled_pack_is_refused_naming_it() {
    let host = host(&[(
        "research.md",
        declaring("uses:\n  - acme/kit\n  - acme/gone\n"),
    )]);
    let error = host
        .parse("research.md")
        .expect_err("acme/gone is not installed");
    assert_eq!(error.code, ErrorCode::RequirementsUnmet, "{error:?}");
    assert_eq!(
        error.message,
        "the environment cannot satisfy this prompt:\n\
         - gone is unavailable: acme/gone is not installed"
    );
}

#[test]
fn a_uses_entry_naming_a_pack_whose_factory_refuses_parses_since_parsing_makes_no_pack() {
    let host = host(&[("research.md", declaring("uses:\n  - acme/broken\n"))]);
    let prompt = host
        .parse("research.md")
        .expect("only a run calls acme/broken's factory");
    assert_eq!(prompt.file(), "research.md");
}

#[test]
fn a_uses_entry_naming_a_pack_inserted_unavailable_is_refused_with_its_reason() {
    let host = host(&[("research.md", declaring("uses:\n  - acme/offline\n"))]);
    let error = host
        .parse("research.md")
        .expect_err("acme/offline was inserted unavailable");
    assert_eq!(error.code, ErrorCode::RequirementsUnmet, "{error:?}");
    assert_eq!(
        error.message,
        "the environment cannot satisfy this prompt:\n\
         - offline is unavailable: offline takes no configuration"
    );
}

#[test]
fn a_finder_failure_comes_back_as_given() {
    let host = host(&[]);
    let error = host
        .parse("absent.md")
        .expect_err("the finder has no such prompt");
    assert_eq!(error, missing_prompt("absent.md"));
}

#[test]
fn a_parse_failure_comes_back_as_given_before_any_uses_check() {
    let source = declaring("uses:\n  - acme/gone\n").replace("# Research\n", "");
    let expected = Prompt::parse(&|_: &str| Ok(source.clone()), "research.md")
        .0
        .expect_err("a prompt with no H1 fails to parse");
    let host = host(&[("research.md", source)]);
    let error = host
        .parse("research.md")
        .expect_err("the parse fails first");
    assert_eq!(error, expected);
}
