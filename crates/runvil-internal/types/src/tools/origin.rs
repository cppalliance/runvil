//! Who made one tool call: [`ToolCallOrigin`] and its [`ToolCaller`].

use serde::{Deserialize, Serialize};

/// Records who made one tool call and where.
///
/// It names the run's execution, the section whose Lua was running and the
/// entry of that section that made the call, and whether the section's
/// script or a model round asked for the call, with that round's model. A
/// log uses it to attribute the call. The application can use it to
/// apply different policy to the same tool depending on who called it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolCallOrigin {
    /// The run's execution identifier.
    pub execution: String,
    /// The name of the section that made the call.
    pub section: String,
    /// Which kind of code asked for the call.
    pub caller: ToolCaller,
    /// The section entry that made the call: the value its Lua reads as
    /// `sys.id`, which differs between two entries of one section. An
    /// origin serialized without the field reads as `None`, and a `None` is
    /// left out when serializing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// The name of the model whose round requested the call, without the
    /// server namespace its id also holds; `None` for a script call. It
    /// serializes as `entry` does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl ToolCallOrigin {
    /// Builds the origin of a call `caller` made from `section` in the run
    /// `execution`, naming no entry and no model.
    #[must_use]
    pub fn new(
        execution: impl Into<String>,
        section: impl Into<String>,
        caller: ToolCaller,
    ) -> ToolCallOrigin {
        ToolCallOrigin {
            execution: execution.into(),
            section: section.into(),
            caller,
            entry: None,
            model: None,
        }
    }

    /// Names `entry` as the section entry that made the call.
    #[must_use]
    pub fn with_entry(mut self, entry: impl Into<String>) -> ToolCallOrigin {
        self.entry = Some(entry.into());
        self
    }

    /// Names `model` as the model whose round requested the call.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> ToolCallOrigin {
        self.model = Some(model.into());
        self
    }
}

/// Which kind of code asked for one tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolCaller {
    /// The section's own Lua called the tool through `tools.call`.
    Script,
    /// A model round requested the call.
    Model,
}
