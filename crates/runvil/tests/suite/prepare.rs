//! The prepare pass: real-file claims, the caller's tool catalog, and model
//! roles. The store-mount isolation case and the prepare-run refusals
//! drive the run fixture in `runvil-engine`'s test support, so they
//! sit in that crate's own suite.

use runvil::event::Event;
use runvil::model::{ModelRole, ThinkingMode};
use runvil::tools::{ToolCatalog, ToolInfo};
use runvil::vfs::{MemoryBackend, Origin, RealBackend, VfsError, VfsRef};
use runvil::{RequirementCheck, Requirements, RunErrorKind, UnmetRequirement};

use super::support::{
    Prepared, Scenario, TempRoot, TestResult, Trace, greeter, model, prompt, text, tool_info,
};

fn declaring(front: &str) -> String {
    prompt(front, &[("Only", "return 'done'")])
}

fn report(front: &str, context: u32, thinking: ThinkingMode) -> TestResult<Requirements> {
    let prepared = Scenario::default()
        .model(model("current", context, thinking)?)
        .prepare(&declaring(front))?;
    Ok(prepared.requirements)
}

fn only_unmet(requirements: &Requirements) -> TestResult<&UnmetRequirement> {
    match requirements.unmet_models.as_slice() {
        [unmet] => Ok(unmet),
        unmet => Err(format!("exactly one requirement is unmet: {unmet:?}").into()),
    }
}

#[test]
fn two_runs_writing_the_same_real_file_through_the_shared_base_conflict() -> TestResult {
    let temp =
        TempRoot::new("two_runs_writing_the_same_real_file_through_the_shared_base_conflict");
    let vfs = VfsRef::builder()
        .mount("/", RealBackend::rooted(temp.path())?)
        .store("/my/store", MemoryBackend::new())
        .build();
    let scenario = Scenario::default().context(move |ctx| ctx.with_vfs(vfs.clone()));
    let source = greeter("return 'done'");
    let (run_a, run_b) = (scenario.prepare(&source)?, scenario.prepare(&source)?);
    let access_a = run_a.ctx.vfs().acquire(Origin::new("run-a"))?;
    access_a.write("/shared.txt", b"from a")?;
    let access_b = run_b.ctx.vfs().acquire(Origin::new("run-b"))?;
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

const STRONG: &str = "models: { strong: { keywords: [thinking], min_context: 200000 } }";

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
fn one_model_fills_every_role_and_soft_keywords_are_never_checked() -> TestResult {
    // Soft keywords document author intent, and are never checked.
    let source = asking_every_role(
        "models: { fast: { keywords: [fast] }, creative: { keywords: [small, creative, chat] } }",
    );
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
fn a_role_map_with_models_but_no_default_is_refused_with_one_unbound_entry() -> TestResult {
    let strong = model("strong-model", 200_000, ThinkingMode::Switchable)?;
    let Prepared { requirements, .. } = Scenario::default()
        .roles([(ModelRole::Strong, strong)])
        .prepare(&declaring(""))?;
    let unmet = only_unmet(&requirements)?;
    assert_eq!(unmet.role, ModelRole::Default);
    assert_eq!(unmet.check, RequirementCheck::Unbound);
    assert_eq!(unmet.required, "a model");
    assert_eq!(unmet.actual, "none");
    let refusal = requirements
        .refusal()
        .ok_or("a missing default refuses the run")?;
    assert_eq!(refusal.kind(), RunErrorKind::RequirementsUnmet);
    assert!(
        refusal.to_string().contains(
            "- role 'default': requires a model; the caller's role map has no 'default' entry"
        ),
        "{refusal}"
    );
    Ok(())
}

#[test]
fn a_context_minimum_above_the_filling_models_is_reported() -> TestResult {
    let requirements = report(STRONG, 32_000, ThinkingMode::Always)?;
    assert!(!requirements.is_satisfied());
    assert_eq!(requirements.missing_packs, []);
    let unmet = only_unmet(&requirements)?;
    assert_eq!(unmet.role, ModelRole::Strong);
    assert_eq!(unmet.check, RequirementCheck::ContextMinimum);
    assert_eq!(unmet.required, "200000");
    assert_eq!(unmet.actual, "32000");
    Ok(())
}

#[test]
fn a_hard_keyword_the_filling_model_fails_is_reported() -> TestResult {
    // The context minimum is met, so only the thinking check fires.
    let requirements = report(STRONG, 200_000, ThinkingMode::Never)?;
    let unmet = only_unmet(&requirements)?;
    assert_eq!(unmet.role, ModelRole::Strong);
    assert_eq!(unmet.check, RequirementCheck::Thinking);
    assert_eq!(unmet.required, "thinking: true");
    assert_eq!(unmet.actual, "Never");

    let small = "models: { small: { keywords: [no-thinking] } }";
    let requirements = report(small, 32_000, ThinkingMode::Always)?;
    let unmet = only_unmet(&requirements)?;
    assert_eq!(unmet.role, ModelRole::Small);
    assert_eq!(unmet.check, RequirementCheck::Thinking);
    assert_eq!(unmet.required, "thinking: false");
    assert_eq!(unmet.actual, "Always");
    Ok(())
}

fn prepared_over(info: &ToolInfo) -> TestResult<Prepared> {
    let catalog = ToolCatalog::new([info.clone()])?;
    Scenario::default()
        .context(move |ctx| ctx.with_tools(catalog.clone()))
        .prepare(&declaring("uses: [web]"))
}

#[test]
fn a_prepared_context_keeps_the_caller_supplied_catalog() -> TestResult {
    let info = tool_info(
        "web/fetch",
        serde_json::json!({"type": "object", "properties": {"url": {"type": "string"}}}),
    )?
    .structured(true);
    let Prepared {
        ctx, requirements, ..
    } = prepared_over(&info)?;
    assert!(
        requirements.is_satisfied(),
        "a catalog reports nothing: {requirements:?}"
    );
    assert_eq!(ctx.tools().tools(), [info]);
    Ok(())
}

#[test]
fn prepare_reports_nothing_for_a_declared_pack_the_catalog_lacks() -> TestResult {
    let other = tool_info(
        "other/search",
        serde_json::json!({"type": "object", "properties": {}}),
    )?;
    let Prepared {
        ctx, requirements, ..
    } = prepared_over(&other)?;
    assert!(
        requirements.is_satisfied(),
        "the Engine's prepare checks model roles only: {requirements:?}"
    );
    assert_eq!(ctx.tools().tools(), [other]);
    Ok(())
}
