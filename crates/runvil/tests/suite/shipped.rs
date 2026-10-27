//! The shipped research prompt runs end to end over its web tools, and the
//! prose the model reads names no tool by its id.

use std::fs;
use std::path::Path;

use runvil::effect::{EffectRecord, ToolCaller};
use runvil::tools::ToolCatalog;
use serde_json::{Value, json};

use super::support::{Scenario, TestResult, Tool, Trace, calls, text, tool_info};

/// Runs the shipped research prompt for "Ada Lovelace" over a catalog
/// holding `web/fetch` and `web/search`, with a model that searches once
/// and then answers.
fn research_ada_lovelace() -> TestResult<Trace> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prompts/research-person.md");
    let query = json!({ "type": "object", "properties": { "query": { "type": "string" } } });
    let catalog = ToolCatalog::new([
        tool_info("web/fetch", query.clone())?,
        tool_info("web/search", query)?,
    ])?;
    let search = calls("web_search", json!({ "query": "Ada Lovelace" }))?;
    Scenario::tool(
        Tool::untrusted("web/search", "search results"),
        [search, text("a factual summary")],
    )
    .args("Ada Lovelace")
    .context(move |ctx| ctx.with_tools(catalog.clone()))
    .run(&fs::read_to_string(path)?)
}

#[test]
fn the_shipped_research_prompt_offers_its_web_tools_by_id_and_runs_end_to_end() -> TestResult {
    let trace = research_ada_lovelace()?;
    assert_eq!(trace.text()?, "a factual summary");

    let (mut advertised, mut tool_calls) = (Vec::new(), Vec::new());
    for logged in &trace.effects {
        match &logged.effect {
            EffectRecord::Chat { tools, .. } => advertised.push(tools.clone()),
            EffectRecord::ToolCall {
                tool,
                alias,
                origin,
                ..
            } => tool_calls.push((tool.to_string(), alias.clone(), origin.caller)),
            _ => {}
        }
    }
    let wire_names = vec!["web_search".to_owned(), "web_fetch".to_owned()];
    assert_eq!(
        advertised,
        [wire_names.clone(), wire_names],
        "both rounds advertise the offered tools under their wire names, in offer order"
    );
    assert_eq!(
        tool_calls,
        [(
            "web/search".to_owned(),
            "web_search".to_owned(),
            ToolCaller::Model
        )],
        "the model's call by wire name reaches the tool its id names"
    );
    Ok(())
}

#[test]
fn the_shipped_research_prompt_names_no_tool_by_its_id_in_what_the_model_reads() -> TestResult {
    let trace = research_ada_lovelace()?;
    let mut checked = 0;
    for logged in &trace.effects {
        let EffectRecord::Chat { messages, .. } = &logged.effect else {
            continue;
        };
        let user = messages
            .iter()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"));
        for message in user {
            let wire = serde_json::to_string(message)?;
            for id in ["web/search", "web/fetch"] {
                assert!(
                    !wire.contains(id),
                    "the model only knows tools by wire name, but a user message names the id `{id}`: {wire}"
                );
            }
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "the run sends the model at least one user message"
    );
    Ok(())
}
