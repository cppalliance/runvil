//! Prepare-pass tests that reach engine-only items: the per-run store
//! mount's claims isolation, the caller's role map filling the model set, and
//! the test driver's prepare-then-run path refusing an unsatisfiable prompt
//! with the model-readable notice or running a satisfiable one. The rest of
//! the prepare suite runs against the `runvil` facade.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use crate::RunErrorKind;
use crate::execute::tests::context::test_context;
use crate::execute::{Effect, EffectAnswer, RequirementCheck, UnmetRequirement};
use crate::model::ModelBinding;
use crate::test_support::scenario::Scenario;
use crate::test_support::scripted_chat::{ScriptedChat, text};
use runvil_model_client::client::{Completion, CompletionResult};
use runvil_parser::test_support::{PromptSource, parse_ok};
use runvil_types::event::Event;
use runvil_types::models::{ModelId, ModelInfo, ModelRole, ThinkingMode};
use runvil_vfs::Origin;

/// A prompt declaring no Packs at all.
const DECLARES_NOTHING: &str = concat!(
    "---\n",
    "name: declares-nothing\n",
    "description: d\n",
    "runvil: 0\n",
    "---\n\n",
    "# Title\n\n",
    "## Only\n\n",
    "Done.\n",
);

#[test]
fn two_runs_writing_the_same_store_path_do_not_conflict() {
    let prompt = parse_ok(DECLARES_NOTHING);
    let (ctx_a, _) = test_context("run-a").prepare(&prompt, BTreeMap::new());
    let (ctx_b, _) = test_context("run-b").prepare(&prompt, BTreeMap::new());
    let access_a = ctx_a
        .vfs()
        .acquire(Origin::new("run-a"))
        .expect("run a acquires");
    let access_b = ctx_b
        .vfs()
        .acquire(Origin::new("run-b"))
        .expect("run b acquires");
    let store_a =
        runvil_vfs::detail::store_view(&access_a).expect("run a's handle declares a store");
    let store_b =
        runvil_vfs::detail::store_view(&access_b).expect("run b's handle declares a store");
    // Both writes proceed while both accesses are live: each run's store
    // is its own storage under its own claims table.
    store_a.write("paper.md", b"from a").expect("run a writes");
    store_b.write("paper.md", b"from b").expect("run b writes");
    assert_eq!(store_a.read("paper.md").expect("run a reads"), b"from a");
    assert_eq!(store_b.read("paper.md").expect("run b reads"), b"from b");
}

/// The model `name` with the given context window and thinking capability.
fn model_named(name: &str, context: u32, thinking: ThinkingMode) -> ModelInfo {
    ModelInfo::new(
        ModelId::gateway(name).expect("the id is valid"),
        "A model for the prepare tests",
        NonZeroU32::new(context).expect("the context window is non-zero"),
        thinking,
    )
}

/// The one current model the caller chose, with the given context window
/// and thinking capability.
fn current_model(context: u32, thinking: ThinkingMode) -> ModelInfo {
    model_named("current", context, thinking)
}

/// The caller's role map holding `entries`.
fn role_map(
    entries: impl IntoIterator<Item = (ModelRole, ModelInfo)>,
) -> BTreeMap<ModelRole, ModelInfo> {
    entries.into_iter().collect()
}

/// A scenario over `source` run under the one current model `model`.
fn under_model(source: &str, name: &str, model: ModelInfo) -> Scenario {
    Scenario::new(source).name(name).current_model(model)
}

/// The aliases of the bindings in `bindings`, in order.
fn aliases(bindings: &[ModelBinding]) -> Vec<&str> {
    bindings.iter().map(ModelBinding::alias).collect()
}

/// A prompt declaring `front` and one section that returns `lua`.
fn declaring(front: &str, lua: &str) -> String {
    PromptSource::new()
        .front(front)
        .section("Only", lua)
        .build()
}

#[test]
fn a_one_model_map_gives_all_five_roles_that_model() {
    let model = model_named("only", 32_000, ThinkingMode::Switchable);
    let (ctx, requirements) = test_context("one-model").prepare(
        &parse_ok(DECLARES_NOTHING),
        role_map([(ModelRole::Default, model.clone())]),
    );
    assert!(requirements.is_satisfied(), "{requirements:?}");
    let bindings = ctx.model_set.bindings();
    assert_eq!(
        aliases(bindings),
        ["default", "strong", "small", "fast", "creative"],
        "five bindings in role order, whether or not the prompt declares them"
    );
    for binding in bindings {
        assert_eq!(binding.id(), model.id(), "role {}", binding.alias());
    }
}

#[test]
fn a_map_with_a_strong_entry_gives_strong_that_model_and_the_other_roles_the_default() {
    let default = model_named("default-model", 32_000, ThinkingMode::Switchable);
    let strong = model_named("strong-model", 200_000, ThinkingMode::Always);
    let (ctx, requirements) = test_context("two-models").prepare(
        &parse_ok(DECLARES_NOTHING),
        role_map([
            (ModelRole::Default, default.clone()),
            (ModelRole::Strong, strong.clone()),
        ]),
    );
    assert!(requirements.is_satisfied(), "{requirements:?}");
    for binding in ctx.model_set.bindings() {
        let expected = if binding.alias() == "strong" {
            &strong
        } else {
            &default
        };
        assert_eq!(binding.id(), expected.id(), "role {}", binding.alias());
        assert_eq!(
            binding.context(),
            expected.context(),
            "role {}",
            binding.alias()
        );
    }
}

#[test]
fn a_context_minimum_above_the_filling_models_is_one_unmet_entry_for_that_role() {
    let prompt = parse_ok(&declaring(
        "models: { strong: { min_context: 200000 } }",
        "return 'x'",
    ));
    let (_, requirements) = test_context("minimum").prepare(
        &prompt,
        role_map([(
            ModelRole::Default,
            current_model(32_000, ThinkingMode::Switchable),
        )]),
    );
    assert_eq!(
        requirements.unmet_models,
        [UnmetRequirement {
            role: ModelRole::Strong,
            check: RequirementCheck::ContextMinimum,
            required: "200000".to_owned(),
            actual: "32000".to_owned(),
        }]
    );
}

#[test]
fn a_hard_thinking_keyword_the_filling_model_fails_is_a_thinking_check() {
    let prompt = parse_ok(&declaring(
        "models: { strong: { keywords: [thinking] }, small: { keywords: [no-thinking] } }",
        "return 'x'",
    ));
    let never = model_named("never", 32_000, ThinkingMode::Never);
    let always = model_named("always", 32_000, ThinkingMode::Always);
    let (_, requirements) = test_context("thinking").prepare(
        &prompt,
        role_map([(ModelRole::Default, never), (ModelRole::Small, always)]),
    );
    assert_eq!(
        requirements.unmet_models,
        [
            UnmetRequirement {
                role: ModelRole::Strong,
                check: RequirementCheck::Thinking,
                required: "thinking: true".to_owned(),
                actual: "Never".to_owned(),
            },
            UnmetRequirement {
                role: ModelRole::Small,
                check: RequirementCheck::Thinking,
                required: "thinking: false".to_owned(),
                actual: "Always".to_owned(),
            },
        ]
    );
}

#[test]
fn a_soft_keyword_and_a_description_are_never_checked() {
    let prompt = parse_ok(&declaring(
        "models: { strong: { keywords: [frontier, fast], description: Deep analysis } }",
        "return 'x'",
    ));
    let (ctx, requirements) = test_context("soft").prepare(
        &prompt,
        role_map([(
            ModelRole::Default,
            current_model(32_000, ThinkingMode::Never),
        )]),
    );
    assert!(requirements.is_satisfied(), "{requirements:?}");
    let strong = ctx
        .model_set
        .binding("strong")
        .expect("the strong role is bound");
    assert_eq!(strong.description(), "Deep analysis");
}

#[test]
fn a_declared_label_outside_the_five_roles_is_ignored() {
    let prompt = parse_ok(&declaring(
        "models: { reviewer: { min_context: 900000 } }",
        "return 'x'",
    ));
    let (ctx, requirements) = test_context("ignored").prepare(
        &prompt,
        role_map([(
            ModelRole::Default,
            current_model(32_000, ThinkingMode::Never),
        )]),
    );
    assert!(requirements.is_satisfied(), "{requirements:?}");
    assert!(ctx.model_set.binding("reviewer").is_none());
    assert_eq!(ctx.model_set.bindings().len(), 5);
}

#[test]
fn a_non_empty_map_without_a_default_entry_is_one_unbound_entry_and_refuses_the_run() {
    let prompt = parse_ok(DECLARES_NOTHING);
    let strong = model_named("strong-model", 200_000, ThinkingMode::Switchable);
    let (ctx, requirements) =
        test_context("unbound").prepare(&prompt, role_map([(ModelRole::Strong, strong)]));
    assert_eq!(
        requirements.unmet_models,
        [UnmetRequirement {
            role: ModelRole::Default,
            check: RequirementCheck::Unbound,
            required: "a model".to_owned(),
            actual: "none".to_owned(),
        }]
    );
    assert!(
        ctx.model_set.bindings().is_empty(),
        "the model set stays empty when the role map has no default"
    );
    let refusal = requirements
        .refusal()
        .expect("a missing default refuses the run");
    assert_eq!(refusal.kind(), RunErrorKind::RequirementsUnmet);
    assert!(
        refusal.to_string().contains(
            "- role 'default': requires a model; the caller's role map has no 'default' entry"
        ),
        "{refusal}"
    );
}

#[test]
fn an_empty_map_leaves_the_model_set_empty_and_a_model_call_raises_model_required() {
    let (ctx, requirements) =
        test_context("empty").prepare(&parse_ok(DECLARES_NOTHING), BTreeMap::new());
    assert!(requirements.unmet_models.is_empty(), "{requirements:?}");
    assert!(ctx.model_set.bindings().is_empty());

    let asking = declaring("", "return models.infer('ping')");
    let trace = Scenario::new(asking).run();
    let error = trace.error();
    assert_eq!(error.kind(), RunErrorKind::Binding, "{error}");
    assert!(
        error.to_string().contains("model binding required"),
        "{error}"
    );

    let silent = declaring("", "return 'no model needed'");
    assert_eq!(Scenario::new(silent).run().text(), "no model needed");
}

#[test]
fn a_prompt_with_no_models_block_runs_on_the_default_role_and_can_pick_another() {
    let source = declaring(
        "",
        "
        local first = models.infer('one')
        models.use('strong')
        return first .. '|' .. models.infer('two')",
    );
    let gateway = ScriptedChat::new(vec![text("a"), text("b")]);
    let trace = Scenario::new(source)
        .current_model(model_named("only", 32_000, ThinkingMode::Switchable))
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "a|b");
    let served: Vec<String> = gateway
        .requests()
        .iter()
        .map(|body| body.options.model().to_owned())
        .collect();
    assert_eq!(served, ["only", "only"], "one model fills every role");
}

#[test]
fn a_use_of_each_role_runs_its_round_on_the_model_that_fills_the_role() {
    let source = declaring(
        "",
        "
        models.use('strong')
        local strong = models.infer('one')
        models.use('fast')
        return strong .. '|' .. models.infer('two')",
    );
    let gateway = ScriptedChat::new(vec![text("a"), text("b")]);
    let trace = Scenario::new(source)
        .current_model(model_named("model-a", 32_000, ThinkingMode::Switchable))
        .role_model(
            ModelRole::Strong,
            model_named("model-b", 32_000, ThinkingMode::Switchable),
        )
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "a|b");
    let served: Vec<String> = gateway
        .requests()
        .iter()
        .map(|body| body.options.model().to_owned())
        .collect();
    assert_eq!(
        served,
        ["model-b", "model-a"],
        "strong runs on B, and fast falls back to the default model A"
    );
}

#[test]
fn a_strong_round_attributes_its_request_and_its_events_to_the_strong_model() {
    let source = declaring(
        "",
        "
        models.use('strong')
        return models.infer('ping')",
    );
    let requested = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorder = Arc::clone(&requested);
    let trace = Scenario::new(source)
        .current_model(model_named("model-a", 32_000, ThinkingMode::Switchable))
        .role_model(
            ModelRole::Strong,
            model_named("model-b", 32_000, ThinkingMode::Switchable),
        )
        .chat_with(move |effect| {
            let Effect::Chat { binding, .. } = effect else {
                panic!("a model round, got {effect:?}");
            };
            let requested_name = binding.completion_options().model().to_owned();
            recorder
                .lock()
                .expect("the recorder is not poisoned")
                .push(requested_name.clone());
            // A caller labels the completion with the model the request named.
            let completion =
                Completion::from_result(CompletionResult::Text("pong".to_owned()), requested_name)
                    .expect("a text completion builds");
            EffectAnswer::Chat(Ok(Box::new(completion)))
        })
        .run();
    assert_eq!(trace.text(), "pong");
    assert_eq!(
        *requested.lock().expect("the recorder is not poisoned"),
        ["model-b"],
        "the request names the strong model"
    );
    let reply_models: Vec<&str> = trace
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::AssistantReply { model, .. } => Some(model.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        reply_models,
        ["model-b"],
        "the round's reply event carries the strong model's name"
    );
}

#[path = "prepare-refusals.rs"]
mod refusals;
