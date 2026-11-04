//! The run's first step refusing an unsatisfiable prompt with the
//! model-readable notice, and running a satisfiable one; the version check
//! running before the requirements; and a refusal after the run's cancel
//! fired ending the run cancelled.

use runvil_types::cancel::CancelHandle;
use runvil_types::error::ErrorCode;
use runvil_types::event::Event;
use runvil_types::models::ThinkingMode;

use super::{current_model, refusal, under_model};
use crate::RunErrorKind;
use crate::execute::RunOutcome;
use crate::execute::run_error_kind;
use crate::test_support::scenario::Scenario;
use crate::test_support::scripted_chat::{ScriptedChat, text};

/// A prompt declaring one model role with a thinking requirement and a
/// context minimum.
const DECLARES_STRONG: &str = concat!(
    "---\n",
    "name: declares-strong\n",
    "description: d\n",
    "runvil: 0\n",
    "models:\n",
    "  strong:\n",
    "    thinking: true\n",
    "    min_context: 200000\n",
    "---\n\n",
    "# Title\n\n",
    "## Only\n\n",
    "```lua\n",
    "return 'done'\n",
    "```\n",
);

#[test]
fn the_first_step_refuses_an_unsatisfiable_prompt_with_a_model_readable_notice() {
    let model = current_model(32_000, ThinkingMode::Never);
    let notice = refusal(under_model(DECLARES_STRONG, "refuse", model));
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
fn a_run_whose_role_map_meets_the_prompt_runs() {
    // The declared role's requirements are met by the model that fills it.
    let model = current_model(200_000, ThinkingMode::Always);
    let trace = under_model(DECLARES_STRONG, "satisfied", model).run();
    assert_eq!(trace.text(), "done");
}

/// A prompt declaring one role that requires no thinking, whose section
/// asks it one question.
const DECLARES_NO_THINKING: &str = concat!(
    "---\n",
    "name: declares-no-thinking\n",
    "description: d\n",
    "runvil: 0\n",
    "models:\n",
    "  small:\n",
    "    thinking: false\n",
    "---\n\n",
    "# Title\n\n",
    "## Only\n\n",
    "```lua\n",
    "models.use('small')\n",
    "return models.infer('ping')\n",
    "```\n",
);

#[test]
fn a_no_thinking_role_on_a_switchable_model_runs_and_asks_for_thinking_off() {
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
    let notice = refusal(under_model(DECLARES_NO_THINKING, "always", model));
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

/// An unmet requirement refuses the run with the model-readable notice
/// text, line for line.
#[test]
fn an_unmet_requirement_produces_the_model_readable_notice() {
    let model = current_model(32_000, ThinkingMode::Switchable);
    assert_eq!(
        refusal(under_model(DECLARES_LONG_CONTEXT, "notice", model)),
        "the environment cannot satisfy this prompt:\n\
         - role 'creative': requires a context of at least 200000 tokens; \
         the model filling it provides 32000"
    );
}

#[test]
fn an_unsupported_version_is_reported_before_an_unmet_requirement() {
    let source = DECLARES_LONG_CONTEXT.replace("runvil: 0", "runvil: 7");
    let model = current_model(32_000, ThinkingMode::Switchable);
    let trace = under_model(&source, "version-first", model).run();
    let error = trace.error();
    assert_eq!(
        (error.code, error.message.as_str()),
        (
            ErrorCode::UnsupportedVersion,
            "unsupported runvil version: 7 (this build supports major 0)"
        ),
        "the version gate refuses the run first"
    );
    assert_eq!(
        error.at.as_ref().map(|at| at.start_line.get()),
        Some(4),
        "on the `runvil:` line, not the `models:` line of the unmet requirement"
    );
    assert_eq!(run_error_kind(error.code), RunErrorKind::Parse, "{error}");
    assert!(
        trace.run_events().is_empty(),
        "nothing ran, nothing reported"
    );
}

/// `scenario`, with its run's cancel flag set before the run is built.
fn cancelled_before_it_starts(scenario: Scenario) -> Scenario {
    let cancel = CancelHandle::new();
    cancel.cancel();
    scenario.config(move |ctx| ctx.with_cancel(cancel))
}

#[test]
fn a_refusal_after_the_runs_cancel_fired_ends_the_run_cancelled_with_no_run_events() {
    let model = current_model(32_000, ThinkingMode::Switchable);
    let unsupported = DECLARES_LONG_CONTEXT.replace("runvil: 0", "runvil: 7");
    for (source, refused_for) in [
        (DECLARES_LONG_CONTEXT, "an unmet requirement"),
        (unsupported.as_str(), "an unsupported version"),
    ] {
        let trace =
            cancelled_before_it_starts(under_model(source, "cancelled", model.clone())).run();
        assert!(
            matches!(trace.outcome(), RunOutcome::Cancelled),
            "{refused_for} after a cancel ends the run cancelled, got {:?}",
            trace.outcome()
        );
        assert!(
            trace.run_events().is_empty(),
            "{refused_for}: a run that never started reports no run events: {:?}",
            trace.run_events()
        );
    }
}

#[test]
fn a_run_that_passes_its_start_checks_after_a_cancel_starts_and_ends_cancelled() {
    let model = current_model(200_000, ThinkingMode::Always);
    let trace = cancelled_before_it_starts(under_model(DECLARES_STRONG, "started", model)).run();
    assert!(
        matches!(trace.outcome(), RunOutcome::Cancelled),
        "got {:?}",
        trace.outcome()
    );
    assert!(
        matches!(trace.run_events().first(), Some(Event::RunStarted { .. })),
        "the run started before it saw the cancel: {:?}",
        trace.run_events()
    );
}
