//! The discovery arm of the `tool_call` arm: a model call that names a
//! discovery tool, or a Dynamic Set tool by its own wire name, decided in
//! full before any preparer runs.
//!
//! `GetDynamicTools` answers inline from the section's frozen Dynamic Set:
//! one tool's description and input schema for a `toolName`, which wins
//! over a `pattern`, else every Dynamic Set tool whose wire name the
//! `pattern` regex matches, or every one when the call names neither, in
//! Dynamic Set order with each description cut to its first 200
//! characters. `CallDynamicTool` hands its inner `arguments`, under the
//! call's own site, to the adhoc or catalog preparer of the Dynamic Set
//! tool its `toolName` names, which counts, reports, and answers the call
//! as that tool's own. A direct call to a Dynamic Set tool, and every
//! malformed discovery call, is answered with refusal text that says what
//! to do next, so the loop goes on.
//!
//! A listing is untrusted, because its descriptions come from the catalog
//! Packs, so it is wrapped with the run's nonce; a refusal is Engine text
//! and passes verbatim. A `GetDynamicTools` call counts under
//! `dynamic/search` whatever its outcome, and any other refused call under
//! `dynamic/call`.

use std::borrow::Cow;

use regex::Regex;
use runvil_types::event::lifecycle;
use runvil_types::tools::OutputTrust;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::execute::protocol::{Answer, ToolCallOutcome};
use crate::lua::{DiscoveryTool, OfferMode, RenderedTools, ToolBinding};
use crate::{Error, Result};

use super::super::{CallSite, ChainIndex, Scheduler};
use super::ToolCallDispatch;

/// The longest `pattern` a search compiles, in characters.
const PATTERN_LIMIT: usize = 256;

/// The longest description a search result keeps whole, in characters.
const DESCRIPTION_LIMIT: usize = 200;

/// What one discovery call comes to.
#[derive(Debug)]
enum Discovered {
    /// A listing, answered as untrusted text.
    Listing(String),
    /// A refusal, answered as trusted text.
    Refused(String),
    /// A `CallDynamicTool` call that reaches the Dynamic Set tool it names,
    /// with the inner arguments.
    Reached(ToolBinding, Value),
}

/// One Dynamic Set tool as a `GetDynamicTools` answer shows it.
#[derive(Debug, Serialize)]
struct Listed<'a> {
    tool: &'a str,
    description: Cow<'a, str>,
    #[serde(rename = "inputSchema", skip_serializing_if = "Option::is_none")]
    input_schema: Option<&'a Value>,
}

impl Scheduler {
    /// Answers a model call routed to `binding`, a discovery tool or a
    /// Dynamic Set tool the model named directly. A `CallDynamicTool` call
    /// that reaches its target goes to that tool's preparer. Any other
    /// call is answered inline: the attempt counted, then a listing
    /// reported succeeded and untrusted, wrapped with the run's nonce, or
    /// a refusal reported failed and trusted, its `ToolResult` under the
    /// call's id and `binding`, as a failed model call reports.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the chain's section holds no frozen
    /// sets, which a model call can only reach after the freeze, and the
    /// error of the counts install or of the target's preparer.
    pub(super) fn prepare_discovery_call(
        &mut self,
        id: ChainIndex,
        binding: &ToolBinding,
        args: Value,
        site: CallSite,
    ) -> Result<ToolCallDispatch> {
        let tool = DiscoveryTool::of(binding.id());
        let frozen = self.chains[id.index()]
            .frame()?
            .vm()?
            .rendered_tools()
            .ok_or(Error::internal(
                "a model call's section holds its frozen sets",
            ))?;
        let discovered = match tool {
            Some(DiscoveryTool::Search) => search(&frozen, &args)?,
            Some(DiscoveryTool::Call) => reach(&frozen, args),
            None => Discovered::Refused(format!(
                "{name:?} is a dynamic tool; call it through CallDynamicTool with toolName {name:?}",
                name = binding.alias()
            )),
        };
        let (text, trust, boundary) = match discovered {
            Discovered::Reached(target, args) => {
                return self.prepare_reached(id, target, args, site);
            }
            Discovered::Listing(listing) => (
                listing,
                OutputTrust::Untrusted,
                lifecycle::TOOL_CALL_SUCCEEDED,
            ),
            Discovered::Refused(refusal) => {
                (refusal, OutputTrust::Trusted, lifecycle::TOOL_CALL_FAILED)
            }
        };
        let counted = tool.unwrap_or(DiscoveryTool::Call).id();
        let counts = self.section_call_counts(id)?;
        counts.ensure(counted)?;
        counts.increment(counted)?;
        let turn = self.call_turn(id, &site);
        let chain = &self.chains[id.index()];
        let text = match trust {
            OutputTrust::Trusted => text,
            _ => chain.ctx.nonce().wrap(&text),
        };
        let emitter = chain.ctx.emitter();
        let section = chain.section_name();
        emitter.report(section, boundary);
        emitter.tool_result(
            section,
            turn,
            site.call_id().unwrap_or_default(),
            binding.alias(),
            Some(binding.id()),
            chain.ctx.tool_set().vendor_for(binding.id()),
            &text,
            trust,
        );
        Ok(ToolCallDispatch::Answered(Answer::ToolCallResult(Ok(
            ToolCallOutcome::Plain(text),
        ))))
    }

    /// Hands a `CallDynamicTool` call's inner `args`, under the call's own
    /// `site`, to the preparer of `target`: the adhoc tool the section's VM
    /// defines under its id, else the catalog tool, issued as a model call.
    fn prepare_reached(
        &mut self,
        id: ChainIndex,
        target: ToolBinding,
        args: Value,
        site: CallSite,
    ) -> Result<ToolCallDispatch> {
        let adhoc = self.chains[id.index()]
            .frame()?
            .vm()?
            .adhoc_tools()
            .binding(&target.id().to_string())?;
        if let Some(adhoc) = adhoc {
            return self.prepare_adhoc_call(id, &adhoc, args, &site);
        }
        self.prepare_catalog_call(id, target, args, site)?;
        Ok(ToolCallDispatch::Issued)
    }
}

/// What a `GetDynamicTools` call with `args` comes to: the lookup of its
/// `toolName`, else the search its `pattern` makes, else the whole
/// Dynamic Set.
///
/// # Errors
/// Returns [`Error::Internal`] should a listing not serialize.
fn search(frozen: &RenderedTools, args: &Value) -> Result<Discovered> {
    let tool = DiscoveryTool::Search.wire_name();
    let refused = |refusal| Ok(Discovered::Refused(refusal));
    match text_arg(tool, args, "toolName") {
        Err(refusal) => return refused(refusal),
        Ok(Some(name)) => {
            return match dynamic_target(tool, frozen, name) {
                Ok(binding) => listing(&Listed {
                    tool: binding.alias(),
                    description: Cow::Borrowed(binding.description()),
                    input_schema: Some(binding.schema()),
                }),
                Err(refusal) => refused(refusal),
            };
        }
        Ok(None) => {}
    }
    let pattern = match text_arg(tool, args, "pattern") {
        Err(refusal) => return refused(refusal),
        Ok(None) => None,
        Ok(Some(pattern)) => match compiled(pattern) {
            Ok(regex) => Some(regex),
            Err(refusal) => return refused(refusal),
        },
    };
    let found: Vec<Listed<'_>> = frozen
        .dynamic_set
        .iter()
        .filter(|binding| {
            pattern
                .as_ref()
                .is_none_or(|pattern| pattern.is_match(binding.alias()))
        })
        .map(|binding| Listed {
            tool: binding.alias(),
            description: cut(binding.description()),
            input_schema: None,
        })
        .collect();
    listing(&found)
}

/// What a `CallDynamicTool` call with `args` comes to: the Dynamic Set
/// tool its `toolName` names, reached with its `arguments`, an empty
/// object when absent.
fn reach(frozen: &RenderedTools, mut args: Value) -> Discovered {
    let tool = DiscoveryTool::Call.wire_name();
    let target = match text_arg(tool, &args, "toolName") {
        Ok(Some(name)) => dynamic_target(tool, frozen, name).cloned(),
        Ok(None) => Err(format!(
            "{tool}: toolName is required; pass a name from GetDynamicTools"
        )),
        Err(refusal) => Err(refusal),
    };
    let target = match target {
        Ok(target) => target,
        Err(refusal) => return Discovered::Refused(refusal),
    };
    match args.get_mut("arguments").map(Value::take) {
        None => Discovered::Reached(target, Value::Object(Map::new())),
        Some(inner @ Value::Object(_)) => Discovered::Reached(target, inner),
        Some(other) => Discovered::Refused(format!(
            "{tool}: arguments must be an object of the tool's arguments, got {}",
            json_kind(&other)
        )),
    }
}

/// Reads the string `key` of `args`, a call to `tool`: `None` when absent,
/// and the refusal when present as anything but a string.
fn text_arg<'a>(
    tool: &str,
    args: &'a Value,
    key: &str,
) -> std::result::Result<Option<&'a str>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(other) => Err(format!(
            "{tool}: {key} must be a string, got {}",
            json_kind(other)
        )),
    }
}

/// Resolves `name`, the `toolName` of a call to `tool`, in the Dynamic
/// Set, or the refusal for a Static Set name or a name neither set holds.
fn dynamic_target<'a>(
    tool: &str,
    frozen: &'a RenderedTools,
    name: &str,
) -> std::result::Result<&'a ToolBinding, String> {
    match frozen.binding(name) {
        Some((binding, OfferMode::Dynamic)) => Ok(binding),
        Some((_, OfferMode::Static)) => Err(format!(
            "{tool}: {name:?} is in your tool list, not a dynamic tool; call it directly"
        )),
        None => Err(format!(
            "{tool}: no dynamic tool is named {name:?}; call GetDynamicTools with a pattern or \
             no arguments to find the names"
        )),
    }
}

/// Compiles a search's `pattern`, or the refusal for one past
/// [`PATTERN_LIMIT`] characters or that is no regex, naming the parser's
/// reason, the last line of its report.
fn compiled(pattern: &str) -> std::result::Result<Regex, String> {
    let length = pattern.chars().count();
    if length > PATTERN_LIMIT {
        return Err(format!(
            "GetDynamicTools: pattern is {length} characters, over the limit of \
             {PATTERN_LIMIT}; shorten it"
        ));
    }
    Regex::new(pattern).map_err(|error| {
        let report = error.to_string();
        let reason = report.lines().next_back().unwrap_or_default().trim();
        let reason = reason.strip_prefix("error: ").unwrap_or(reason);
        format!(
            "GetDynamicTools: pattern {pattern:?} is not a valid regex: {reason}; fix it, or \
             omit it to list every tool"
        )
    })
}

/// `description` whole, or its first [`DESCRIPTION_LIMIT`] characters and
/// `... [truncated]` when it is longer.
fn cut(description: &str) -> Cow<'_, str> {
    match description.char_indices().nth(DESCRIPTION_LIMIT) {
        Some((end, _)) => Cow::Owned(format!("{}... [truncated]", &description[..end])),
        None => Cow::Borrowed(description),
    }
}

/// A listing's text: `listed` as compact JSON.
///
/// # Errors
/// Returns [`Error::Internal`] should `listed` not serialize.
fn listing(listed: &impl Serialize) -> Result<Discovered> {
    serde_json::to_string(listed)
        .map(Discovered::Listing)
        .map_err(|_| Error::internal("a discovery listing serializes"))
}

/// The JSON type of `value`, for a refusal.
fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
