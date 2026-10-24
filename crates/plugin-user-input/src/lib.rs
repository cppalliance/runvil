//! `plugin-user-input` - the Pack that lets a prompt section pause and
//! ask the operator for their next message.
//!
//! The crate's Plugin, [`install`], inserts the Pack `runvil/user-input`,
//! whose Pack id is `user-input`. A prompt declares the Pack in its
//! frontmatter, and its sections then have an `input` table:
//!
//! ````text
//! uses:
//!   - user-input
//! ````
//!
//! ````lua
//! local text = input.ask()
//! ````
//!
//! A prompt that does not declare the Pack has no `input` global.
//!
//! # Where this fits
//!
//! The Pack offers one tool, the ask tool, `user-input/ask`, whose last
//! segment is [`ASK`]. Its prelude defines `input.ask()`, which calls the
//! ask tool by that full id, so every ask reaches the Harness as an
//! ordinary tool call. The ask tool waits on the run's [`InputBroker`], provided among
//! the run's own services under [`INPUT_BROKER`]. A Host binds each run's
//! broker to whoever launched the run, so the question reaches the right
//! operator without naming the run or the section.
//!
//! The operator's answer is trusted, so the ask tool answers with
//! [`ToolOutput::trusted`](runvil_plugin::ToolOutput::trusted) and
//! the text is never guard-wrapped. The ask tool's info survives
//! stops: a stop leaves the question open. The other occasions
//! [`Pack::call`](runvil_plugin::Pack::call) lists drop it, the
//! early end of the task that asked included.
//!
//! # A run without an operator
//!
//! The Pack names [`INPUT_BROKER`] among its needs, so a run whose Host
//! supplies no broker cannot use the Pack, and a prompt that declares it
//! is refused before the run starts with a line naming the Pack and
//! `runvil/input-broker`.
//!
//! # Failed and cancelled waits
//!
//! A broker failure is a [`ToolError`], and it becomes the ask call's
//! error unchanged. It raises at the script's `input.ask()` call as a Lua
//! error of kind `"tool"` whose message is the broker's message, so
//! `pcall` catches it; uncaught, it ends the run as a tool failure. The
//! message reaches the prompt, where the model can read it too, so a
//! broker writes it for that audience. Cancelling the run drops the ask
//! tool's pending wait.
//!
//! # Prompt-side rules
//!
//! - `input.ask()` takes no arguments. Calling it with any argument
//!   raises a Lua error with the message `input.ask takes no arguments`,
//!   so a prompt that passes a question fails loudly instead of losing
//!   it.
//! - The model can ask the operator only when the prompt's Lua opts in.
//!   A section offers the ask tool before its first loop round, with
//!   the Pack, `tools.offer("user-input")`, or by its id with
//!   `tools.offer("user-input/ask")`, and the model sees it as
//!   `user-input_ask`. Neither inserting nor declaring the Pack
//!   advertises anything to the model.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-plugin`, `shared-*` crates, and outside
//!   libraries. `cargo test -p build-xtask` enforces the Plugin family's
//!   allow-list.
//! - The crate names no async runtime. The Harness polls the ask call
//!   inside the run's own future, so an [`InputBroker`] must not block
//!   while polled.
//! - The ask tool's info sets `survives_stop`, so a stop leaves the
//!   operator's question open; every other occasion `Pack::call` lists
//!   drops it.

mod ask;

use runvil_plugin::{InstallError, PackInfo, PackRegistry, ServiceId, ServiceKey, ToolError};
use serde_json::Value;

/// The Pack's `PackInfo`.
///
/// Its FQN is `runvil/user-input`, so its Pack id is `user-input`. It
/// has a prelude and needs the run's [`INPUT_BROKER`].
const USER_INPUT: PackInfo = PackInfo::new("runvil/user-input", ask::construct)
    .description("Ask the person running the prompt")
    .prelude(PRELUDE)
    .needs(NEEDS);

/// The Plugin: inserts the `user-input` Pack into `packs` with
/// `config["user-input"]`, which is `null` when `config` has no
/// `user-input` key.
///
/// # Errors
/// Returns the registry's [`InstallError`], such as
/// [`InstallError::DuplicatePack`] when `packs` holds
/// `runvil/user-input` already.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the Plugin signature fixes the argument types"
)]
pub fn install(packs: &mut PackRegistry, config: Value) -> Result<(), InstallError> {
    packs.insert(USER_INPUT, config["user-input"].clone())
}

/// The ask tool's last segment: its full id is `user-input/ask`, under
/// the Pack's id.
pub const ASK: &str = "ask";

/// The service key for the input broker, the per-run service that carries
/// a question to the operator.
///
/// The key's id is `runvil/input-broker`. A Host with an operator
/// provides its broker among each run's services under this key.
pub const INPUT_BROKER: ServiceKey<dyn InputBroker> = ServiceKey::new("runvil/input-broker");

/// The Pack's needs: a run without a broker cannot use the Pack.
const NEEDS: &[ServiceId] = &[INPUT_BROKER.id()];

/// The Lua that defines `input.ask()` for a prompt that declares the
/// Pack. `...` is the Pack's id.
const PRELUDE: &str = r#"local pack = ...
input = {}
function input.ask(...)
  if select('#', ...) > 0 then
    error("input.ask takes no arguments", 2)
  end
  return tools.call(pack .. "/ask")
end
"#;

/// Waits for the operator's next message on the ask tool's behalf.
///
/// # Invariants
///
/// - [`wait`](InputBroker::wait) returns the operator's text byte-exact,
///   as the operator sent it.
/// - A wait whose future is dropped, as happens when the run is
///   cancelled, must close any prompt it opened for the operator. It must
///   not panic.
/// - [`wait`](InputBroker::wait) must not block while it is polled. The
///   Harness polls it inside the run's own future, beside every other
///   effect of the run, so a broker hands any blocking work to the Host's
///   own runtime.
#[async_trait::async_trait]
pub trait InputBroker: Send + Sync {
    /// Waits for the operator's next message and returns it byte-exact.
    ///
    /// # Errors
    /// Returns a [`ToolError`], whose message is safe to show the model,
    /// when the broker fails to produce the operator's message, for
    /// example because the Host withdrew the wait. It is the ask call's
    /// error unchanged.
    async fn wait(&self) -> Result<String, ToolError>;
}
