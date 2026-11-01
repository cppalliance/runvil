//! A run's start: real-file claims, the caller's tool catalog, and model
//! roles, whose requirements the run's first step checks. The store-mount
//! isolation case and the start-check refusals that reach engine-only
//! items sit in `runvil-engine`'s own suite.

use runvil::event::Event;
use runvil::model::{ModelRole, ThinkingMode};
use runvil::tools::{ToolCatalog, ToolInfo, ToolOutput};
use runvil::vfs::{MemoryBackend, Origin, RealBackend, VfsError, VfsRef};
use runvil::{RunErrorKind, RunOutcome};

use super::support::{Scenario, TempRoot, TestResult, Tool, Trace, model, prompt, text, tool_info};

fn declaring(front: &str) -> String {
    prompt(front, &[("Only", "return 'done'")])
}

/// The refusal notice of `scenario`'s run of `source`, after checking that
/// its first step failed it as `RequirementsUnmet`, reporting nothing
/// after the parse events and issuing no effect.
pub(super) fn refusal(scenario: Scenario, source: &str) -> TestResult<String> {
    let trace = scenario.run(source)?;
    let RunOutcome::Failed { kind, message } = &trace.outcome else {
        return Err(format!("expected a refusal, got {:?}", trace.outcome).into());
    };
    assert_eq!(*kind, RunErrorKind::RequirementsUnmet, "{message}");
    assert!(
        matches!(trace.events.last(), Some(Event::ParseSucceeded { .. })),
        "a refused run reports no run events: {:?}",
        trace.events
    );
    assert!(trace.effects.is_empty(), "a refused run issues nothing");
    Ok(message.clone())
}

/// The refusal notice of a run of `declaring(front)` under one model of
/// the given context window and thinking capability.
fn refusal_under(front: &str, context: u32, thinking: ThinkingMode) -> TestResult<String> {
    let scenario = Scenario::default().model(model("current", context, thinking)?);
    refusal(scenario, &declaring(front))
}

#[test]
fn two_runs_writing_the_same_real_file_through_the_shared_base_conflict() -> TestResult {
    let temp =
        TempRoot::new("two_runs_writing_the_same_real_file_through_the_shared_base_conflict");
    let vfs = VfsRef::builder()
        .mount("/", RealBackend::rooted(temp.path())?)
        .store("/my/store", MemoryBackend::new())
        .build();
    let (run_a, run_b) = (vfs.clone(), vfs);
    let access_a = run_a.acquire(Origin::new("run-a"))?;
    access_a.write("/shared.txt", b"from a")?;
    let access_b = run_b.acquire(Origin::new("run-b"))?;
    let error = access_b
        .write("/shared.txt", b"from b")
        .expect_err("run b conflicts with run a's live claim");
    assert!(
        matches!(error, VfsError::Conflict { .. }),
        "a determinism violation, not a backend error: {error}"
    );
    assert_eq!(std::fs::read(temp.path().join("shared.txt"))?, b"from a");
    Ok(())
}

const STRONG: &str = "models: { strong: { thinking: true, min_context: 200000 } }";

/// The five roles a prompt can pick with `models.use`.
const ROLES: [&str; 5] = ["default", "strong", "small", "fast", "creative"];

/// A prompt that asks each of the five roles one question and returns the
/// answers joined by commas.
fn asking_every_role(front: &str) -> String {
    let lua = format!(
        "local out = {{}}\nfor _, role in ipairs({{'{}'}}) do\nmodels.use(role)\nout[#out + 1] = models.infer(role)\nend\nreturn table.concat(out, ',')",
        ROLES.join("','")
    );
    prompt(front, &[("Only", &lua)])
}

/// The models the replies of `trace` name, in order.
fn replying_models(trace: &Trace) -> Vec<&str> {
    trace
        .events
        .iter()
        .filter_map(|event| match event {
            Event::AssistantReply { model, .. } => Some(model.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn one_model_fills_every_role_and_an_empty_declaration_adds_no_requirement() -> TestResult {
    // A role declared with no requirement checks nothing.
    let source = asking_every_role("models: { fast: {}, creative: {} }");
    let model = model("current", 32_000, ThinkingMode::Never)?;
    let replies = ["a", "b", "c", "d", "e"].map(text);
    let trace = Scenario::replies(replies).model(model).run(&source)?;
    assert_eq!(trace.text()?, "a,b,c,d,e");
    assert_eq!(replying_models(&trace), ["current"; 5]);
    Ok(())
}

#[test]
fn a_strong_entry_fills_strong_and_the_default_fills_every_other_role() -> TestResult {
    let source = asking_every_role("");
    let (default, strong) = (
        model("default-model", 32_000, ThinkingMode::Switchable)?,
        model("strong-model", 200_000, ThinkingMode::Switchable)?,
    );
    let replies = ["a", "b", "c", "d", "e"].map(text);
    let trace = Scenario::replies(replies)
        .roles([(ModelRole::Default, default), (ModelRole::Strong, strong)])
        .run(&source)?;
    assert_eq!(trace.text()?, "a,b,c,d,e");
    assert_eq!(
        replying_models(&trace),
        [
            "default-model",
            "strong-model",
            "default-model",
            "default-model",
            "default-model"
        ],
        "the rounds ran in the order of {ROLES:?}"
    );
    Ok(())
}

#[test]
fn a_role_map_with_models_but_no_default_is_refused_naming_the_default_role() -> TestResult {
    let strong = model("strong-model", 200_000, ThinkingMode::Switchable)?;
    let scenario = Scenario::default().roles([(ModelRole::Strong, strong)]);
    assert_eq!(
        refusal(scenario, &declaring(""))?,
        "the environment cannot satisfy this prompt:\n\
         - role 'default': requires a model; the caller's role map has no 'default' entry"
    );
    Ok(())
}

#[test]
fn a_context_minimum_above_the_filling_models_is_refused() -> TestResult {
    // The model can think, so only the context check fails.
    assert_eq!(
        refusal_under(STRONG, 32_000, ThinkingMode::Always)?,
        "the environment cannot satisfy this prompt:\n\
         - role 'strong': requires a context of at least 200000 tokens; \
         the model filling it provides 32000"
    );
    Ok(())
}

#[test]
fn a_thinking_requirement_the_filling_model_fails_is_refused() -> TestResult {
    // The context minimum is met, so only the thinking check fires.
    assert_eq!(
        refusal_under(STRONG, 200_000, ThinkingMode::Never)?,
        "the environment cannot satisfy this prompt:\n\
         - role 'strong': requires 'thinking: true'; \
         the thinking capability of the model filling it is Never"
    );
    let small = "models: { small: { thinking: false } }";
    assert_eq!(
        refusal_under(small, 32_000, ThinkingMode::Always)?,
        "the environment cannot satisfy this prompt:\n\
         - role 'small': requires 'thinking: false'; \
         the thinking capability of the model filling it is Always"
    );
    Ok(())
}

/// `scenario` with its catalog replaced by one that holds `info` alone.
fn over_catalog(scenario: Scenario, info: &ToolInfo) -> TestResult<Scenario> {
    let catalog = ToolCatalog::new([info.clone()])?;
    Ok(scenario.config(move |ctx| ctx.with_tools(catalog.clone())))
}

#[test]
fn a_run_calls_its_tools_as_the_caller_supplied_catalog_describes_them() -> TestResult {
    // Only the caller's entry marks the output structured, so the script
    // reads a table field only when the run uses that entry.
    let info = tool_info(
        "web/fetch",
        serde_json::json!({"type": "object", "properties": {"url": {"type": "string"}}}),
    )?
    .structured(true);
    let fetch = Tool {
        id: "web/fetch",
        handler: Box::new(|_| Ok(ToolOutput::trusted(r#"{"title":"Ada"}"#))),
    };
    let scenario = over_catalog(Scenario::tool(fetch, []), &info)?;
    let lua = "return tools.call('web/fetch', { url = 'https://example.com' }).title";
    let source = prompt("uses: [web]", &[("Only", lua)]);
    assert_eq!(scenario.run(&source)?.text()?, "Ada");
    Ok(())
}

#[test]
fn a_run_given_no_roster_checks_no_declared_pack_against_the_catalog() -> TestResult {
    let other = tool_info(
        "other/search",
        serde_json::json!({"type": "object", "properties": {}}),
    )?;
    let scenario = over_catalog(Scenario::default(), &other)?;
    assert_eq!(
        scenario.run(&declaring("uses: [web]"))?.text()?,
        "done",
        "without a roster, the run checks model roles and reserved ids only"
    );
    Ok(())
}
