//! A Host runs one prompt through a Harness: it builds a Host over a
//! fresh Pack registry, which holds only the built-ins, a finder that
//! serves its one prompt, and a tokio timer, parses the prompt by its
//! name, hands the run a recorder, an offline inference broker, and the
//! operator's name, and prints the greeting the run leaves at its output
//! file.

use std::sync::Arc;
use std::time::Duration;

use runvil::effect::Round;
use runvil::harness::{BoxFuture, InferenceBroker, MemoryRecorder, Packs, Timer};
use runvil::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
    make_completion_error, phrase,
};
use runvil::plugin::PackRegistry;
use runvil::{Error, ErrorCode, Harness, Host, RunSetup};

/// The name the Host finds the greeting prompt by.
const GREET_NAME: &str = "greet.md";

/// Reads the line in `name.txt` and writes its greeting to `reply.txt`.
const GREET: &str = concat!(
    "---\nname: greet\ndescription: Greets the operator by name\nrunvil: 0\n",
    "input: { path: name.txt, description: The operator's name }\n",
    "output: { path: reply.txt, description: The greeting }\n",
    "---\n\n# Greet\n\n## Answer\n\n```lua\n",
    "store.write('reply.txt', 'Hello, ' .. store.read('name.txt') .. '.')\n",
    "```\n",
);

/// Serves [`GREET`] under [`GREET_NAME`] and finds no other prompt.
fn find(name: &str) -> Result<String, Error> {
    if name == GREET_NAME {
        return Ok(GREET.to_owned());
    }
    let code = ErrorCode::NotFound;
    Err(Error::new(
        code,
        format!("no prompt named {name}"),
        code.prose(),
    ))
}

/// Lists no model and refuses every round, since the prompt asks no model.
struct Offline;

impl InferenceBroker for Offline {
    fn models(&self) -> BoxFuture<Result<ModelCatalog, Error>> {
        Box::pin(async { Ok(ModelCatalog::empty()) })
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
    ) -> BoxFuture<Result<Box<Completion>, Error>> {
        let code = ErrorCode::Unavailable;
        Box::pin(async move { Err(make_completion_error(code, phrase(code))) })
    }
}

/// Sleeps on tokio's timer.
struct Clock;

impl Timer for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Build the Host once: its installed Packs, how it finds prompts,
    //    and its timer.
    let host = Arc::new(Host {
        packs: Packs::new(PackRegistry::new()),
        prompts: Arc::new(find),
        timer: Arc::new(Clock),
    });

    // 2. Find the prompt by its name and parse it.
    let prompt = host.parse(GREET_NAME)?;

    // 3. Give this run its recorder, its broker, and the operator's name;
    //    every other field keeps its default, such as a fresh store.
    let recorder = Arc::new(MemoryRecorder::new());
    let setup = RunSetup {
        input_text: Some("desk".to_owned()),
        ..RunSetup::new(recorder.clone(), Arc::new(Offline))
    };

    // 4. Run it to its end; the Harness is spent.
    let report = Harness::new(host, prompt, setup).run().await?;

    // 5. The report says how the run ended and what it left at its output
    //    file, and the recorder holds the run's result.
    assert!(report.result.is_ok());
    assert_eq!(recorder.result(report.run_id), Some(report.result));
    let greeting = report.output?;
    assert_eq!(greeting, "Hello, desk.");
    println!("{greeting}");
    Ok(())
}
