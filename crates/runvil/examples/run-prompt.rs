//! A complete script-only run, using the same API as a model-backed Host.

use std::sync::Arc;

use runvil::harness::MemoryRecorder;
use runvil::{Error, ErrorCode, Host};

const GREET: &str = concat!(
    "---\nname: greet\ndescription: Greets the operator by name\nrunvil: 0\n",
    "input: { path: name.txt, description: The operator's name }\n",
    "output: { path: reply.txt, description: The greeting }\n",
    "---\n\n# Greet\n\n## Answer\n\n```lua\n",
    "store.write('reply.txt', 'Hello, ' .. store.read('name.txt') .. '.')\n",
    "```\n",
);

fn find(name: &str) -> Result<String, Error> {
    if name == "greet.md" {
        return Ok(GREET.to_owned());
    }
    let code = ErrorCode::NotFound;
    Err(Error::new(
        code,
        format!("no prompt named {name}"),
        code.prose(),
    ))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let host = Host::new("./prompts").with_prompt_finder(find);
    // Keep a shared recorder only when the application wants the history.
    let history = Arc::new(MemoryRecorder::new());
    let report = host
        .run("greet.md")
        .input("desk")
        .recorder(history.clone())
        .await?;

    assert_eq!(history.result(report.run_id), Some(report.result.clone()));
    report.result?;
    let greeting = report.output?;
    assert_eq!(greeting, "Hello, desk.");
    println!("{greeting}");
    Ok(())
}
