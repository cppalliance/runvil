//! `models.get` and a handle's `infer`: a handle leaves the section
//! model alone, `models.infer` uses the section model and never binds
//! `reply`, `models.use` re-selection steers the next round, a run whose
//! caller gave no model fails `models.infer`, and a handle's `provider` reads
//! the catalog's.

use super::*;
use crate::model::ModelRole;
use crate::test_support::scenario::test_model;

/// A run of `source` whose caller maps the `default` role to `main-model` and
/// the `strong` role to `strong-model`: the two-model shape a one-model
/// caller cannot produce, exercising the runtime's role resolution.
#[track_caller]
fn two_models(source: &PromptSource) -> Scenario {
    Scenario::new(source.build())
        .current_model(test_model("main-model"))
        .role_model(ModelRole::Strong, test_model("strong-model"))
}

#[test]
fn models_get_returns_a_handle_without_changing_the_section_model() {
    let gateway = ScriptedChat::new(vec![text("hello from the mock")]);
    let source = PromptSource::new().prose(
        "Only",
        "
            ```lua
            store.write('handle.txt', models.get('strong').name)
            ```

            Ask the model.

            ```lua
            return models.infer(prose)
            ```",
    );
    let trace = two_models(&source).chat(&gateway).run();

    assert_eq!(trace.text(), "hello from the mock");
    assert_eq!(
        trace.read("handle.txt").unwrap(),
        "strong",
        "models.get must return the strong handle"
    );
    let body = gateway
        .last_request()
        .expect("complete must reach the gateway");
    assert_eq!(
        body.options.model(),
        "main-model",
        "models.get must not change the section's model"
    );
}

#[test]
fn models_infer_uses_the_section_model_without_touching_reply() {
    let gateway = ScriptedChat::new(vec![text("pong")]);
    let source = PromptSource::new().prose(
        "Only",
        "
        ```lua
        var.r = models.infer('ping')
        ```

        ```lua
        return var.r .. ':' .. tostring(reply)
        ```",
    );
    let trace = Scenario::new(source.build()).bound().chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "pong:nil",
        "models.infer must not bind the section's reply"
    );

    let body = gateway
        .last_request()
        .expect("complete must reach the gateway");
    assert_eq!(body.options.model(), "claude-sonnet-4-6");
    assert!(
        body.tools.is_empty(),
        "models.infer advertises no tools: {body:?}"
    );
    assert_eq!(
        body.messages.len(),
        1,
        "models.infer runs on a fresh context: {body:?}"
    );
}

#[test]
fn handle_infer_uses_that_model_regardless_of_the_section_model() {
    let gateway = ScriptedChat::new(vec![text("pong")]);
    let source =
        PromptSource::new().section("Only", "return models.infer(models.get('strong'), 'ping')");
    let trace = two_models(&source).chat(&gateway).run();
    assert_eq!(trace.text(), "pong");
    let body = gateway
        .last_request()
        .expect("complete must reach the gateway");
    assert_eq!(
        body.options.model(),
        "strong-model",
        "a handle's infer must use the handle's model, not the section default"
    );
}

#[test]
fn models_use_reselection_steers_the_next_round() {
    let gateway = ScriptedChat::new(vec![text("first"), text("second")]);
    let source = PromptSource::new().section(
        "Only",
        "
        models.use('default')
        models.infer('ping')
        models.use('strong')
        return models.infer('ping')",
    );
    let trace = two_models(&source).chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "second",
        "re-selection within a section must succeed"
    );
    let requests = gateway.requests();
    assert_eq!(
        requests.len(),
        2,
        "both infer rounds must reach the gateway"
    );
    assert_eq!(
        requests[0].options.model(),
        "main-model",
        "the first round uses the initial selection"
    );
    assert_eq!(
        requests[1].options.model(),
        "strong-model",
        "the second round uses the re-selected model"
    );
}

#[test]
fn models_infer_without_a_caller_model_errors() {
    let source = PromptSource::new().section("Only", "return models.infer('ping')");
    let trace = Scenario::new(source.build()).run();
    let error = trace.error();
    assert!(
        error
            .to_string()
            .contains("model binding required for section Only"),
        "the error must name the section: {error}"
    );
}

#[test]
fn a_handles_provider_is_the_catalog_provider_or_nil() {
    let source = PromptSource::new().section(
        "Only",
        "return tostring(models.get('default').provider) .. ':' .. tostring(models.get('strong').provider)",
    );
    let main = test_model("main-model").with_provider("xai");
    let trace = Scenario::new(source.build())
        .current_model(main)
        .role_model(ModelRole::Strong, test_model("strong-model"))
        .run();
    assert_eq!(trace.text(), "xai:nil");
}
