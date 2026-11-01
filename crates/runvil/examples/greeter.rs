//! The greeter: one caller loop that parses a prompt, builds its run with
//! a canned model and a shout tool, steps the run, answers every effect it
//! hands back, logs every event, and prints the result.
//!
//! The facade suite includes this file as a module and runs `greet`, so
//! the items it reaches are `pub`.

use std::collections::BTreeMap;
use std::error::Error;
use std::num::NonZeroU32;
use std::sync::Arc;

use runvil::cancel::CancelHandle;
use runvil::effect::{Effect, EffectAnswer, IssuedEffect};
use runvil::event::Event;
use runvil::model::{Completion, CompletionResult, ModelId, ModelInfo, ModelRole, ThinkingMode};
use runvil::timestamp::Timestamp;
use runvil::tools::{ToolCatalog, ToolId, ToolInfo, ToolOutput};
use runvil::vfs::perform_vfs_op;
use runvil::{Prompt, Run, RunContext, RunOutcome, RunSeed, Step};

/// Writes a note, asks the default model to reply, and shouts the reply.
pub const GREETER: &str = concat!(
    "---\n",
    "name: greeter\n",
    "description: Writes a note, asks a model to reply, and shouts the reply.\n",
    "runvil: 0\n",
    "---\n\n",
    "# Greeter\n\n",
    "## Greet\n\n",
    "```lua\n",
    "store.write('note.md', 'hello')\n",
    "local reply = models.infer(store.read('note.md'))\n",
    "return tools.call('example/text/shout', { text = reply })\n",
    "```\n",
);

/// Answers a store effect from the run's store, the model with `hi there`,
/// a render with the offered tools unchanged, the shout tool with the
/// trusted `HI THERE`, and a timer as fired at once. A caller with a real
/// clock waits `seconds` before answering a timer.
#[must_use]
pub fn answer(effect: Effect) -> EffectAnswer {
    match effect {
        Effect::Vfs { access, op } => EffectAnswer::Vfs(perform_vfs_op(&access, op)),
        Effect::Chat { .. } => {
            let reply = CompletionResult::Text("hi there".to_owned());
            EffectAnswer::Chat(Completion::from_result(reply, "canned").map(Box::new))
        }
        Effect::Render { offered, .. } => EffectAnswer::Render(offered),
        Effect::ToolCall { .. } => EffectAnswer::ToolCall(Ok(ToolOutput::trusted("HI THERE"))),
        Effect::Timer { .. } => EffectAnswer::Timer,
    }
}

/// Runs the greeter to its end and returns its text.
///
/// # Errors
///
/// Returns an error when the greeter fails to parse or the run ends
/// without text, as it does when its first step finds that the context
/// cannot satisfy the prompt.
///
/// # Panics
///
/// Panics when the log does not open with the parse events, or when the
/// run was cancelled.
pub fn greet() -> Result<String, Box<dyn Error>> {
    // 1. Parse the greeter, and start the log with its parse events before
    //    checking the result.
    let (parsed, parse_events) = Prompt::parse(GREETER, "greeter");
    let mut log: Vec<Event> = parse_events.clone();
    let prompt = parsed?;

    // 2. Build the context: run events number on from the parse events. A
    //    live caller draws the seed's 32 bytes from a CSPRNG; the example
    //    fixes them.
    let ctx = RunContext::new(
        "greeter",
        RunSeed::from_bytes([7; 32]),
        Timestamp::UNIX_EPOCH,
    )
    .with_provenance_start(u32::try_from(parse_events.len())?);

    // 3. Offer the shout tool and give the canned model the `default`
    //    role. The run's first step checks them against the prompt.
    let shout = ToolInfo::new(
        ToolId::parse("example/text/shout")?,
        "Returns the text in capital letters.",
        serde_json::json!({"type": "object", "properties": {"text": {"type": "string"}}}),
    );
    let catalog = ToolCatalog::new([shout])?;
    let model = ModelInfo::new(
        ModelId::gateway("canned")?,
        "Always replies hi there",
        NonZeroU32::new(8_192).ok_or("a context window is never zero")?,
        ThinkingMode::Never,
    );
    let ctx = ctx
        .with_tools(catalog)
        .with_models(BTreeMap::from([(ModelRole::Default, model)]));

    // 4. Give the context a cancel flag, keep a clone for anything that may
    //    need to stop the run, and create the run.
    let cancel = CancelHandle::new();
    let mut run = Run::new(Arc::new(prompt), "", ctx.with_cancel(cancel.clone()));

    // 5. Step until Done, logging each step's events and answering each
    //    effect exactly once: an effect the step withdraws is dropped
    //    instead of performed.
    let outcome = loop {
        match run.step() {
            Step::Pending {
                effects,
                events,
                withdrawn,
            } => {
                log.extend(events);
                for IssuedEffect { id, effect, .. } in effects {
                    if withdrawn.contains(&id) {
                        run.resume(id, EffectAnswer::Dropped);
                    } else {
                        run.resume(id, answer(effect));
                    }
                }
            }
            Step::Done { outcome, events } => {
                log.extend(events);
                break outcome;
            }
        }
    };

    // 6. The tool's output is the run's final text, and the log opens with
    //    the parse events.
    assert!(log.starts_with(&parse_events));
    assert!(!cancel.is_cancelled());
    let RunOutcome::Completed { final_text } = outcome else {
        return Err(format!("the greeter did not finish: {outcome:?}").into());
    };
    Ok(final_text)
}

fn main() -> Result<(), Box<dyn Error>> {
    let text = greet()?;
    assert_eq!(text, "HI THERE");
    println!("{text}");
    Ok(())
}
