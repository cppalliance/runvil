//! Nash tool registration with typed arguments and results.
//! Run with `cargo run -p runvil --example nash -- http://127.0.0.1:3000`.

use std::time::Duration;

use reqwest::{Client, Method};
use runvil::plugin::{PackInfo, PackRegistry, Tool, ToolContext};
use runvil::{Error, ErrorCode, Host, OutputTrust, make_tool_error};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Request {
    method: Verb,
    /// Path on the Nash server, starting with /api/.
    path: String,
    /// JSON text to send with a PUT.
    body: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
enum Verb {
    Get,
    Put,
    Delete,
}

#[derive(Serialize)]
struct Reply {
    status: u16,
    body: String,
}

fn install(packs: &mut PackRegistry, server: &str) -> Result<(), Error> {
    let server = server.trim_end_matches('/').to_owned();
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| make_tool_error(ErrorCode::ToolOther, error.to_string()))?;
    packs.define(
        PackInfo::new("kirill/nash").description("Send requests to Nash"),
        move |pack| {
            pack.add(Nash {
                client: client.clone(),
                server: server.clone(),
            })
        },
    )
}

struct Nash {
    client: Client,
    server: String,
}

impl Tool for Nash {
    const NAME: &'static str = "request";
    const DESCRIPTION: &'static str = "Send a request to Nash and return its status and body.";
    const TRUST: OutputTrust = OutputTrust::Untrusted;
    type Args = Request;
    type Output = Reply;

    async fn call(&self, _context: ToolContext<'_>, args: Request) -> Result<Reply, Error> {
        if !args.path.starts_with("/api/") {
            return Err(make_tool_error(
                ErrorCode::ToolInvalidArguments,
                "path must start with /api/",
            ));
        }
        let method = match args.method {
            Verb::Get => Method::GET,
            Verb::Put => Method::PUT,
            Verb::Delete => Method::DELETE,
        };
        let mut request = self
            .client
            .request(method, format!("{}{}", self.server, args.path));
        if let Some(body) = args.body {
            request = request
                .header("content-type", "application/json")
                .body(body);
        }
        let unreachable = |error: reqwest::Error| {
            make_tool_error(
                ErrorCode::ToolTransport,
                format!("Nash at {} did not answer: {error}", self.server),
            )
        };
        let response = request.send().await.map_err(unreachable)?;
        Ok(Reply {
            status: response.status().as_u16(),
            body: response.text().await.map_err(unreachable)?,
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = std::env::args()
        .nth(1)
        .ok_or("supply the Nash server URL")?;
    let mut packs = PackRegistry::new();
    install(&mut packs, &server)?;
    let host = Host::new(".")
        .with_packs(packs)
        .with_prompt_finder(|_: &str| {
            Ok(concat!(
                "---\nname: nash\ndescription: Reads Nash\nrunvil: 0\n",
                "uses:\n  - kirill/nash\n---\n\n# Nash\n\n## Read\n\n```lua\n",
                "return tools.call('nash/request', {method = 'GET', path = '/api/'})\n```\n",
            )
            .to_owned())
        });
    println!("{}", host.run("nash.md").text().await?);
    Ok(())
}
