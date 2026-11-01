//! The run's first step: a run whose only section returns a literal is
//! done at once, a run that cannot start reports its failure with no run
//! events, and so does a run an answer decides before it starts.

use super::*;

#[test]
fn a_run_whose_only_section_returns_a_literal_is_done_at_its_first_step() {
    let mut run = run_of("return 'hello'", run_context());
    let Step::Done {
        result: RunResult::Completed(text),
        ..
    } = run.step()
    else {
        panic!("the literal run is done at once");
    };
    assert_eq!(text, "hello");
}

#[test]
fn a_run_that_cannot_start_reports_its_failure_on_the_first_step() {
    let source = "---\nname: t\ndescription: d\nrunvil: 7\n---\n\n# Run\n\n## Only\n\ndone\n";
    let prompt = Prompt::parse(source, "run-test")
        .0
        .expect("the prompt parses whatever version it declares");
    let mut run = Run::new(Arc::new(prompt), "", run_context());
    let Step::Done { result, events } = run.step() else {
        panic!("a run that cannot start is done at once");
    };
    assert!(events.is_empty(), "nothing ran, nothing reported");
    let RunResult::Failure(error) = result else {
        panic!("an unsupported version fails, got {result:?}");
    };
    assert_eq!(error.kind(), crate::execute::RunErrorKind::Version);
    assert!(run.decided(), "a run that never started is decided");
    let Step::Done {
        result: RunResult::Failure(again),
        ..
    } = run.step()
    else {
        panic!("a finished run stays done");
    };
    assert_eq!(again.kind(), crate::execute::RunErrorKind::Internal);
}

#[test]
fn an_answer_before_the_first_step_fails_the_run_with_no_run_events() {
    let mut run = run_of("return 'never runs'", run_context());
    run.resume(EffectId(7), EffectAnswer::Timer);
    assert!(run.decided(), "the stray answer decides the run");
    let Step::Done { result, events } = run.step() else {
        panic!("a run decided before it started is done at its first step");
    };
    assert!(
        events.is_empty(),
        "a run that never started reports no run events: {events:?}"
    );
    let RunResult::Failure(error) = result else {
        panic!("a stray answer fails the run, got {result:?}");
    };
    assert_eq!(error.kind(), crate::execute::RunErrorKind::Internal);
    assert!(
        error
            .to_string()
            .contains("an answer arrived for an effect the run did not issue"),
        "{error}"
    );
}
