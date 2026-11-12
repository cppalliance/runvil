//! Fanout failure semantics on the scheduler. The arms are task chains the
//! `fanout` shim spawns, so their lifecycle reports through the `Task*`
//! observations: started under the caller's section, the terminal under the
//! worker's.

use super::*;
use crate::execute::tests::fixtures::echo_tool;
use crate::execute::tests::models_loop::loop_scenario;

#[test]
fn fanout_empty_collection_errors_before_any_scheduling() {
    // Pin of the pre-scheduling guard: the fanout errors before any arm is
    // created - no STARTED observation, and the worker's store tripwire
    // never fires.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # Fanout\n\n\
        ## Parent\n\n\
        ```lua\ntasks.fanout('Worker', {})\n```\n\n\
        ### Worker\n\n\
        ```lua\nstore.write('ran.txt', 'yes')\n```\n";
    let trace = scheduler_scenario(md).run();

    let error = trace.error();
    assert!(
        error.to_string().contains("empty collection"),
        "error was: {error}"
    );
    assert!(
        trace.read("ran.txt").is_err(),
        "the worker never ran: the rejection precedes scheduling"
    );
    let recorder = recorded(&trace);
    assert_eq!(
        terminal_count(&trace, task_started),
        0,
        "no arm was ever started: {:?}",
        recorder.events()
    );
}

#[test]
fn fanout_worker_that_is_a_list_section_errors() {
    // Pin of the worker-template guard: a resolved list section is not a
    // worker template.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # Fanout\n\n\
        ## Parent\n\n\
        ```lua\ntasks.fanout('Items', {'x'})\n```\n\n\
        ### Items\n\n\
        - a\n\
        - b\n";
    let trace = scheduler_scenario(md).run();

    let error = trace.error();
    assert!(
        error
            .to_string()
            .contains("is a list section, not a worker template"),
        "error was: {error}"
    );
}

/// Alpha and Beta ping-pong calls through nested call chains, and the
/// chain that lands at depth 8 calls fanout, on prompt line 13, where each
/// arm would run one level deeper than the cap allows.
const DEPTH_FANOUT: &str = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
    # Depth\n\n\
    ## Alpha\n\n\
    ```lua\n\
    var.n = (var.n or 0) + 1\n\
    if var.n >= 9 then return tasks.fanout('Worker', {'x'}) end\n\
    return call('../Beta')\n\
    ```\n\n\
    ### Worker\n\n\
    ```lua\nreturn item\n```\n\n\
    ## Beta\n\n\
    ```lua\n\
    var.n = var.n + 1\n\
    return call('../Alpha')\n\
    ```\n";

#[test]
fn fanout_depth_cap_reads_the_chain_field() {
    // Pin of the fanout depth-cap guard: the cap fires from the requesting
    // chain's call-depth field, and the spawn arm answers the arm's spawn
    // with the typed error; the shim re-raises that table and the retained
    // typed error is substituted back at every `call` level, so the run's
    // error is the typed error itself: the exact variant, the exact text,
    // no runtime-error prefix or traceback.
    let trace = scheduler_scenario(DEPTH_FANOUT).run();

    let error = trace.error();
    assert_eq!(
        error,
        &crate::test_support::with_author_prose(
            crate::error::make_call_depth_exceeded_error(8),
            13
        ),
        "expected the typed depth-cap error on Alpha's fanout line"
    );
    assert_eq!(error.code, ErrorCode::CallDepthExceeded, "{error}");
    assert_eq!(
        error.to_string(),
        "recursion exceeded cap of 8",
        "the rendered text is exactly the cap message: no prefix"
    );
}

#[test]
fn an_uncaught_fanout_depth_error_lands_on_the_authors_fanout_line() {
    // The refused spawn answers the fanout's yield, so the error is placed
    // on that yield's call line, and every `call` level above keeps it.
    let trace = scheduler_scenario(DEPTH_FANOUT).run();
    assert_eq!(
        trace.error().at,
        std::num::NonZeroU32::new(13)
            .map(|line| runvil_types::error::SourceLoc::line("prompt", line)),
        "the run's error points at Alpha's `tasks.fanout(...)` line"
    );
}

#[test]
fn an_exhausted_arm_becomes_the_incomplete_stub_and_its_sibling_still_lands() {
    // The `make_tool_loop_exhausted_error` arm rule: one arm's `models.loop` runs its
    // round cap against a never-converging model and fails as
    // `make_tool_loop_exhausted_error`; the shim turns that arm's slot into the
    // incomplete stub (`ok = false`, `exhausted = true`) and the fanout
    // continues, so the sibling's plain result lands beside it. Only the
    // looping arm touches the gateway, so the scripted replies serve one
    // arm and the run is deterministic. The exhausted arm still reports
    // `TaskFailed` (the stub is the fanout's recovery, not the arm's), and
    // the sibling `TaskSucceeded`.
    let cap = 2;
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_0", "echo", "{\"value\":\"x\"}")]),
        calls(&[("call_1", "echo", "{\"value\":\"x\"}")]),
    ]);
    let md = format!(
        "---\nname: t\ndescription: d\nrunvil: 0\nmax_tool_iterations: {cap}\n---\n\n\
        # Fanout\n\n\
        ## Parent\n\n\
        ```lua\n\
        local r = tasks.fanout('Worker', {{'loop', 'plain'}})\n\
        assert(#r == 2, 'both slots are filled')\n\
        assert(r[1].ok == false and r[1].exhausted == true, 'the exhausted arm is flagged')\n\
        assert(r[1].item == 'loop', 'the stub keeps its item')\n\
        assert(r[2].ok == true and r[2].exhausted == false, 'the sibling is a plain success')\n\
        assert(r[2].item == 'plain', 'the sibling keeps its item')\n\
        return r[1].text .. '|' .. r[2].text\n\
        ```\n\n\
        ### Worker\n\n\
        ```lua\n\
        tools.offer('tools/echo')\n\
        if item == 'loop' then\n\
          messages:user('loop forever')\n\
          models.loop()\n\
          return 'unreachable'\n\
        end\n\
        return 'plain:' .. item\n\
        ```\n"
    );
    let trace = loop_scenario(md)
        .offer("echo", echo_tool())
        .chat(&gateway)
        .run();

    assert_eq!(
        trace.text(),
        "## loop\n\nUNKNOWN\n\n(section incomplete: tool loop exhausted)|plain:plain",
        "the exhausted slot is the stub and the sibling's result lands"
    );
    assert_eq!(
        gateway.call_count(),
        cap,
        "the looping arm made exactly `cap` round trips before exhausting"
    );
    let recorder = recorded(&trace);
    assert_eq!(
        terminal_count(&trace, task_started),
        2,
        "both arms started: {:?}",
        recorder.events()
    );
    assert_eq!(
        terminal_count(&trace, task_failed),
        1,
        "the exhausted arm reports its failure: {:?}",
        recorder.events()
    );
    assert_eq!(
        terminal_count(&trace, task_succeeded),
        1,
        "the sibling reports its success: {:?}",
        recorder.events()
    );
    assert_eq!(
        terminal_count(&trace, make_task_canceled_error),
        0,
        "an exhausted arm cancels nothing: {:?}",
        recorder.events()
    );
}

#[test]
fn two_arms_writing_one_path_terminate_the_run_with_a_determinism_violation() {
    // The arms write unordered: whichever op reaches the backend second
    // meets the first's standing write claim, because claims are never
    // released during a run, so the conflict cannot depend on timing.
    // The violation is fatal to the whole run at the answer boundary -
    // the loser never resumes into Lua, so no author pcall can catch it -
    // and every arm still parked when the fatal answer lands drops
    // unarmed, reporting canceled rather than failed.
    //
    // The winner's answer may land before or after the loser's: either
    // interleaving satisfies the contract - both arms started, no arm
    // fails on its own, and at most the winner reports a terminal (the
    // loser is stranded mid-chain by the run's own failure, which is the
    // record of how it ended).
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # Fanout\n\n\
        ## Parent\n\n\
        ```lua\n\
        local r = tasks.fanout('Worker', {'alpha', 'beta'})\n\
        return r[1].text\n\
        ```\n\n\
        ### Worker\n\n\
        ```lua\n\
        store.write('shared.txt', item)\n\
        return item\n\
        ```\n";
    let trace = scheduler_scenario(md).run();

    let error = trace.error();
    assert_eq!(
        error.code,
        ErrorCode::Determinism,
        "expected the fatal determinism violation, got {error:?}"
    );
    let detail = &error.message;
    assert!(detail.contains("shared.txt"), "error was: {detail}");
    assert!(
        detail.contains("conflicts with"),
        "the conflict is named: {detail}"
    );
    assert_eq!(
        detail.matches("ExecId(").count(),
        2,
        "both arms' identities are named: {detail}"
    );
    assert_eq!(
        error.at,
        at_line(19),
        "the violation, which never reaches Lua, is placed on the losing arm's \
         `store.write` line: {error:?}"
    );
    let recorder = recorded(&trace);
    assert_eq!(
        terminal_count(&trace, task_started),
        2,
        "both arms started before the conflict: {:?}",
        recorder.events()
    );
    assert_eq!(
        terminal_count(&trace, task_failed),
        0,
        "no arm fails on its own; the run ends at the answer boundary: {:?}",
        recorder.events()
    );
    assert!(
        terminal_count(&trace, task_succeeded) <= 1,
        "at most the winner (whose answer landed first) reports a terminal: {:?}",
        recorder.events()
    );
}

#[test]
fn two_live_arms_appending_one_path_terminate_with_a_determinism_violation() {
    // The case the WriteScope registry never caught: `append`
    // claims write intent, so two arms appending to one path conflict
    // exactly as two writes do - and under happens-before the conflict is
    // unconditional, because the arms never join each other before the
    // fanout's own rounds deliver them. The conflict is the fatal
    // make_determinism_error violation, not a per-arm store error.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # Fanout\n\n\
        ## Parent\n\n\
        ```lua\n\
        local r = tasks.fanout('Worker', {'alpha', 'beta'})\n\
        return r[1].text\n\
        ```\n\n\
        ### Worker\n\n\
        ```lua\n\
        store.append('evidence.md', item .. '\\n')\n\
        return item\n\
        ```\n";
    let trace = scheduler_scenario(md).run();

    let error = trace.error();
    assert_eq!(
        error.code,
        ErrorCode::Determinism,
        "expected the fatal determinism violation, got {error:?}"
    );
    let detail = &error.message;
    assert!(detail.contains("evidence.md"), "error was: {detail}");
    assert_eq!(
        detail.matches("ExecId(").count(),
        2,
        "both arms' identities are named: {detail}"
    );
}
