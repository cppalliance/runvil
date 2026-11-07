//! The chat-completions message role.

use std::fmt;
use std::str::FromStr;

use runvil_types::error::{Error, ErrorCode, ErrorFact};
use serde_json::Value;

/// Names who speaks one message in a chat-completions conversation.
///
/// It serializes as its lowercase wire name: `system`, `user`, `assistant`,
/// or `tool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Role {
    /// Instructions that frame the conversation.
    System,
    /// A turn from the user.
    User,
    /// A turn from the model.
    Assistant,
    /// The result of a tool call the model requested.
    Tool,
}

impl Role {
    /// Lists every role, in declaration order.
    pub const ALL: &'static [Role] = &[Role::System, Role::User, Role::Assistant, Role::Tool];

    /// Returns the role's wire name: `system`, `user`, `assistant`, or
    /// `tool`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = Error;

    /// Parses a role from its exact wire name, never case-folded. Other text
    /// is an [`InvalidValue`](ErrorCode::InvalidValue) error that names it
    /// and lists every role.
    fn from_str(name: &str) -> Result<Role, Error> {
        if let Some(role) = Role::ALL.iter().copied().find(|role| role.as_str() == name) {
            return Ok(role);
        }
        let roles: Vec<String> = Role::ALL.iter().map(|role| format!("`{role}`")).collect();
        let text = format!(
            "invalid role `{name}`: required one of {}",
            roles.join(", ")
        );
        let mut error = Error::new(ErrorCode::InvalidValue, text.clone(), text);
        error.facts.insert(ErrorFact::Name, Value::from(name));
        Err(error)
    }
}
