//! Lua-driven `models.use` integration tests over pre-filled role bindings.

use super::*;

#[test]
fn models_use_selects_a_role_by_label() {
    let models = shared_models(vec![
        bound_role("default", "A tiny model", "small", 8_192, None, &[]),
        bound_role(
            "strong",
            "A careful analysis model",
            "analyst",
            131_072,
            Some(true),
            &["thinking", "frontier"],
        ),
    ]);
    let vm =
        section_vm_with_models(&models, &null_emitter(), "Section").expect("the section VM builds");
    vm.run_chunk(
        &chunk(r#"models.use("strong")"#),
        &null_emitter(),
        "Section",
    )
    .expect("a role label selects");
    let model = resolve_section_model(&vm).expect("the resolution reads the selection");
    let model = model.expect("a selection resolves");
    assert_eq!(model.alias(), "strong");
    assert_eq!(model.invocation().thinking, Some(true));
    assert_eq!(
        model.capabilities(),
        &["thinking".to_owned(), "frontier".to_owned()]
    );
    vm.teardown(&null_emitter(), "Section");
}

#[test]
fn a_section_with_no_models_has_no_current_model() {
    let models = shared_models(Vec::new());
    let vm =
        section_vm_with_models(&models, &null_emitter(), "Section").expect("the section VM builds");
    let model = resolve_section_model(&vm).expect("the resolution reads the shared set");
    assert!(model.is_none());
    vm.teardown(&null_emitter(), "Section");
}

#[test]
fn models_use_rejects_a_label_outside_the_five_roles() {
    let models = shared_models(vec![bound_role(
        "default",
        "A tiny model",
        "small",
        8_192,
        None,
        &[],
    )]);
    let vm =
        section_vm_with_models(&models, &null_emitter(), "Section").expect("the section VM builds");
    let error = vm
        .run_chunk(
            &chunk(r#"models.use("reviewer")"#),
            &null_emitter(),
            "Section",
        )
        .expect_err("a label outside the five roles must fail");
    let rendered = error.to_string();
    assert!(
        rendered.contains(
            "models.use label \"reviewer\" is not a model role: expected default, strong, small, fast or creative"
        ),
        "the error must name the label and the five roles: {rendered}"
    );
    vm.teardown(&null_emitter(), "Section");
}

#[test]
fn models_use_with_no_model_for_the_role_says_the_caller_supplied_none() {
    let models = shared_models(Vec::new());
    let vm =
        section_vm_with_models(&models, &null_emitter(), "Section").expect("the section VM builds");
    let error = vm
        .run_chunk(
            &chunk(r#"models.use("default")"#),
            &null_emitter(),
            "Section",
        )
        .expect_err("a role with no model must fail");
    let rendered = error.to_string();
    assert!(
        rendered.contains("the caller supplied no model"),
        "the error must say the caller supplied no model: {rendered}"
    );
    vm.teardown(&null_emitter(), "Section");
}
