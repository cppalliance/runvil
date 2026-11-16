//! Runs a prompt through the Host API and prints its declared output.

use runvil::{Error, Host};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let prompts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/prompts");
    let host = Host::new(prompts);
    let greeting = host.run("greet.md").input("desk").output().await?;
    assert_eq!(greeting, "Hello, desk.");
    println!("{greeting}");
    Ok(())
}
