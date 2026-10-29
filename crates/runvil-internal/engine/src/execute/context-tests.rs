//! Tests for `RunContext` construction, forking, task sequence seeding,
//! and the model set a run binds from its filled roles.

use runvil_types::packs::PackId;
use runvil_types::tools::ToolId;

use super::*;
use crate::execute::{UnavailablePack, UsablePack};
use crate::model::ThinkingMode;
use crate::test_support::scenario::{TEST_SEED, test_model_with};

fn test_prompt() -> Prompt {
    let source = concat!(
        "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n",
        "# Title\n\n## Only\n\ndone\n",
    );
    Prompt::parse(source, "run-context-test")
        .0
        .expect("the test prompt parses")
}

fn test_state(prompt: &Prompt) -> RunState {
    state_over(
        prompt,
        RunContext::new(
            "run-context-test",
            TEST_SEED,
            runvil_types::timestamp::Timestamp::UNIX_EPOCH,
        ),
    )
}

/// The state a run of `prompt` assembles from `config`, as `Run::new`
/// assembles it.
fn state_over(prompt: &Prompt, config: RunContext) -> RunState {
    RunState::new(Arc::new(prompt.clone()), &"".into(), config)
}

#[test]
fn new_builds_a_context_over_the_prompt() {
    let prompt = test_prompt();
    let ctx = test_state(&prompt);
    assert_eq!(ctx.prompt().title(), prompt.title());
}

#[test]
fn accessor_returns_the_run_prompt() {
    let prompt = test_prompt();
    let ctx = test_state(&prompt);
    assert_eq!(**ctx.prompt(), prompt);
}

#[test]
fn clones_share_the_prompt_allocation() {
    let ctx = test_state(&test_prompt());
    let clone = ctx.clone();
    assert!(Arc::ptr_eq(&ctx.prompt, &clone.prompt));
}

#[test]
fn derived_values_come_from_the_prompt_and_limits() {
    let prompt = test_prompt();
    let ctx = test_state(&prompt);
    assert_eq!(
        ctx.section_count(),
        runvil_parser::detail::sections(&prompt).len()
    );
    assert_eq!(ctx.max_tool_iterations(), 24);
}

/// The args text and `argv` a run of `prompt` with `args` starts from.
fn args_of(prompt: &str, args: &serde_json::Value) -> (String, Option<serde_json::Value>) {
    let prompt = Prompt::parse(prompt, "run-context-test")
        .0
        .expect("the test prompt parses");
    let ctx = RunState::new(
        Arc::new(prompt),
        args,
        RunContext::new(
            "run-context-test",
            TEST_SEED,
            runvil_types::timestamp::Timestamp::UNIX_EPOCH,
        ),
    );
    (
        ctx.args().to_string(),
        ctx.argv().map(|argv| (**argv).clone()),
    )
}

#[test]
fn a_string_argument_is_the_args_text_as_it_stands() {
    let source = concat!(
        "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n",
        "# Title\n\n## Only\n\ndone\n",
    );
    assert_eq!(
        args_of(source, &serde_json::json!("two words")),
        (
            "two words".to_owned(),
            Some(serde_json::json!({ "prose": "two words" }))
        )
    );
}

#[test]
fn any_other_argument_is_its_json_text_and_a_structured_prompts_argv() {
    let source = concat!(
        "---\nname: t\ndescription: d\nrunvil: 0\n",
        "args:\n  topic:\n    type: string\n    description: the topic\n---\n\n",
        "# Title\n\n## Only\n\ndone\n",
    );
    let args = serde_json::json!({ "topic": "cats" });
    assert_eq!(
        args_of(source, &args),
        (r#"{"topic":"cats"}"#.to_owned(), Some(args))
    );
}

#[test]
fn forks_swap_only_their_own_fields() {
    let ctx = test_state(&test_prompt());
    let chain = ctx.with_args("chain-args");
    assert_eq!(&**chain.args(), "chain-args");
    assert!(Arc::ptr_eq(&ctx.prompt, &chain.prompt));
    assert_eq!(&**ctx.args(), "");

    let turns = Arc::new(AtomicU32::new(7));
    let task: TaskId = "0.4".parse().expect("a task id parses");
    let arm = ctx.with_task(task.clone(), Arc::clone(&turns));
    assert!(Arc::ptr_eq(arm.turns(), &turns));
    assert!(Arc::ptr_eq(&ctx.prompt, &arm.prompt));
    assert_eq!(arm.emitter().task(), &task);
    assert_eq!(ctx.emitter().task(), &TaskId::from(ChainId::root()));
}

#[test]
fn the_root_task_sequence_starts_where_the_context_says() {
    let ctx = state_over(
        &test_prompt(),
        RunContext::new(
            "run-context-test",
            TEST_SEED,
            runvil_types::timestamp::Timestamp::UNIX_EPOCH,
        )
        .with_provenance_start(5),
    );
    ctx.emitter()
        .report("Only", runvil_types::event::lifecycle::SECTION_STARTED);
    let events = ctx.take_events();
    assert_eq!(events[0].provenance().task, TaskId::from(ChainId::root()));
    assert_eq!(
        events[0].provenance().seq,
        5,
        "the root task's first stamp continues past the caller's parse events"
    );
}

#[test]
fn a_task_fork_reports_into_the_shared_buffer_under_its_own_task() {
    let ctx = test_state(&test_prompt());
    let task: TaskId = "0.1".parse().expect("a task id parses");
    let arm = ctx.with_task(task.clone(), Arc::new(AtomicU32::new(0)));
    ctx.emitter()
        .report("Only", runvil_types::event::lifecycle::SECTION_STARTED);
    arm.emitter()
        .report("Only", runvil_types::event::lifecycle::SECTION_STARTED);
    // Both emitters share one buffer, drained through either context.
    let events = arm.events.take();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].provenance().task, TaskId::from(ChainId::root()));
    assert_eq!(events[1].provenance().task, task);
    assert_eq!(events[1].provenance().seq, 0);
}

#[test]
fn a_filled_role_binds_with_the_catalog_info_its_render_receives() {
    let source = concat!(
        "---\nname: t\ndescription: d\nrunvil: 0\nmodels:\n  writer: {}\n---\n\n",
        "# Title\n\n## Only\n\ndone\n",
    );
    let prompt = Prompt::parse(source, "run-context-test")
        .0
        .expect("the test prompt parses");
    let info =
        test_model_with("catalog-model", 65_536, ThinkingMode::Switchable).with_provider("acme");
    let mut ctx = RunContext::new(
        "run-context-test",
        TEST_SEED,
        runvil_types::timestamp::Timestamp::UNIX_EPOCH,
    );
    ctx.bind_role_for_test("writer", info.clone());
    let set = bound::bound_model_set(&prompt, ctx.model_bindings());
    let binding = set.binding("writer").expect("the filled role binds");
    assert_eq!(binding.render_info(), info);
}

#[test]
fn a_pack_in_both_roster_lists_records_no_vendor() {
    let web = PackId::parse("web").expect("a static valid Pack id");
    let state = state_over(
        &test_prompt(),
        RunContext::new(
            "run-context-test",
            TEST_SEED,
            runvil_types::timestamp::Timestamp::UNIX_EPOCH,
        )
        .with_packs(
            vec![UsablePack::new(web.clone(), "acme")],
            vec![UnavailablePack::new(web, "the server is down")],
        ),
    );
    let tool = ToolId::parse("web/fetch").expect("a static valid tool id");
    assert_eq!(
        state.tool_set().vendor_for(&tool),
        None,
        "a name in both lists counts as unavailable, so it has no vendor"
    );
}
