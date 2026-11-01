//! Tests for `ChainState` construction, forking, task sequence seeding,
//! and the model set a run binds from its filled roles.

use std::collections::BTreeMap;

use runvil_types::packs::PackId;
use runvil_types::tools::ToolId;

use serde_json::json;

use super::*;
use crate::execute::section_vm::VmSeed;
use crate::lua::{Argv, RosterEntry};
use crate::model::{ModelRole, ThinkingMode};
use crate::test_support::fresh_access;
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

fn test_state(prompt: &Prompt) -> ChainState {
    state_over(
        prompt,
        RunConfig::new(
            "run-context-test",
            TEST_SEED,
            runvil_types::timestamp::Timestamp::UNIX_EPOCH,
        ),
    )
}

/// The state a run of `prompt` assembles from `config`, as `Run::new`
/// assembles it.
fn state_over(prompt: &Prompt, config: RunConfig) -> ChainState {
    ChainState::root(Arc::new(prompt.clone()), "", config)
}

#[test]
fn root_builds_a_context_over_the_prompt() {
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
    assert!(Arc::ptr_eq(ctx.prompt(), clone.prompt()));
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

#[test]
fn vm_setup_hands_the_vm_the_runs_argv_in_the_mode_the_caller_asks_for() {
    let prompt = test_prompt();
    let ctx = test_state(&prompt).with_args("{\"query\":\"papers\"}");
    let sys = json!({});
    let access = fresh_access();
    let requested = ctx.argv().cloned();
    let expected = requested.as_deref();
    assert!(
        expected.is_some(),
        "the fixture's args must derive an argv, or neither mode is observable"
    );
    let selection = Arc::new(Mutex::new(None));
    let frozen = ctx.vm_setup(&sys, VmSeed::default(), &access, &selection, "Only", false);
    assert!(
        matches!(frozen.argv, Argv::Frozen(value) if value == expected),
        "an unprivileged constructor must get the run's argv frozen: {:?}",
        frozen.argv
    );
    let writable = ctx.vm_setup(&sys, VmSeed::default(), &access, &selection, "Only", true);
    assert!(
        matches!(writable.argv, Argv::Writable(value) if value == expected),
        "the H1 pass's flag must get the run's argv writable: {:?}",
        writable.argv
    );
}

#[test]
fn forks_swap_only_their_own_fields() {
    let ctx = test_state(&test_prompt());
    let chain = ctx.with_args("chain-args");
    assert_eq!(&**chain.args(), "chain-args");
    assert!(Arc::ptr_eq(&ctx.shared, &chain.shared));
    assert_eq!(&**ctx.args(), "");

    let turns = Arc::new(AtomicU32::new(7));
    let task: TaskId = "0.4".parse().expect("a task id parses");
    let arm = ctx.with_task(task.clone(), Arc::clone(&turns));
    assert!(Arc::ptr_eq(arm.turns(), &turns));
    assert!(Arc::ptr_eq(&ctx.shared, &arm.shared));
    assert_eq!(arm.emitter().task(), &task);
    assert_eq!(ctx.emitter().task(), &TaskId::from(ChainId::root()));
}

#[test]
fn the_root_task_sequence_starts_where_the_config_says() {
    let ctx = state_over(
        &test_prompt(),
        RunConfig::new(
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
    let events = arm.take_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].provenance().task, TaskId::from(ChainId::root()));
    assert_eq!(events[1].provenance().task, task);
    assert_eq!(events[1].provenance().seq, 0);
}

#[test]
fn a_filled_role_binds_with_the_catalog_info_its_render_receives() {
    let prompt = test_prompt();
    let info =
        test_model_with("catalog-model", 65_536, ThinkingMode::Switchable).with_provider("acme");
    let ctx = RunConfig::new(
        "run-context-test",
        TEST_SEED,
        runvil_types::timestamp::Timestamp::UNIX_EPOCH,
    )
    .with_models(BTreeMap::from([(ModelRole::Default, info.clone())]));
    let state = state_over(&prompt, ctx);
    let set = state.model_set();
    let binding = set.binding("default").expect("the filled role binds");
    assert_eq!(binding.render_info(), info);
    assert_eq!(state.refusal(), None, "the prompt requires nothing");
}

#[test]
fn an_unavailable_roster_pack_records_no_vendor_and_a_pack_listed_twice_keeps_its_last_entry() {
    let web = PackId::parse("web").expect("a static valid Pack id");
    let usable = RosterEntry::Usable {
        vendor: "acme".to_owned(),
    };
    let down = RosterEntry::Unavailable {
        reason: "the server is down".to_owned(),
    };
    let state = state_over(
        &test_prompt(),
        RunConfig::new(
            "run-context-test",
            TEST_SEED,
            runvil_types::timestamp::Timestamp::UNIX_EPOCH,
        )
        .with_packs([(web.clone(), usable), (web.clone(), down.clone())]),
    );
    let tool = ToolId::parse("web/fetch").expect("a static valid tool id");
    assert_eq!(
        state.tool_set().vendor_for(&tool),
        None,
        "an unavailable Pack has no vendor"
    );
    assert_eq!(
        state.tool_set().roster_for_test().get(&web),
        Some(&down),
        "one roster entry per Pack, the last one given, carrying its reason"
    );
}
