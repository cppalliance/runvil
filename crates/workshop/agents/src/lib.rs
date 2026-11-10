//! workshop-agents - Workshop's agent conversations: the conversation
//! table keyed by a Workshop-minted id, each conversation's transcript,
//! state, and failure reports, the operator's messages to its run,
//! discovery over the agents folder with the built-in `chat` agent, the
//! Host's timer, and the recorder tee every run of a conversation writes
//! through.
//!
//! A conversation is one launched agent and its one run. The server
//! builds that run's `RunSetup` around the recorder tee the
//! [`Conversation`] hands it, streams the run's own rounds into it
//! through [`Conversation::publish_delta`], and hands it to
//! [`Conversation::run`], which parses the agent on the server's Host and
//! drives the run. An agent that fails to parse is reported as a failed
//! run and records none. [`Conversation::send_input`] sends the
//! operator's message to the run's main chain. A stop drops the round in
//! flight and keeps the run going; a close cancels the run, and the
//! conversation ends when the run does.
//!
//! ## Invariants
//!
//! - Tier: feature; may depend on: the vocabulary crates
//!   (`workshop-protocol`, `workshop-registry`, `workshop-support`), the
//!   service crates (`workshop-gateway`, `workshop-menu`,
//!   `workshop-status`), `runvil`, the one crate a Host depends on for
//!   the Harness and the Engine, and third-party crates; today it names
//!   none of the Workshop crates. Never on `workshop-server`, another
//!   feature crate, an Engine crate, or a private Harness or Gateway
//!   crate.
//!   `cargo test -p build-xtask` enforces the product and container
//!   boundaries.
//! - A conversation's transcript is held in memory. The recorder tee adds
//!   each event to it only after the inner recorder accepts the event, so
//!   the record, the live broadcast, and the transcript agree event for
//!   event, and each event takes its `reply` from its own round.
//! - A conversation's channels close when it ends, so whatever holds only
//!   its receivers ends with it.

mod conversation;
mod discovery;
mod protocol;
mod state;
mod table;
mod tee;
mod timer;
mod transcript;

pub use conversation::Conversation;
pub use discovery::{BUILTIN_CHAT_SOURCE, LaunchError, discover_agents, load_agent};
pub use protocol::{ConversationId, Delta, DeltaKind, SessionEvent};
pub use state::{FailureKind, SessionFailure, SessionState};
pub use table::Conversations;
pub use timer::TokioTimer;
