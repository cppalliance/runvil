//! Unit tests for section execution, tool scoping, and the tool-call loop.

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use runvil_lua::LuaProgram;
use runvil_model_client::client::Role;
use runvil_types::tools::{ToolError, ToolErrorKind, ToolId, ToolOutput};
use runvil_types::untrusted::GuardNonce;
use runvil_vfs::{Access, VfsError};
use serde_json::{Value, json};

use self::context::setup_prompt;
use self::fixtures::untrusted_echo_tool;
use super::turn::advance_turn;
use super::*;
use crate::Error;
use crate::lua::{SectionVm, current_static_set};
use crate::model::{ModelInfo, ModelSet};
use crate::parser::ParseErrorKind;
use crate::parser::Prompt;
use crate::test_support::recording::{Observation, Observer, detail, null_emitter};
use crate::test_support::scenario::{Scenario, Trace};
use crate::test_support::scripted_chat::{
    MOCK_MODEL, ScriptedCall, ScriptedChat, ScriptedReply, calls, fail, text, text_finish,
};
use crate::test_support::seeded::Seeded;
use crate::test_support::{FnTool, fresh_access};

mod context;
mod fixtures;
mod gateway;

/// A tool whose calls the scenario holds, so the test can prove the
/// tool-call loop honors cancellation mid-call rather than waiting the call
/// out, and a task parked on it stays live until its owner ends or cancels
/// it. The closure never runs.
#[track_caller]
fn slow_tool() -> FnTool {
    FnTool::new("test/tools/slow", json!({ "type": "object" }), |_| {
        panic!("a held call never reaches the slow tool")
    })
    .description("a deliberately slow tool")
}

/// Whether `effect` is a call to the slow tool, which a scenario holds.
fn calls_slow_tool(effect: &Effect) -> bool {
    matches!(effect, Effect::ToolCall { tool, .. } if tool.to_string() == "test/tools/slow")
}

#[test]
fn run_with_a_pre_cancelled_handle_fails_as_cancelled() {
    use crate::cancel::CancelHandle;
    use runvil_parser::test_support::prompt;

    // The explicit-cancel wiring of the public entry point: a handle passed
    // through `RunContext::with_cancel` is installed around the whole run body, so
    // the section's Lua instruction hook observes it and the run ends
    // cancelled.
    let source = prompt("", &[("Loop", "local n = 0\nwhile true do n = n + 1 end")]);
    let handle = CancelHandle::new();
    handle.cancel();
    let trace = Scenario::new(source)
        .context(move |ctx| ctx.with_cancel(handle))
        .run();
    assert!(
        matches!(trace.result(), RunResult::Cancelled),
        "a pre-cancelled handle must end the run cancelled, got {:?}",
        trace.result()
    );
}

// --- Guard-wrapping of untrusted tool results in the loop ---

/// The content of the first `tool`-role message in the last recorded round.
///
/// The second round the loop sends includes the dispatched tool's result;
/// this pulls that result string back out so a test can assert on it.
fn last_tool_turn_content(bodies: &[ScriptedCall]) -> String {
    let last = bodies.last().expect("the loop must send a second request");
    last.messages_json()
        .as_array()
        .expect("a round's messages serialize to an array")
        .iter()
        .find(|m| m["role"] == Role::Tool.as_str())
        .expect("the re-sent conversation must include the tool turn")["content"]
        .as_str()
        .expect("a tool turn's content must be a string")
        .to_string()
}

/// Extracts the guard-tag nonce from every `tool`-role turn in the last
/// recorded round.
fn tool_turn_nonces(bodies: &[ScriptedCall]) -> Vec<String> {
    let last = bodies.last().expect("the loop must send a final request");
    last.messages_json()
        .as_array()
        .expect("a round's messages serialize to an array")
        .iter()
        .filter(|m| m["role"] == Role::Tool.as_str())
        .filter_map(|m| m["content"].as_str())
        .filter_map(|content| {
            let marker = "<untrusted_input_";
            let start = content.find(marker)? + marker.len();
            let rest = &content[start..];
            let end = rest.find('>')?;
            Some(rest[..end].to_string())
        })
        .collect()
}

#[test]
fn untrusted_nonce_differs_across_runs_under_different_seeds() {
    use runvil_parser::test_support::PromptSource;

    // The nonce is the run seed's: two runs of the same prompt under
    // different caller-drawn seeds wrap the same untrusted tool result under
    // different nonces, so an envelope's tag stays unguessable from one run
    // to the next as long as the caller draws each seed afresh. (Under one
    // seed the two runs agree byte for byte, which `run_inputs` pins.)
    let source = setup_prompt(PromptSource::new().front("uses: [tools]").section(
        "Only",
        "return tools.call('tools/untrusted_echo', { value = 'hi' })",
    ));
    let mut rng = Seeded::new(0x5EED);
    let mut run_nonces = Vec::new();
    for seed in [rng.run_seed(), rng.run_seed()] {
        let trace = Scenario::new(&source)
            .seed(seed)
            .bound()
            .tool(untrusted_echo_tool())
            .run();
        let out = trace.text();
        let marker = "<untrusted_input_";
        let start = out.find(marker).expect("the result is guard-wrapped") + marker.len();
        let end = out[start..]
            .find('>')
            .map(|end| start + end)
            .expect("the guard tag closes");
        run_nonces.push(out[start..end].to_string());
    }
    rng.check(
        run_nonces[0] != run_nonces[1],
        "each run must mint its own nonce",
    );
}

// --- Progress reporting ---

/// The two-section fixture the fall-through test uses: the first section
/// falls through, the second returns from Lua.
const TWO_SECTIONS: &str = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
# Test prompt\n\n\
## First\n\n```lua\nlocal x = 1\n```\n\n\
## Second\n\n```lua\nreturn \"second\"\n```\n";

const STORE_SECTIONS: &str = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
# Test prompt\n\n\
## First\n\n```lua\nstore.write('state.txt', 'first')\n```\n\n\
## Second\n\n```lua\nstore.append('state.txt', '\\nsecond')\nreturn \"second\"\n```\n";

mod adhoc_tools;
mod batch_turn;
mod chat_arm;
mod chat_record_rebuild;
mod chat_scope;
mod compaction;
mod debug_and_counts;
mod discovery;
mod effects;
mod event_vendors;
mod exec_flow;
mod exit_rules;
mod fanout_acceptance;
mod full_id_calls;
mod happens_before;
mod live_infer;
mod model_and_reply;
mod models_compact;
mod models_loop;
mod models_loop_compactors;
mod models_loop_contract;
mod observations;
mod offering;
mod parallel_tool_calls;
mod precheck_anchor;
mod preludes;
mod provenance;
mod render_effect;
mod rendered_tools;
mod rounds;
mod run_inputs;
mod run_termination;
mod scheduler;
mod section_messages;
mod serial_driver;
mod suite;
mod tasks;
mod timeouts;
mod tool_call_access;
mod tool_call_arm;
mod tool_call_origins;
mod tool_listings;
mod tool_loop;
mod tool_scoping;
mod unified_pipeline;
mod waits;
