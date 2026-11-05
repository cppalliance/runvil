//! The run's first step: a run whose only section returns a literal is
//! done at once, a run that cannot start reports its failure with no run
//! events, and so does a run an answer decides before it starts.

use super::*;

#[test]
fn a_run_whose_only_section_returns_a_literal_is_done_at_its_first_step() {
    let mut run = run_of("return 'hello'", run_params());
    let Step::Done {
        result: Ok(final_text),
        ..
    } = run.step()
    else {
        panic!("the literal run is done at once");
    };
    assert_eq!(final_text, "hello");
}

#[test]
fn a_run_that_cannot_start_reports_its_failure_on_the_first_step() {
    let source = "---\nname: t\ndescription: d\nrunvil: 7\n---\n\n# Run\n\n## Only\n\ndone\n";
    let prompt = Prompt::parse(source, "prompt", "run-test")
        .0
        .expect("the prompt parses whatever version it declares");
    let mut run = Run::new(Arc::new(prompt), "", run_params());
    let Step::Done { result, events } = run.step() else {
        panic!("a run that cannot start is done at once");
    };
    assert!(events.is_empty(), "nothing ran, nothing reported");
    let error = result.expect_err("an unsupported version fails the run");
    assert_eq!(
        (error.code, error.located_message()),
        (
            ErrorCode::UnsupportedVersion,
            "unsupported runvil version: 7 (this build supports major 0) (at prompt:4)".to_owned()
        )
    );
    assert!(
        run.decided_for_test(),
        "a run that never started is decided"
    );
    let Step::Done {
        result: Err(again), ..
    } = run.step()
    else {
        panic!("a finished run stays done");
    };
    assert_eq!(again.code, ErrorCode::Internal, "{again:?}");
}

#[test]
fn an_answer_before_the_first_step_fails_the_run_with_no_run_events() {
    let mut run = run_of("return 'never runs'", run_params());
    run.resume(EffectId(7), EffectAnswer::Timer);
    assert!(run.decided_for_test(), "the stray answer decides the run");
    let Step::Done { result, events } = run.step() else {
        panic!("a run decided before it started is done at its first step");
    };
    assert!(
        events.is_empty(),
        "a run that never started reports no run events: {events:?}"
    );
    let error = result.expect_err("a stray answer fails the run");
    assert_eq!(error.code, ErrorCode::Internal, "{error:?}");
    let message = error.located_message();
    assert!(
        message.contains("an answer arrived for an effect the run did not issue"),
        "{message}"
    );
}
