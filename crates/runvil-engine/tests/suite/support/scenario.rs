//! Scenarios: a facade test's setup as data, the [`Driver`] that steps its
//! run, and the [`Trace`] a finished run leaves.
//!
//! The driver answers a store effect with `perform_vfs_op` and a timer as
//! fired at once, a chat effect with the next scripted reply served under
//! the bound model's name, and a tool call with the registered handler. An
//! effect the scenario holds waits. An effect the run withdraws gets
//! `Dropped`, whether the scenario holds it or the withdrawing step issued
//! it; no other effect is ever dropped. The driver fails the test instead
//! of hanging when the chat script runs out, when scripted replies are
//! left over at `Done`, or when a run issues and withdraws nothing while
//! only held effects remain. It also fails a run that reports `Done` while
//! it still holds an effect, because `Done` waits for every issued
//! effect's answer.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use runvil_engine::Error;
use runvil_engine::ToolCatalog;
use runvil_engine::effect::{Effect, EffectAnswer, EffectId, IssuedEffect};
use runvil_engine::event::Event;
use runvil_engine::ids::Provenance;
use runvil_engine::model::{Completion, ModelInfo, ModelRole};
use runvil_engine::timestamp::Timestamp;
use runvil_engine::vfs::perform_vfs_op;
use runvil_engine::{Prompt, Run, RunParams, RunSeed, Step};
use serde_json::{Value, json};

use super::{Reply, TestResult, Tool, canned, tool_info};

type Edit = Box<dyn Fn(RunParams) -> RunParams>;

/// The seed every scenario's run takes.
const TEST_SEED: RunSeed = RunSeed::from_bytes([7; 32]);

/// One test's setup. Every scenario gives the `default` role [`canned`]
/// unless it names a model or a role map, and numbers run events on from
/// the parse events.
#[derive(Default)]
pub(crate) struct Scenario {
    args: String,
    started_at: Timestamp,
    model: Option<ModelInfo>,
    roles: Option<BTreeMap<ModelRole, ModelInfo>>,
    tool: Option<Tool>,
    replies: Vec<Reply>,
    hold: Option<fn(&Effect) -> bool>,
    reverse: bool,
    params: Option<Edit>,
}

impl Scenario {
    /// A scenario that serves `replies` to the run's chat effects, in
    /// order, and registers no tool.
    pub(crate) fn replies(replies: impl IntoIterator<Item = Reply>) -> Self {
        Self {
            replies: replies.into_iter().collect(),
            ..Self::default()
        }
    }

    /// A scenario whose catalog lists `tool` alone, and that serves
    /// `replies` to the run's chat effects, in order.
    pub(crate) fn tool(tool: Tool, replies: impl IntoIterator<Item = Reply>) -> Self {
        Self {
            tool: Some(tool),
            ..Self::replies(replies)
        }
    }

    /// Holds each effect `hold` picks, unanswered, until the run withdraws
    /// it.
    pub(crate) fn hold(mut self, hold: fn(&Effect) -> bool) -> Self {
        self.hold = Some(hold);
        self
    }

    /// Runs the prompt with `args` as its argument text.
    pub(crate) fn args(mut self, args: &str) -> Self {
        args.clone_into(&mut self.args);
        self
    }

    /// Starts the run at `started_at`, which the prompt reads as `sys.when`.
    pub(crate) fn started_at(mut self, started_at: Timestamp) -> Self {
        self.started_at = started_at;
        self
    }

    /// Sets `model` as the `default` role's model, in place of [`canned`].
    pub(crate) fn model(mut self, model: ModelInfo) -> Self {
        self.model = Some(model);
        self
    }

    /// Sets the whole role map the run fills the roles from, in place of the
    /// one-model map [`model`](Self::model) and [`canned`] make. The map may
    /// leave `default` out.
    pub(crate) fn roles(mut self, roles: impl IntoIterator<Item = (ModelRole, ModelInfo)>) -> Self {
        self.roles = Some(roles.into_iter().collect());
        self
    }

    /// Answers each batch of effects in reverse issue order.
    pub(crate) fn reverse(mut self) -> Self {
        self.reverse = true;
        self
    }

    /// Edits the run's params after the scenario sets its own, so an edit
    /// can also undo one of them.
    pub(crate) fn params(mut self, edit: impl Fn(RunParams) -> RunParams + 'static) -> Self {
        self.params = Some(Box::new(edit));
        self
    }

    /// Parses `source` and builds its params.
    pub(crate) fn build(&self, source: &str) -> TestResult<Built> {
        let (parsed, events) = Prompt::parse(&|_: &str| Ok(source.to_owned()), "prompt");
        let prompt = parsed?;
        let roles = match (&self.roles, &self.model) {
            (Some(roles), _) => roles.clone(),
            (None, Some(model)) => BTreeMap::from([(ModelRole::Default, model.clone())]),
            (None, None) => BTreeMap::from([(ModelRole::Default, canned()?)]),
        };
        let mut ctx = RunParams::new(TEST_SEED, self.started_at)
            .with_provenance_start(u32::try_from(events.len())?)
            .with_models(roles);
        if let Some(tool) = &self.tool {
            let info = tool_info(tool.id, json!({ "type": "object" }))?;
            ctx = ctx.with_tools(ToolCatalog::new([info])?);
        }
        if let Some(edit) = &self.params {
            ctx = edit(ctx);
        }
        Ok(Built {
            prompt,
            ctx,
            events,
        })
    }

    /// Builds `source`'s params and creates its run, not yet stepped.
    pub(crate) fn start(self, source: &str) -> TestResult<Driver> {
        let Built {
            prompt,
            ctx,
            events,
        } = self.build(source)?;
        Ok(Driver {
            run: Run::new(Arc::new(prompt), &self.args, ctx),
            replies: self.replies.into(),
            tool: self.tool,
            hold: self.hold,
            reverse: self.reverse,
            held: Vec::new(),
            events,
            effects: Vec::new(),
            result: None,
        })
    }

    pub(crate) fn run(self, source: &str) -> TestResult<Trace> {
        self.start(source)?.finish()
    }
}

pub(crate) struct Built {
    pub(crate) prompt: Prompt,
    pub(crate) ctx: RunParams,
    /// The parse events.
    pub(crate) events: Vec<Event>,
}

/// A started run, stepped by hand for a test that acts mid-run.
pub(crate) struct Driver {
    pub(crate) run: Run,
    replies: VecDeque<Reply>,
    tool: Option<Tool>,
    hold: Option<fn(&Effect) -> bool>,
    reverse: bool,
    held: Vec<Issued>,
    events: Vec<Event>,
    effects: Vec<Logged>,
    result: Option<Result<String, Error>>,
}

struct Issued {
    id: EffectId,
    provenance: Provenance,
    effect: Value,
}

impl Driver {
    /// Steps the run once, answers or holds each effect the step issued,
    /// and drops each effect the step withdrew. Returns how many it issued.
    pub(crate) fn pump(&mut self) -> TestResult<usize> {
        if self.result.is_some() {
            return Err("the run is done, so there is nothing left to step".into());
        }
        let (mut effects, withdrawn) = match self.run.step() {
            Step::Pending {
                effects,
                events,
                withdrawn,
                ..
            } => {
                self.events.extend(events);
                (effects, withdrawn)
            }
            Step::Done { result, events, .. } => {
                self.events.extend(events);
                let held = self.held.len();
                if held > 0 {
                    return Err(format!("the run ended with {held} held effects unanswered").into());
                }
                let left = self.replies.len();
                if left > 0 {
                    return Err(format!("the run ended with {left} scripted replies left").into());
                }
                self.result = Some(result);
                return Ok(0);
            }
        };
        if self.reverse {
            effects.reverse();
        }
        let issued = effects.len();
        for IssuedEffect {
            id,
            provenance,
            effect,
            ..
        } in effects
        {
            let effect_record = effect.record();
            let held = Issued {
                id,
                provenance,
                effect: effect_record,
            };
            if withdrawn.contains(&id) {
                self.respond(held, EffectAnswer::Dropped);
            } else if self.hold.is_some_and(|hold| hold(&effect)) {
                self.held.push(held);
            } else {
                let answer = self.answer(effect)?;
                self.respond(held, answer);
            }
        }
        let (dropped, held): (Vec<_>, Vec<_>) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(|issued| withdrawn.contains(&issued.id));
        self.held = held;
        for issued in dropped {
            self.respond(issued, EffectAnswer::Dropped);
        }
        Ok(issued)
    }

    /// How many effects the driver holds unanswered.
    pub(crate) fn held(&self) -> usize {
        self.held.len()
    }

    fn respond(&mut self, issued: Issued, answer: EffectAnswer) {
        self.effects.push(Logged {
            provenance: issued.provenance,
            effect: issued.effect,
            answer: answer.record(),
        });
        self.run.resume(issued.id, answer);
    }

    pub(crate) fn pump_until_held(&mut self) -> TestResult {
        while self.held.is_empty() {
            self.advance()?;
            if self.result.is_some() {
                return Err("the run finished without issuing an effect to hold".into());
            }
        }
        Ok(())
    }

    pub(crate) fn finish(mut self) -> TestResult<Trace> {
        loop {
            if let Some(result) = self.result.take() {
                return Ok(Trace {
                    result,
                    events: self.events,
                    effects: self.effects,
                });
            }
            self.advance()?;
        }
    }

    /// Pumps once, and fails when another step could only wait forever. A
    /// step that issues nothing can only withdraw held effects, so an
    /// unchanged count means it withdrew nothing.
    fn advance(&mut self) -> TestResult {
        let held = self.held.len();
        let issued = self.pump()?;
        if self.result.is_none() && issued == 0 && self.held.len() == held {
            return Err(format!(
                "the run issued and withdrew nothing while only {held} held effects remain"
            )
            .into());
        }
        Ok(())
    }

    fn answer(&mut self, effect: Effect) -> TestResult<EffectAnswer> {
        Ok(match effect {
            Effect::Vfs { access, op } => EffectAnswer::Vfs(perform_vfs_op(&access, op)),
            Effect::Timer { .. } => EffectAnswer::Timer,
            Effect::Render { offered, .. } => EffectAnswer::Render(offered),
            Effect::Chat { binding, .. } => {
                let reply = self
                    .replies
                    .pop_front()
                    .ok_or("the run asked for a model reply after the chat script ran out")?;
                let served = binding.id().name();
                let completion = match reply {
                    Reply::Completion(result) => Completion::from_result(result, served),
                    Reply::Failure(error) => Err(error),
                };
                EffectAnswer::Chat(completion.map(Box::new))
            }
            Effect::ToolCall { tool, args, .. } => match &self.tool {
                Some(registered) if tool.to_string() == registered.id => {
                    EffectAnswer::ToolCall((registered.handler)(&args))
                }
                _ => {
                    let error = format!(
                        "the run called {tool}, and the scenario registers no handler for it"
                    );
                    return Err(error.into());
                }
            },
        })
    }
}

pub(crate) struct Trace {
    /// The run's final text, or the error that ended it.
    pub(crate) result: Result<String, Error>,
    /// The parse events, then the run's events, in report order.
    pub(crate) events: Vec<Event>,
    /// Every effect the run issued, in the order the driver answered them.
    pub(crate) effects: Vec<Logged>,
}

impl Trace {
    /// The text of a run that succeeded. Fails on the error a run ended
    /// with.
    pub(crate) fn text(&self) -> TestResult<&str> {
        match &self.result {
            Ok(text) => Ok(text),
            Err(error) => Err(format!("expected the run's text, got {error:?}").into()),
        }
    }

    /// The error a run ended with: a failure, or a cancel. Fails on the
    /// text of a run that succeeded.
    pub(crate) fn error(&self) -> TestResult<&Error> {
        match &self.result {
            Err(error) => Ok(error),
            Ok(text) => Err(format!("expected the run to end with an error, got {text:?}").into()),
        }
    }
}

pub(crate) struct Logged {
    pub(crate) provenance: Provenance,
    pub(crate) effect: Value,
    pub(crate) answer: Value,
}

impl Logged {
    /// The effect record's body when the effect is of `kind`, such as
    /// `Chat` or `ToolCall`.
    pub(crate) fn effect_of(&self, kind: &str) -> Option<&Value> {
        self.effect.get(kind)
    }
}
