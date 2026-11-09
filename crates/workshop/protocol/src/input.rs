//! The agent-session input frame: the operator's message.

use serde::Deserialize;

/// The operator's message, sent at any time. Wire type `input_response`.
///
/// `{"type":"input_response","text":"..."}` goes to the session's run
/// whenever the operator sends it: it answers the agent while the agent
/// waits on its inbox, and steers the turn while the agent works.
///
/// The session routes on the envelope's `type` and deserializes the body
/// with serde, which ignores the envelope tag itself. `text` is the
/// operator's input, byte-exact as typed. Like every inbound frame it
/// takes no delivery classification, because the server pushes none.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InputResponse {
    /// The operator's text, byte-exact as typed.
    pub text: String,
}
