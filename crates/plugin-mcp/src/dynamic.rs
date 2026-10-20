//! The one Pack over every configured server: its two static tools, the
//! listing `get_dynamic_tools` answers, and the call `call_dynamic_tool`
//! routes to one server's tool.

use std::collections::BTreeMap;

use rmcp::model::JsonObject;
use rmcp::service::{Peer, RoleClient};
use runvil_plugin::{
    Pack, PackFuture, PackId, ToolContext, ToolError, ToolErrorKind, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Map, Value, json};
use tokio::runtime::Handle;

use crate::server::{self, Listed, Ready, Server, State};

/// The listing tool's name under the Pack id.
const GET: &str = "get_dynamic_tools";

/// The calling tool's name under the Pack id.
const CALL: &str = "call_dynamic_tool";

const GET_DESCRIPTION: &str = "List the tools of the configured MCP servers. With no arguments, \
     list each server's namespace, status, and tool names; with namespace, that server's tools \
     and their input schemas; with namespace and toolName, one tool.";

const CALL_DESCRIPTION: &str = "Call a tool of a configured MCP server by namespace and \
     toolName, with arguments as a JSON object that matches the tool's input schema. List the \
     servers' tools first to see it.";

/// One configured server.
#[derive(Debug)]
pub(crate) enum Namespace {
    /// An entry that failed to parse, with the reason. It never connects.
    Refused(String),
    /// A parsed entry's connection.
    Server(Server),
}

/// A namespace's status now, borrowed from its state.
#[derive(Clone, Copy)]
enum Status<'a> {
    Ready(&'a Ready),
    Loading,
    Error(&'a str),
}

impl Namespace {
    /// Runs `f` on the namespace's status now. The server's state stays
    /// borrowed only while `f` runs.
    fn with_status<R>(&self, f: impl FnOnce(Status<'_>) -> R) -> R {
        match self {
            Namespace::Refused(reason) => f(Status::Error(reason)),
            Namespace::Server(server) => match &*server.state() {
                State::Starting => f(Status::Loading),
                State::Ready(ready) => f(Status::Ready(ready)),
                State::Failed(reason) => f(Status::Error(reason)),
            },
        }
    }
}

/// The Pack: its two tools, every configured server under its name, and
/// the Host's runtime its calls are spawned on.
#[derive(Debug)]
pub(crate) struct Mcp {
    get: ToolInfo,
    call: ToolInfo,
    namespaces: BTreeMap<String, Namespace>,
    runtime: Handle,
}

fn invalid(text: impl Into<String>) -> ToolError {
    ToolError::message(text).with_kind(ToolErrorKind::InvalidArguments)
}

/// The members of a tool's arguments, which must be a JSON object or
/// `null`.
fn members(args: Value, refusal: &str) -> Result<Map<String, Value>, ToolError> {
    match args {
        Value::Null => Ok(Map::new()),
        Value::Object(members) => Ok(members),
        _ => Err(invalid(refusal)),
    }
}

/// The string at `key`, or `None` when it is absent or `null`.
fn string<'a>(members: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, ToolError> {
    match members.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(invalid(format!("`{key}` must be a string"))),
    }
}

/// One tool as the listing shows it, with its input schema when
/// `schemas` is set.
fn tool_entry(tool: &Listed, schemas: bool) -> Value {
    let mut entry = json!({ "tool": tool.name, "description": tool.description });
    if schemas {
        entry["inputSchema"] = tool.input_schema.clone();
    }
    entry
}

/// A namespace as the listing shows it: its status, the reason when it
/// is `error`, and its tools when it is `ready`.
fn namespace_entry(name: &str, status: Status<'_>, schemas: bool) -> Value {
    match status {
        Status::Ready(ready) => json!({
            "namespace": name,
            "status": "ready",
            "tools": ready
                .tools
                .iter()
                .map(|tool| tool_entry(tool, schemas))
                .collect::<Vec<_>>(),
        }),
        Status::Loading => json!({ "namespace": name, "status": "loading" }),
        Status::Error(reason) => json!({ "namespace": name, "status": "error", "reason": reason }),
    }
}

impl Mcp {
    /// The Pack over `namespaces`, with its two tools under `name`.
    pub(crate) fn new(
        name: &PackId,
        namespaces: BTreeMap<String, Namespace>,
        runtime: Handle,
    ) -> Result<Mcp, ToolError> {
        let id = |tool: &str| {
            ToolId::parse(&format!("{name}/{tool}")).map_err(|e| {
                ToolError::new(ToolErrorKind::Backend, "MCP could not name its tools")
                    .with_source(e)
            })
        };
        let get = ToolInfo::new(
            id(GET)?,
            GET_DESCRIPTION,
            json!({
                "type": "object",
                "properties": {
                    "namespace": {
                        "type": "string",
                        "description": "One server's namespace, as the listing with no arguments names it."
                    },
                    "toolName": {
                        "type": "string",
                        "description": "One tool's name within the namespace. Requires namespace."
                    }
                }
            }),
        );
        let call = ToolInfo::new(
            id(CALL)?,
            CALL_DESCRIPTION,
            json!({
                "type": "object",
                "properties": {
                    "namespace": {
                        "type": "string",
                        "description": "The namespace of the server the tool belongs to."
                    },
                    "toolName": {
                        "type": "string",
                        "description": "The tool's name, exactly as the listing shows it."
                    },
                    "arguments": {
                        "type": "object",
                        "description": "The tool's arguments, matching its input schema. Defaults to {}."
                    }
                },
                "required": ["namespace", "toolName"]
            }),
        );
        Ok(Mcp {
            get,
            call,
            namespaces,
            runtime,
        })
    }

    /// The server under `name`, or an error that says how to list them.
    fn namespace(&self, name: &str) -> Result<&Namespace, ToolError> {
        self.namespaces.get(name).ok_or_else(|| {
            invalid(format!(
                "no configured MCP server has the namespace `{name}`; call {} with no arguments \
                 to list the namespaces",
                self.get.id
            ))
        })
    }

    fn unknown_tool(&self, namespace: &str, tool: &str) -> ToolError {
        invalid(format!(
            "the MCP server `{namespace}` has no tool `{tool}`; call {} with namespace \
             `{namespace}` to list its tools",
            self.get.id
        ))
    }

    /// Answers `get_dynamic_tools` as JSON text.
    fn list(&self, args: Value) -> Result<String, ToolError> {
        let members = members(
            args,
            "get_dynamic_tools takes a JSON object with the optional strings `namespace` and `toolName`",
        )?;
        let listing = match (
            string(&members, "namespace")?,
            string(&members, "toolName")?,
        ) {
            (None, None) => {
                let namespaces: Vec<Value> = self
                    .namespaces
                    .iter()
                    .map(|(name, namespace)| {
                        namespace.with_status(|status| namespace_entry(name, status, false))
                    })
                    .collect();
                json!({ "namespaces": namespaces })
            }
            (None, Some(_)) => {
                return Err(invalid(
                    "`toolName` names a tool of one server, so it needs `namespace` too",
                ));
            }
            (Some(name), None) => self
                .namespace(name)?
                .with_status(|status| namespace_entry(name, status, true)),
            (Some(name), Some(tool)) => self.namespace(name)?.with_status(|status| {
                let Status::Ready(ready) = status else {
                    return Ok(namespace_entry(name, status, true));
                };
                let listed = ready
                    .tools
                    .iter()
                    .find(|listed| listed.name == tool)
                    .ok_or_else(|| self.unknown_tool(name, tool))?;
                let mut entry = tool_entry(listed, true);
                entry["namespace"] = name.into();
                Ok(entry)
            })?,
        };
        Ok(listing.to_string())
    }

    /// The peer and tool name a `call_dynamic_tool` goes to, or why it
    /// cannot go out.
    fn target(&self, namespace: &str, tool: &str) -> Result<(Peer<RoleClient>, String), ToolError> {
        self.namespace(namespace)?.with_status(|status| match status {
            Status::Ready(ready) => ready
                .tools
                .iter()
                .find(|listed| listed.name == tool)
                .map(|listed| (ready.peer.clone(), listed.name.clone()))
                .ok_or_else(|| self.unknown_tool(namespace, tool)),
            Status::Loading => Err(ToolError::message(format!(
                "the MCP server `{namespace}` is still loading; call {} later to see when it is ready",
                self.get.id
            ))),
            Status::Error(reason) => Err(ToolError::message(format!(
                "the MCP server `{namespace}` is unavailable: {reason}"
            ))),
        })
    }

    /// Performs `call_dynamic_tool`. Every argument is checked before
    /// anything is sent.
    async fn call_tool(&self, args: Value) -> Result<ToolOutput, ToolError> {
        let mut members = members(
            args,
            "call_dynamic_tool takes a JSON object with the strings `namespace` and `toolName` \
             and an optional object `arguments`",
        )?;
        let arguments = match members.remove("arguments") {
            None | Some(Value::Null) => JsonObject::new(),
            Some(Value::Object(arguments)) => arguments,
            Some(_) => return Err(invalid("MCP tool arguments must be a JSON object")),
        };
        let (Some(namespace), Some(tool)) = (
            string(&members, "namespace")?,
            string(&members, "toolName")?,
        ) else {
            return Err(invalid(
                "call_dynamic_tool needs the strings `namespace` and `toolName`",
            ));
        };
        let (peer, name) = self.target(namespace, tool)?;
        server::call(&self.runtime, peer, name, arguments).await
    }
}

impl Pack for Mcp {
    fn tools(&self) -> Vec<ToolInfo> {
        vec![self.get.clone(), self.call.clone()]
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, ToolError>> {
        Box::pin(async move {
            let tool = cx.tool();
            if *tool == self.get.id {
                self.list(args).map(ToolOutput::untrusted)
            } else if *tool == self.call.id {
                self.call_tool(args).await
            } else {
                Err(ToolError::message(format!("MCP has no tool {tool}")))
            }
        })
    }
}
