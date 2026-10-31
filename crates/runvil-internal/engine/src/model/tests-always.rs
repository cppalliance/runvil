//! The `default` role as the baseline a section starts on.

use super::*;
use runvil_model_client::model::CompletionOptions;

#[test]
fn a_section_starts_on_the_default_role_without_use() {
    let models = shared_models(vec![bound_role(
        "default",
        "A tiny model",
        "small",
        8_192,
        Some(false),
        &["no-thinking"],
    )]);
    let vm =
        section_vm_with_models(&models, &null_emitter(), "Section").expect("the section VM builds");
    let model = resolve_section_model(&vm).expect("the resolution reads the shared set");
    assert_eq!(model.as_ref().map(ModelBinding::alias), Some("default"));
    let opts = model.as_ref().map(ModelBinding::completion_options);
    let expected = CompletionOptions::new("small").with_thinking(false);
    assert_eq!(opts, Some(expected));
    vm.teardown(&null_emitter(), "Section");
}

#[test]
fn models_get_returns_an_inspectable_handle_without_selecting() {
    let models = shared_models(vec![
        bound_role(
            "default",
            "A tiny model",
            "small",
            8_192,
            Some(false),
            &["no-thinking", "fast"],
        ),
        bound_role(
            "strong",
            "A careful analysis model",
            "analyst",
            131_072,
            None,
            &[],
        ),
    ]);
    let vm =
        section_vm_with_models(&models, &null_emitter(), "Section").expect("the section VM builds");
    vm.run_chunk(
        &chunk(
            r#"local model = models.get("default")
               assert(model.label == "default")
               assert(model.name == "default")
               assert(model.model_id == "small")
               assert(model.description == "A tiny model")
               assert(model.context == 8192)
               assert(model.thinking == false)
               assert(#model.capabilities == 2)
               assert(model.capabilities[1] == "no-thinking")
               assert(model.capabilities[2] == "fast")
               models.get("strong")"#,
        ),
        &null_emitter(),
        "Section",
    )
    .expect("the handle exposes the role label and the full keyword set");
    let current = resolve_section_model(&vm)
        .expect("the resolution reads the shared set")
        .expect("the section has a current model");
    assert_eq!(
        current.alias(),
        "default",
        "models.get inspects a role without selecting it"
    );
    vm.teardown(&null_emitter(), "Section");
}
