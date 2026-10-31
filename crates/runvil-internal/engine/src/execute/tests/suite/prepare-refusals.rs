//! The prepare pass refusing an unsatisfiable prompt with the model-readable
//! notice, and running a satisfiable one, through the test driver's
//! prepare-then-run path.

use runvil_types::models::ThinkingMode;

use super::{current_model, under_model};
use crate::RunErrorKind;
use crate::test_support::scripted_chat::{ScriptedChat, text};

/// A prompt declaring one model role with a hard keyword and a context
/// minimum.
const DECLARES_STRONG: &str = concat!(
    "---\n",
    "name: declares-strong\n",
    "description: d\n",
    "runvil: 0\n",
    "models:\n",
    "  strong:\n",
    "    keywords: [frontier, thinking]\n",
    "    min_context: 200000\n",
    "    description: Deep analysis\n",
    "---\n\n",
    "# Title\n\n",
    "## Only\n\n",
    "```lua\n",
    "return 'done'\n",
    "```\n",
);

#[test]
fn run_with_fixture_refuses_an_unsatisfiable_prompt_with_a_model_readable_notice() {
    let model = current_model(32_000, ThinkingMode::Never);
    let trace = under_model(DECLARES_STRONG, "refuse", model).run();
    let error = trace.error();
    assert_eq!(error.kind(), RunErrorKind::RequirementsUnmet);
    let notice = error.to_string();
    // The notice is written to be read by a model: it names the role,
    // each failed check, and required versus actual.
    assert!(
        notice.starts_with("the environment cannot satisfy this prompt:"),
        "the notice opens with the standing refusal line: {notice}"
    );
    assert!(
        notice.contains(
            "- role 'strong': requires a context of at least 200000 tokens; \
             the model filling it provides 32000"
        ),
        "the notice gives required versus actual context: {notice}"
    );
    assert!(
        notice.contains(
            "- role 'strong': requires 'thinking: true'; \
             the thinking capability of the model filling it is Never"
        ),
        "the notice gives required versus actual thinking: {notice}"
    );
}

#[test]
fn run_with_fixture_prepares_implicitly_and_runs_a_satisfiable_prompt() {
    // The scenario prepares implicitly, and the declared role's
    // requirements are met by the model that fills it.
    let model = current_model(200_000, ThinkingMode::Always);
    let trace = under_model(DECLARES_STRONG, "implicit", model).run();
    assert_eq!(trace.text(), "done");
}

/// A prompt declaring one `no-thinking` role whose section asks it one
/// question.
const DECLARES_NO_THINKING: &str = concat!(
    "---\n",
    "name: declares-no-thinking\n",
    "description: d\n",
    "runvil: 0\n",
    "models:\n",
    "  small:\n",
    "    keywords: [no-thinking]\n",
    "---\n\n",
    "# Title\n\n",
    "## Only\n\n",
    "```lua\n",
    "models.use('small')\n",
    "return models.infer('ping')\n",
    "```\n",
);

#[test]
fn a_no_thinking_role_on_a_switchable_model_prepares_and_asks_for_thinking_off() {
    let gateway = ScriptedChat::new(vec![text("pong")]);
    let model = current_model(32_000, ThinkingMode::Switchable);
    let trace = under_model(DECLARES_NO_THINKING, "switchable", model)
        .chat(&gateway)
        .run();
    // An unmet requirement would refuse the run, so the reply also shows
    // that a switchable model can turn thinking off.
    assert_eq!(trace.text(), "pong");
    let body = gateway
        .last_request()
        .expect("the round reaches the gateway");
    assert_eq!(body.options.model(), "current");
    assert_eq!(body.options.thinking(), Some(false));
}

#[test]
fn a_no_thinking_role_on_an_always_thinking_model_is_refused() {
    let model = current_model(32_000, ThinkingMode::Always);
    let trace = under_model(DECLARES_NO_THINKING, "always", model).run();
    let error = trace.error();
    assert_eq!(error.kind(), RunErrorKind::RequirementsUnmet);
    let notice = error.to_string();
    assert!(
        notice.contains(
            "- role 'small': requires 'thinking: false'; \
             the thinking capability of the model filling it is Always"
        ),
        "the notice gives required versus actual thinking: {notice}"
    );
}

/// A prompt declaring one role that needs a long context.
const DECLARES_LONG_CONTEXT: &str = concat!(
    "---\n",
    "name: declares-long-context\n",
    "description: d\n",
    "runvil: 0\n",
    "models:\n",
    "  creative:\n",
    "    min_context: 200000\n",
    "---\n\n",
    "# Title\n\n",
    "## Only\n\n",
    "Done.\n",
);

/// An unmet requirement found at prepare refuses the run with the
/// model-readable notice text, line for line.
#[test]
fn an_unmet_requirement_produces_the_model_readable_notice() {
    let model = current_model(32_000, ThinkingMode::Switchable);
    let trace = under_model(DECLARES_LONG_CONTEXT, "notice", model).run();
    let error = trace.error();
    assert_eq!(error.kind(), RunErrorKind::RequirementsUnmet);
    assert_eq!(
        error.to_string(),
        "the environment cannot satisfy this prompt:\n\
         - role 'creative': requires a context of at least 200000 tokens; \
         the model filling it provides 32000"
    );
}
