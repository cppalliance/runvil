//! Start tests that reach engine-only items: the per-run store mount's
//! claims isolation, the caller's role map filling the model set, and the
//! run's first step refusing an unsatisfiable prompt with the
//! model-readable notice or running a satisfiable one. The rest of the
//! suite runs against the `runvil` facade.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use crate::execute::context::ChainState;
use crate::execute::tests::context::test_params;
use crate::execute::{Effect, EffectAnswer, RunParams};
use crate::model::ModelBinding;
use crate::prompt::Prompt;
use crate::test_support::scenario::Scenario;
use crate::test_support::scripted_chat::{ScriptedChat, text};
use runvil_model_client::client::{Completion, CompletionResult};
use runvil_prompt::test_support::{PromptSource, parse_ok};
use runvil_types::error::ErrorCode;
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
    let (ctx_a, ctx_b) = (test_params("run-a"), test_params("run-b"));
    let store_a = ctx_a
        .vfs
        .acquire_store(Origin::new("run-a"))
        .expect("run a's handle declares a store");
    let store_b = ctx_b
        .vfs
        .acquire_store(Origin::new("run-b"))
        .expect("run b's handle declares a store");
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

/// The state a run of `prompt` builds over `params`, as `Run::new` builds
/// it.
fn state_of(prompt: &Prompt, params: RunParams) -> ChainState {
    ChainState::root(Arc::new(prompt.clone()), "", params)
}

/// The refusal notice of `scenario`'s run, after asserting that its first
/// step failed it as `RequirementsUnmet` with no run events and no
/// effects.
#[track_caller]
fn refusal(scenario: Scenario) -> String {
    let trace = scenario.run();
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::RequirementsUnmet, "{error}");
    assert!(
        trace.run_events().is_empty(),
        "a refused run reports no run events: {:?}",
        trace.run_events()
    );
    assert!(trace.effects().is_empty(), "a refused run issues nothing");
    error.to_string()
}

#[test]
fn a_one_model_map_gives_all_five_roles_that_model() {
    let model = model_named("only", 32_000, ThinkingMode::Switchable);
    let state = state_of(
        &parse_ok(DECLARES_NOTHING),
        test_params("one-model").with_models(role_map([(ModelRole::Default, model.clone())])),
    );
    assert_eq!(state.refusal(), None);
    let set = state.model_set();
    let bindings = set.bindings();
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
    let state = state_of(
        &parse_ok(DECLARES_NOTHING),
        test_params("two-models").with_models(role_map([
            (ModelRole::Default, default.clone()),
            (ModelRole::Strong, strong.clone()),
        ])),
    );
    assert_eq!(state.refusal(), None);
    for binding in state.model_set().bindings() {
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
fn a_context_minimum_above_the_filling_models_refuses_the_run_naming_that_role() {
    let source = declaring("models: { strong: { min_context: 200000 } }", "return 'x'");
    let notice = refusal(
        Scenario::new(source).current_model(current_model(32_000, ThinkingMode::Switchable)),
    );
    assert_eq!(
        notice,
        "the environment cannot satisfy this prompt:\n\
         - role 'strong': requires a context of at least 200000 tokens; \
         the model filling it provides 32000"
    );
}

#[test]
fn a_thinking_requirement_the_filling_model_fails_refuses_the_run_in_role_order() {
    let source = declaring(
        "models: { strong: { thinking: true }, small: { thinking: false } }",
        "return 'x'",
    );
    let never = model_named("never", 32_000, ThinkingMode::Never);
    let always = model_named("always", 32_000, ThinkingMode::Always);
    let notice = refusal(
        Scenario::new(source)
            .current_model(never)
            .role_model(ModelRole::Small, always),
    );
    assert_eq!(
        notice,
        "the environment cannot satisfy this prompt:\n\
         - role 'strong': requires 'thinking: true'; \
         the thinking capability of the model filling it is Never\n\
         - role 'small': requires 'thinking: false'; \
         the thinking capability of the model filling it is Always"
    );
}

#[test]
fn a_role_binding_carries_the_description_of_the_model_that_fills_it() {
    let prompt = parse_ok(&declaring("models: { strong: {} }", "return 'x'"));
    let state = state_of(
        &prompt,
        test_params("description").with_models(role_map([(
            ModelRole::Default,
            current_model(32_000, ThinkingMode::Never),
        )])),
    );
    assert_eq!(state.refusal(), None);
    let set = state.model_set();
    let strong = set.binding("strong").expect("the strong role is bound");
    assert_eq!(strong.description(), "A model for the prepare tests");
    assert_eq!(set.bindings().len(), 5);
}

#[test]
fn thinking_true_on_strong_turns_thinking_on_for_strong_rounds_only() {
    let source = declaring(
        "models: { strong: { thinking: true } }",
        "
        models.use('strong')
        local first = models.infer('one')
        models.use('fast')
        return first .. '|' .. models.infer('two')",
    );
    let gateway = ScriptedChat::new(vec![text("a"), text("b")]);
    let trace = Scenario::new(source)
        .current_model(model_named("model-a", 32_000, ThinkingMode::Switchable))
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "a|b");
    let thinking: Vec<Option<bool>> = gateway
        .requests()
        .iter()
        .map(|body| body.options.thinking())
        .collect();
    assert_eq!(
        thinking,
        [Some(true), None],
        "the strong round asks for thinking and the fast round does not"
    );
}

#[test]
fn a_non_empty_map_without_a_default_entry_leaves_the_roles_empty_and_refuses_the_run() {
    let strong = model_named("strong-model", 200_000, ThinkingMode::Switchable);
    let state = state_of(
        &parse_ok(DECLARES_NOTHING),
        test_params("unbound").with_models(role_map([(ModelRole::Strong, strong.clone())])),
    );
    assert!(
        state.model_set().bindings().is_empty(),
        "the model set stays empty when the role map has no default"
    );
    let notice = refusal(Scenario::new(DECLARES_NOTHING).role_model(ModelRole::Strong, strong));
    assert_eq!(
        notice,
        "the environment cannot satisfy this prompt:\n\
         - role 'default': requires a model; the caller's role map has no 'default' entry"
    );
}

#[test]
fn an_empty_map_leaves_the_model_set_empty_and_a_model_call_raises_model_required() {
    let state = state_of(
        &parse_ok(DECLARES_NOTHING),
        test_params("empty").with_models(BTreeMap::new()),
    );
    assert_eq!(state.refusal(), None);
    assert!(state.model_set().bindings().is_empty());

    let asking = declaring("", "return models.infer('ping')");
    let trace = Scenario::new(asking).run();
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::ModelRequired, "{error}");
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

/// A scenario over `lua`, with `default` on model A and `strong` on model B.
fn two_role_scenario(lua: &str) -> Scenario {
    Scenario::new(declaring("", lua))
        .current_model(model_named("model-a", 32_000, ThinkingMode::Switchable))
        .role_model(
            ModelRole::Strong,
            model_named("model-b", 32_000, ThinkingMode::Switchable),
        )
}

#[test]
fn a_saved_handle_puts_a_later_round_back_on_its_model() {
    let gateway = ScriptedChat::new(vec![text("a"), text("b")]);
    let trace = two_role_scenario(
        "
        local prev = models.current()
        models.use('strong')
        local first = models.infer('one')
        models.use(prev)
        return first .. '|' .. models.infer('two')",
    )
    .chat(&gateway)
    .run();
    assert_eq!(trace.text(), "a|b");
    assert_eq!(
        gateway.requested_models(),
        ["model-b", "model-a"],
        "the saved handle returns the section to the default model A"
    );
}

#[test]
fn models_use_of_a_handle_applies_its_options_on_top_of_the_handles_model() {
    let gateway = ScriptedChat::new(vec![text("a")]);
    let trace = two_role_scenario(
        "
        local prev = models.current()
        models.use(prev, { temperature = 0.2 })
        return models.infer('one')",
    )
    .chat(&gateway)
    .run();
    assert_eq!(trace.text(), "a");
    assert_eq!(gateway.requested_models(), ["model-a"]);
    assert_eq!(
        gateway.requests()[0]
            .options
            .temperature()
            .map(crate::model::Temperature::get),
        Some(0.2),
        "the temperature applies on top of the handle's own binding"
    );
}

#[test]
fn a_raw_id_handle_runs_the_next_round_on_its_gateway_model() {
    let gateway = ScriptedChat::new(vec![text("a")]);
    let trace = two_role_scenario(
        "
        models.use(models.get('vendor/raw-model'))
        return models.infer('one')",
    )
    .params(|ctx| ctx.with_ui(serde_json::json!({})))
    .chat(&gateway)
    .run();
    assert_eq!(trace.text(), "a");
    assert_eq!(
        gateway.requested_models(),
        ["vendor/raw-model"],
        "a raw-id handle from models.get is accepted by models.use"
    );
}
#[path = "prepare-refusals.rs"]
mod refusals;
