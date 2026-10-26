//! A Pack's render under the Harness, through the render Pack, whose
//! render renames each offered tool and adds one tool only the model sees,
//! both named after the round's model: Lua neither calls the model-only
//! tool nor reads its count by id. Each section entry's render and the
//! model's calls of both rendered tools sit in the `entries` child module;
//! the calls' events, a model-only tool in the Dynamic Set, and a stop
//! between two entries' renders in the `calls` child module.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use harness_runner::recorder::{MemoryRecorder, Record, RecordKind, RunOutcome};
use harness_runner::{Harness, HostContext};
use runvil::model::{Completion, CompletionResult, ToolCall};
use runvil_plugin::{
    HostServices, ModelInfo, Pack, PackFuture, PackId, PackInfo, PackRegistry, RenderContext,
    ServiceKey, ToolContext, ToolError, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use crate::harness::request;
use crate::scripted::{MODEL, ScriptedBroker, reply, scripted_model};
use crate::support::PendingTimer;

#[path = "render-calls.rs"]
mod calls;
#[path = "render-entries.rs"]
mod entries;

/// The render Pack, whose Pack id is `render`: one catalog tool,
/// [`ECHO`], and a render that names each offered tool `shout_<model>`
/// and adds [`SECRET`] as `secret_<model>`, after the name of the round's
/// model.
const RENDERER: PackInfo = PackInfo::new("tests/render", construct);

/// The render Pack's one catalog tool.
const ECHO: &str = "render/echo";

/// The tool only the model sees, which every render adds.
const SECRET: &str = "render/secret";

/// The Host-wide service the render Pack's construct reads its [`Seen`]
/// from.
const SEEN: ServiceKey<Seen> = ServiceKey::new("tests/render-seen");

/// The frontmatter and H1 every prompt here shares: the render Pack
/// declared and one role bound as the default.
const PREAMBLE: &str = "---\nname: renders\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/render\nmodels:\n  writer: {}\n---\n\n\
    # Renders\n\n```lua\nmodels.default('writer')\n```\n\n";

/// One render as the render Pack saw it.
#[derive(Debug, Clone, PartialEq)]
struct Rendered {
    entry: String,
    model: ModelInfo,
    offered: Vec<String>,
}

/// One call as the render Pack saw it: the called tool, and the entry and
/// the model its origin names.
#[derive(Debug, Clone, PartialEq)]
struct Called {
    tool: String,
    entry: Option<String>,
    model: Option<String>,
}

impl Called {
    /// A call to `tool` that the round on `model` asked for in `entry`.
    fn new(tool: &str, entry: &str, model: &str) -> Called {
        Called {
            tool: tool.to_owned(),
            entry: Some(entry.to_owned()),
            model: Some(model.to_owned()),
        }
    }
}

/// What the render Pack saw, shared with the test that reads it, and the
/// gate its held calls wait on.
struct Seen {
    renders: Mutex<Vec<Rendered>>,
    calls: Mutex<Vec<Called>>,
    /// The model of each held call torn down before it answered.
    dropped: Mutex<Vec<String>>,
    /// Each permit lets one held call answer.
    gate: Semaphore,
}

impl Seen {
    fn new() -> Arc<Seen> {
        Arc::new(Seen {
            renders: Mutex::default(),
            calls: Mutex::default(),
            dropped: Mutex::default(),
            gate: Semaphore::new(0),
        })
    }

    fn renders(&self) -> Vec<Rendered> {
        self.renders.lock().unwrap().clone()
    }

    fn calls(&self) -> Vec<Called> {
        self.calls.lock().unwrap().clone()
    }

    fn dropped(&self) -> Vec<String> {
        self.dropped.lock().unwrap().clone()
    }

    /// Lets one held call answer.
    fn open(&self) {
        self.gate.add_permits(1);
    }
}

/// The render Pack's object.
struct RenderPack {
    seen: Arc<Seen>,
    /// The names of the models whose render marks [`SECRET`] to survive
    /// a stop.
    surviving: Vec<String>,
    /// Whether every render panics.
    panics: bool,
}

/// Builds the render Pack over the Host's [`Seen`] from a configuration
/// whose `surviving` lists model names and whose `panics` makes every
/// render panic.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the PackInfo construct signature fixes the argument types"
)]
fn construct(
    _name: &PackId,
    config: Value,
    services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    let seen = services
        .get(&SEEN)
        .ok_or_else(|| ToolError::message("the render Pack needs tests/render-seen"))?;
    let surviving = config["surviving"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    Ok(Arc::new(RenderPack {
        seen,
        surviving,
        panics: config["panics"] == true,
    }))
}

/// A render Pack tool under `id` whose arguments are any object.
fn tool(id: &str, description: &str) -> ToolInfo {
    ToolInfo::new(
        ToolId::parse(id).expect("a fixture tool id"),
        description,
        json!({ "type": "object" }),
    )
}

/// The render Pack's one catalog tool, as its tool list holds it.
fn echo() -> ToolInfo {
    tool(ECHO, "Echo the value.")
}

/// What the render Pack shows the model `model` for `offered`: each
/// offered tool as `shout_<model>`, then [`SECRET`] as `secret_<model>`,
/// which survives a stop when `surviving` is set.
fn shown(offered: Vec<ToolInfo>, model: &str, surviving: bool) -> Vec<ToolInfo> {
    let mut shown: Vec<ToolInfo> = offered
        .into_iter()
        .map(|tool| tool.wire_name(format!("shout_{model}")))
        .collect();
    shown.push(
        tool(SECRET, "Answer only the model.")
            .wire_name(format!("secret_{model}"))
            .survives_stop(surviving),
    );
    shown
}

impl Pack for RenderPack {
    fn tools(&self) -> Vec<ToolInfo> {
        vec![echo()]
    }

    /// Answers `<tool> <value> on <model>`: at once, or once the gate
    /// lets it when the arguments set `hold`.
    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, ToolError>> {
        let origin = cx.origin();
        self.seen.calls.lock().unwrap().push(Called {
            tool: cx.tool().to_string(),
            entry: origin.entry.clone(),
            model: origin.model.clone(),
        });
        let model = origin.model.clone().unwrap_or_default();
        let value = args["value"].as_str().unwrap_or_default();
        let text = format!("{} {value} on {model}", cx.tool());
        if args["hold"] != true {
            return Box::pin(async move { Ok(ToolOutput::trusted(text)) });
        }
        Box::pin(async move {
            let mut held = HeldCall {
                seen: Some(Arc::clone(&self.seen)),
                model,
            };
            self.seen
                .gate
                .acquire()
                .await
                .expect("the gate is never closed")
                .forget();
            held.seen = None;
            Ok(ToolOutput::trusted(text))
        })
    }

    fn render<'a>(
        &'a self,
        cx: RenderContext<'a>,
        offered: Vec<ToolInfo>,
    ) -> PackFuture<'a, Vec<ToolInfo>> {
        assert!(!self.panics, "the render Pack's render panics");
        let model = cx.model.id().name();
        self.seen.renders.lock().unwrap().push(Rendered {
            entry: cx.entry.to_owned(),
            model: cx.model.clone(),
            offered: offered.iter().map(|tool| tool.id.to_string()).collect(),
        });
        let surviving = self.surviving.iter().any(|name| name == model);
        let shown = shown(offered, model, surviving);
        Box::pin(async move { shown })
    }
}

/// Notes its model in [`Seen::dropped`] when it drops still armed: a
/// held call torn down before it answered.
struct HeldCall {
    seen: Option<Arc<Seen>>,
    model: String,
}

impl Drop for HeldCall {
    fn drop(&mut self) {
        if let Some(seen) = &self.seen {
            seen.dropped.lock().unwrap().push(self.model.clone());
        }
    }
}

/// A Harness over `recorder` and `broker`, with the render Pack built
/// from `config` over `seen` and a timer no test here reaches.
fn render_harness(
    recorder: &Arc<MemoryRecorder>,
    broker: ScriptedBroker,
    seen: &Arc<Seen>,
    config: Value,
) -> Harness {
    let mut services = HostServices::new();
    services.provide(&SEEN, Arc::clone(seen)).unwrap();
    let mut packs = PackRegistry::new();
    packs.insert(RENDERER, config).unwrap();
    Harness::new(
        recorder.clone(),
        Arc::new(broker),
        Arc::new(PendingTimer::default()),
        Arc::new(HostContext::new(packs, services)),
        HostServices::new(),
    )
}

/// A prompt over [`PREAMBLE`] whose sections are `sections`, each a
/// heading and the Lua of its one block.
fn prompt(sections: &[(&str, &str)]) -> String {
    let mut source = PREAMBLE.to_owned();
    for (heading, lua) in sections {
        writeln!(source, "## {heading}\n\n```lua\n{lua}\n```\n").expect("a String takes any write");
    }
    source
}

/// One call the model asks for: its id, the wire name it calls, and its
/// arguments.
fn call(id: &str, name: &str, args: Value) -> ToolCall {
    ToolCall::from_parts(id, name, args).expect("a whole call")
}

/// A broker that answers a round whose last message is one of the texts
/// `script` lists by asking for that text's calls, and any other round
/// with the reply `done`.
fn calling(script: Vec<(&'static str, Vec<ToolCall>)>) -> ScriptedBroker {
    ScriptedBroker::new(move |_round, messages| {
        let last = messages
            .last()
            .map(|message| message.content().to_owned())
            .unwrap_or_default();
        let calls = script
            .iter()
            .find(|(asked, _)| *asked == last)
            .map(|(_, calls)| calls.clone());
        Box::pin(async move {
            match calls {
                Some(calls) => {
                    Completion::from_result(CompletionResult::ToolCalls(calls), MODEL).map(Box::new)
                }
                None => reply("done"),
            }
        })
    })
}

/// Runs `source` with `broker` on a Harness over the render Pack built
/// from `config` over `seen`, and returns the completed run's final text
/// and its records.
async fn run(
    source: &str,
    broker: ScriptedBroker,
    seen: &Arc<Seen>,
    config: Value,
) -> (String, Vec<Record>) {
    let recorder = Arc::new(MemoryRecorder::new());
    let report = render_harness(&recorder, broker, seen, config)
        .run(request(source))
        .await
        .expect("the run reaches an outcome");
    let final_text = match report.outcome {
        RunOutcome::Completed { final_text } => final_text,
        other => panic!("the run completes: {other:?}"),
    };
    (
        final_text,
        recorder.records(report.run_id.expect("the run began")),
    )
}

/// Each effect whose record is keyed by `kind`, as its body beside its
/// answer's payload, in effect order.
fn answered(records: &[Record], kind: &str) -> Vec<(Value, Value)> {
    records
        .iter()
        .filter(|record| record.kind == RecordKind::Effect)
        .filter_map(|effect| {
            let body = effect.payload.get(kind)?.clone();
            let answer = records.iter().find(|record| {
                record.kind == RecordKind::Answer && record.effect_id == effect.effect_id
            })?;
            Some((body, answer.payload.clone()))
        })
        .collect()
}

/// A tool call's answer as its output text, or `Dropped`.
fn outcome_of(answer: &Value) -> String {
    match answer.as_str() {
        Some(dropped) => dropped.to_owned(),
        None => answer["ToolCall"]["Ok"]["text"]
            .as_str()
            .unwrap_or("a failure")
            .to_owned(),
    }
}

/// The payload of every event of `kind`, in record order.
fn events(records: &[Record], kind: &str) -> Vec<Value> {
    records
        .iter()
        .filter(|record| record.kind == RecordKind::Event && record.payload["kind"] == kind)
        .map(|record| record.payload.clone())
        .collect()
}

/// Runs a section that offers the render Pack's tool and lets the model
/// call the model-only tool once, then runs `tail`, and returns the run's
/// final text beside what the Pack saw.
async fn after_a_secret_call(tail: &str) -> (String, Arc<Seen>) {
    let seen = Seen::new();
    let broker = calling(vec![(
        "call the secret",
        vec![call("c1", "secret_m", json!({ "value": "x" }))],
    )]);
    let source = prompt(&[(
        "Only",
        &format!(
            "tools.offer('render/echo')\nmessages:user('call the secret')\nmodels.loop()\n{tail}"
        ),
    )]);
    let (text, _records) = run(&source, broker, &seen, Value::Null).await;
    (text, seen)
}

#[tokio::test]
async fn tools_call_refuses_the_model_only_id_the_model_called() {
    let (text, seen) = after_a_secret_call(
        "local ok, err = pcall(tools.call, 'render/secret', { value = 'y' })\n\
         return tostring(ok) .. ':' .. tostring(err)",
    )
    .await;
    assert!(
        text.starts_with("false:")
            && text.contains(
                "tool \"render/secret\" is not a tool in this run; catalog tools: \
                 [\"render/echo\"]"
            ),
        "{text}"
    );
    assert_eq!(
        seen.calls(),
        [Called::new(SECRET, &seen.renders()[0].entry, MODEL)],
        "only the model's call reached the Pack"
    );
}

#[tokio::test]
async fn a_model_only_call_counts_under_its_pack_and_never_under_its_own_id() {
    let (text, _seen) = after_a_secret_call(
        "local ok, err = pcall(function() return tools.stats.tools['render/secret'] end)\n\
         local ids = {}\n\
         for id, count in pairs(tools.stats.tools) do ids[#ids + 1] = id .. '=' .. count end\n\
         return tools.stats.packs.render .. ' ' .. tools.stats.total .. ' ' .. \
         table.concat(ids, ',') .. ' ' .. tostring(err)",
    )
    .await;
    assert!(
        text.starts_with("1 1 render/echo=0 "),
        "the call counts under its Pack and in the total, and tools lists only the \
         catalog tool: {text}"
    );
    assert!(
        text.contains(
            "tools.stats.tools: \"render/secret\" has no seeded count; seeded names: \
             [\"render/echo\"]"
        ),
        "reading the model-only id raises, and its key list leaves it out: {text}"
    );
}
