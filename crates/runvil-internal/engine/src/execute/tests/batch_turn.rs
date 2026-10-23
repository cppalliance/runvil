//! Tests for the turn a model-issued tool result reports: every result in
//! a batch carries the turn of the round that requested the batch, even
//! when an earlier adhoc handler in that batch runs model rounds of its
//! own - through `models.infer` or a `call` into a section that runs one -
//! and advances the chain's turn counter before the later calls dispatch.
//! Bound tools and adhoc tools both report the requesting round's turn.
//! A handler or a compactor cannot start its own loop while a loop runs in
//! the section; once no loop runs, the next one starts normally.

use runvil_parser::test_support::{one_section, prompt};
use runvil_types::event::Event;

use super::fixtures::echo_tool;
use super::models_loop::loop_scenario;
use super::*;
use crate::test_support::scenario::Trace;

/// One turn-bearing report: a requested batch (its call ids joined by
/// commas), a tool result (its call id), or an assistant reply (its text).
#[derive(Debug, Clone, PartialEq, Eq)]
enum TurnReport {
    Batch { turn: u32, ids: String },
    Result { turn: u32, id: String },
    Reply { turn: u32, text: String },
}

/// The turn on every batch, result, and reply a run reported, in order.
#[derive(Debug)]
struct Turns(Vec<TurnReport>);

impl Turns {
    /// The turn-bearing reports among `trace`'s events.
    fn of(trace: &Trace) -> Turns {
        let reports = trace.events().iter().filter_map(|event| match event {
            Event::AssistantToolCalls { turn, calls, .. } => {
                let ids: Vec<&str> = calls.iter().map(|call| call.id.as_str()).collect();
                Some(TurnReport::Batch {
                    turn: *turn,
                    ids: ids.join(","),
                })
            }
            Event::ToolResult {
                turn, tool_call_id, ..
            } => Some(TurnReport::Result {
                turn: *turn,
                id: tool_call_id.clone(),
            }),
            Event::AssistantReply { turn, text, .. } => Some(TurnReport::Reply {
                turn: *turn,
                text: text.clone(),
            }),
            _ => None,
        });
        Turns(reports.collect())
    }

    /// The turn of the batch whose ids are exactly `ids`.
    #[track_caller]
    fn batch_turn(&self, ids: &str) -> u32 {
        self.0
            .iter()
            .find_map(|report| match report {
                TurnReport::Batch { turn, ids: seen } if seen == ids => Some(*turn),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no batch {ids} in {:?}", self.0))
    }

    /// The turn of the one result reported under `id`.
    #[track_caller]
    fn result_turn(&self, id: &str) -> u32 {
        let turns: Vec<u32> = self
            .0
            .iter()
            .filter_map(|report| match report {
                TurnReport::Result { turn, id: seen } if seen == id => Some(*turn),
                _ => None,
            })
            .collect();
        assert_eq!(turns.len(), 1, "one result under {id}: {:?}", self.0);
        turns[0]
    }

    /// The turn of the reply whose text is `text`.
    #[track_caller]
    fn reply_turn(&self, text: &str) -> u32 {
        self.0
            .iter()
            .find_map(|report| match report {
                TurnReport::Reply { turn, text: seen } if seen == text => Some(*turn),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no reply {text:?} in {:?}", self.0))
    }
}

/// The block every test here runs: registers `grab` with `handler` as its
/// body, then one loop over a single user message, returning the final
/// record's text. `setup` runs first.
fn grab_block(setup: &str, handler: &str) -> String {
    format!(
        "{setup}\
         tools.offer_local('grab', 'Grab a value', {{ value = 'string' }}, function(args)\n\
           {handler}\n\
         end)\n\
         messages:user('Use the tools.')\n\
         models.loop()\n\
         return messages[#messages].content"
    )
}

/// The `grab` handler body that runs one inference round.
const INFER_HANDLER: &str = "return 'inferred ' .. models.infer('inner prompt')";

/// The [`grab_block`] setup that puts the `echo` fixture in scope.
const OFFER_ECHO: &str = "tools.offer('tools/echo')\n";

/// The refusal of a loop started while another runs in the section.
const LOOP_RUNNING: &str = "a loop or a compactor is already running in this section; \
                            ask a side question with models.infer(prompt)";

/// Lua text defining `run_loop(text, compactor)`, which appends the user
/// message `text` to the section's list, runs `models.loop` under
/// `compactor`, and returns the last record's text, followed by `body`.
fn with_run_loop(body: &str) -> String {
    format!(
        "local function run_loop(text, compactor)\n\
           messages:user(text)\n\
           models.loop(compactor)\n\
           return messages[#messages].content\n\
         end\n\
         {body}"
    )
}

/// Drives `scenario` against `replies`, returning the run's output and its
/// turn reports.
#[track_caller]
fn run_turns(scenario: Scenario, replies: Vec<ScriptedReply>) -> (String, Turns) {
    let gateway = ScriptedChat::new(replies);
    let trace = scenario.chat(&gateway).run();
    (trace.text().to_owned(), Turns::of(&trace))
}

#[test]
fn a_later_bound_result_reports_the_batch_turn_after_a_handler_infers() {
    let (out, turns) = run_turns(
        loop_scenario(one_section(&grab_block(OFFER_ECHO, INFER_HANDLER)))
            .offer("echo", echo_tool()),
        vec![
            calls(&[
                ("c1", "grab", "{\"value\":\"a\"}"),
                ("c2", "echo", "{\"value\":\"b\"}"),
            ]),
            text("inner reply"),
            text("final answer"),
        ],
    );
    assert_eq!(out, "final answer");
    let batch = turns.batch_turn("c1,c2");
    assert_eq!(
        turns.reply_turn("inner reply"),
        batch + 1,
        "the handler's inference advanced the counter mid-batch: {turns:?}"
    );
    assert_eq!(turns.result_turn("c1"), batch, "the handler's own result");
    assert_eq!(
        turns.result_turn("c2"),
        batch,
        "the bound call dispatched after the handler reports the batch turn: {turns:?}"
    );
}

#[test]
fn a_handlers_loop_during_a_loop_raises_loop_running_as_a_lua_error() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("c1", "grab", "{\"value\":\"a\"}")]),
        text("final answer"),
    ]);
    let source = one_section(&with_run_loop(
        "local refusal\n\
         tools.offer_local('grab', 'Grab a value', { value = 'string' }, function()\n\
           local ok, err = pcall(models.loop)\n\
           refusal = tostring(ok) .. '|' .. err.kind .. '|' .. tostring(err)\n\
           return 'refused'\n\
         end)\n\
         local final = run_loop('Use the tools.')\n\
         return refusal .. '|' .. final",
    ));
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(
        trace.text(),
        format!("false|lua|{LOOP_RUNNING}|final answer")
    );
    assert_eq!(
        gateway.call_count(),
        2,
        "the refused loop sends nothing and the outer loop goes on"
    );
}

#[test]
fn an_overflow_compactors_loop_raises_loop_running() {
    let gateway = ScriptedChat::new(vec![text("unreachable")]);
    let source = one_section(&with_run_loop(
        "local ok, err = pcall(run_loop, string.rep('x', 100000), function()\n\
           models.loop()\n\
         end)\n\
         return tostring(ok) .. '|' .. err.kind .. '|' .. tostring(err)",
    ));
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), format!("false|lua|{LOOP_RUNNING}"));
    assert_eq!(
        gateway.call_count(),
        0,
        "the precheck refuses the outer round and the compactor's loop never starts"
    );
}

#[test]
fn a_loop_after_an_earlier_loop_returned_runs_normally() {
    let gateway = ScriptedChat::new(vec![text("first"), text("second")]);
    let source = one_section(&with_run_loop(
        "return run_loop('one') .. '|' .. run_loop('two')",
    ));
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "first|second");
    assert_eq!(gateway.call_count(), 2);
}

#[test]
fn a_loop_after_an_earlier_loop_raised_under_pcall_runs_normally() {
    // The section's list is still empty at the first loop, so its round is
    // refused and the loop raises without leaving a record behind.
    let gateway = ScriptedChat::new(vec![text("after")]);
    let source = one_section(&with_run_loop(
        "local ok, err = pcall(models.loop)\n\
         return tostring(ok) .. '|' .. err.kind .. '|' .. run_loop('small')",
    ));
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "false|lua|after");
    assert_eq!(
        gateway.call_count(),
        1,
        "only the loop after the raise sends"
    );
}

#[test]
fn a_handlers_loop_runs_normally_when_tools_call_reaches_it_outside_any_loop() {
    let gateway = ScriptedChat::new(vec![text("inner reply")]);
    let source = one_section(&with_run_loop(
        "tools.offer_local('grab', 'Grab a value', { value = 'string' }, function(args)\n\
           return run_loop(args.value)\n\
         end)\n\
         return tools.call('grab', { value = 'inner' })",
    ));
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "inner reply");
    assert_eq!(gateway.call_count(), 1);
}

#[test]
fn a_later_result_keeps_the_batch_turn_after_a_handler_calls_an_inferring_section() {
    let source = prompt(
        "",
        &[
            ("Only", &grab_block(OFFER_ECHO, "return call('## Helper')")),
            ("Helper", "return models.infer('helper prompt')"),
        ],
    );
    let (out, turns) = run_turns(
        loop_scenario(source).offer("echo", echo_tool()),
        vec![
            calls(&[
                ("c1", "grab", "{\"value\":\"a\"}"),
                ("c2", "echo", "{\"value\":\"b\"}"),
            ]),
            text("helper reply"),
            text("final answer"),
        ],
    );
    assert_eq!(out, "final answer");
    let batch = turns.batch_turn("c1,c2");
    assert_eq!(
        turns.reply_turn("helper reply"),
        batch + 1,
        "the called section shares its caller's counter: {turns:?}"
    );
    assert_eq!(
        turns.result_turn("c2"),
        batch,
        "the call dispatched after the handler reports the batch turn: {turns:?}"
    );
}
