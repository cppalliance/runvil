//! Describing models and answering their rounds.

use runvil::error::ErrorCode;
use runvil::model::{ModelCatalog, ModelId, ThinkingMode};

use super::support::{Reply, Scenario, TestResult, Trace, fail, model, prompt};

fn asking() -> String {
    let lua = "
        store.write('note.md', 'hello')
        return models.infer(store.read('note.md'))
    ";
    prompt("", &[("Greet", lua)])
}

fn run_asking(reply: Reply) -> TestResult<Trace> {
    let scenario = Scenario::replies([reply]).model(model("fast", 32_000, ThinkingMode::Never)?);
    scenario.run(&asking())
}

#[test]
fn a_catalog_finds_a_model_by_id_and_refuses_a_repeated_id_as_a_whole() -> TestResult {
    let fast = model("fast", 32_000, ThinkingMode::Never)?;
    let deep = model("deep", 200_000, ThinkingMode::Always)?;

    let catalog = ModelCatalog::new([fast.clone(), deep])?;
    assert_eq!(catalog.models().len(), 2);
    let found = catalog
        .get(&ModelId::new(ModelId::GATEWAY, "deep")?)
        .ok_or("gateway/deep is in the catalog")?;
    assert_eq!(found.context().get(), 200_000);
    assert_eq!(found.thinking(), ThinkingMode::Always);

    let error = ModelCatalog::new([fast.clone(), fast])
        .err()
        .ok_or("a repeated id is refused")?;
    assert_eq!(
        error.to_string(),
        "duplicate model identity in catalog: gateway/fast"
    );
    Ok(())
}

#[test]
fn a_failed_model_round_fails_the_run_with_the_rounds_code() -> TestResult {
    let trace = run_asking(fail(ErrorCode::Overloaded))?;
    let error = trace.error()?;
    assert_eq!(
        (error.code, error.located_message()),
        (
            ErrorCode::Overloaded,
            "the model backend is overloaded (at prompt:13)".to_owned()
        )
    );
    Ok(())
}
