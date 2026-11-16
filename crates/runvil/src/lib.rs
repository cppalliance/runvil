//! Runs prompts for a Host. This is the one crate an application depends on.
//!
//! Save this as `prompts/greet.md`:
//!
//! ````markdown
//! ---
//! name: greet
//! description: Greets the operator by name
//! runvil: 0
//! input: { path: name.txt, description: The operator's name }
//! output: { path: reply.txt, description: The greeting }
//! ---
//!
//! # Greet
//!
//! ## Answer
//!
//! ```lua
//! store.write('reply.txt', 'Hello, ' .. store.read('name.txt') .. '.')
//! ```
//! ````
//!
//! Configure a [`Host`] once, then supply each run's inputs where you run it:
//!
//! ```text
//! use runvil::{Error, Host};
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() -> Result<(), Error> {
//!     let host = Host::new("./prompts");
//!     let greeting = host.run("greet.md").input("Ada").output().await?;
//!     assert_eq!(greeting, "Hello, Ada.");
//!     Ok(())
//! }
//! ```
//!
//! The Host reads prompt sources from that folder. A run's files live in a
//! fresh in-memory filesystem: [`RunBuilder::input`] writes its declared
//! `input:` file, and [`RunBuilder::output`] reads its declared `output:` file.
//! [`RunBuilder::args`] supplies Lua's `args` string instead. Use
//! [`RunBuilder::text`] to get the final text returned by the prompt's Lua.
//! All parse, recording, and execution failures propagate through `?` when
//! using `text()` or `output()`; their execution errors carry the run id.
//!
//! Model calls need a broker configured with [`Host::with_broker`]. Install
//! optional Packs with [`Host::with_packs`], passing a [`plugin::PackRegistry`]:
//!
//! ```text
//! let host = Host::new("./prompts")
//!     .with_broker(broker)
//!     .with_packs(packs);
//! let answer = host.run("research.md")
//!     .args("Explain this code")
//!     .model("my-model")
//!     .text().await?;
//! ```
//!
//! Without a model selection, the run uses the first model in the broker's
//! catalog. The default timer works on any executor. Each run gets its own
//! instances of the Packs its prompt holds. To retain history, configure a
//! recorder once with [`Host::with_recorder`]. These service methods take
//! ordinary values and also accept shared `Arc` values when needed.
//!
//! Awaiting a [`RunBuilder`] gives the full [`RunReport`]: its run id, final
//! result, declared output, and unread messages. A recorded execution failure
//! stays in [`RunReport::result`]; `await?` alone checks parsing and recording:
//!
//! ```text
//! let report = host.run("research.md").args("Explain this code").await?;
//! println!("run {}: {:?}", report.run_id, report.result);
//! ```
//!
//! Each builder owns a Host snapshot and can outlive it or move into a
//! spawned task. Snapshots share services and Pack factories. When a
//! run needs messages, stop, or cancellation, call [`RunBuilder::prepare`],
//! take [`Harness::control`], and drive [`Harness::run`]:
//!
//! ```text
//! let run = host.run("chat.md").interactive().prepare()?;
//! let control = run.control();
//! assert!(control.send("Hello"));
//! let report = run.run().await?;
//! ```
//!
//! [`RunBuilder::interactive`] enables the prompt's `user` Pack. Controls can
//! also send messages, stop a round, or cancel while the run is pending.
//! [`RunBuilder::configure`] exposes all of [`RunSetup`] for filesystem policy,
//! resource limits, and other settings. Launchers with an already parsed
//! [`Prompt`] can construct a [`Harness`] directly with their explicit setup.
//!
//! Every executed run is recorded, including a cancel before execution and
//! a failure to resolve its model. Parsing fails before recording begins.
//! A recorder's refusal is `RecorderFailed` and stops the run. The Engine's
//! stepping API is in `runvil-engine`; this crate also exposes the Engine's
//! readable data and the Plugin contract.
//!
//! From the repository, run the complete quickstart with
//! `cargo run -p runvil --example run-prompt`.
//!
//! Install tools with [`plugin::PackRegistry::define`]. Its factory runs
//! once per run that holds the Pack, and adds typed async functions through
//! [`plugin::ToolPack::tool`]. Rust argument types supply their JSON schemas
//! and decoding; Rust results are serialized automatically. Raw JSON and
//! borrowed-context handlers are available when needed. Stateful tools can
//! implement [`plugin::Tool`] with an ordinary async method that borrows
//! their configuration, and register through [`plugin::ToolPack::add`]. Implement
//! [`plugin::Pack`] directly for dynamic tool lists or custom rendering.
//!
//! The `nash` example demonstrates a stateful HTTP tool with typed arguments.

pub use harness_runner::Harness;
pub use harness_runner::Host;
pub use harness_runner::RunBuilder;
pub use harness_runner::RunControl;
pub use harness_runner::RunReport;
pub use harness_runner::RunSetup;

/* The Engine's prompts, run limits, and run seed, and the start time a
   Host's timer stamps each run with, which a prompt reads as `sys.when`.
*/
pub use runvil_engine::Prompt;
pub use runvil_engine::PromptFinder;
pub use runvil_engine::RunLimits;
pub use runvil_engine::RunSeed;
pub use runvil_engine::Timestamp;

/* Universal Error types
*/
pub use runvil_engine::Error;
pub use runvil_engine::ErrorCode;
pub use runvil_engine::ErrorFact;
pub use runvil_engine::SourceLoc;

/* Tool descriptions, and the output or error that answers a tool call.
*/
pub use runvil_engine::OutputTrust;
pub use runvil_engine::ToolId;
pub use runvil_engine::ToolInfo;
pub use runvil_engine::ToolOutput;
pub use runvil_engine::make_tool_error;

/* The boxed future that the inference broker, the timer, the recorder,
   and a Pack return.
*/
pub use runvil_engine::BoxFuture;

pub mod harness {
    //! What a Host gives the Harness beside a run's setup, and what the
    //! Harness hands back: the inference broker and the timer a Host
    //! supplies, the Packs installed from a Pack registry, whose factories
    //! make each run's instances, the Plugins a Host inserts into that
    //! registry, from the dynamic libraries in a Plugin directory or
    //! linked into the Host, with what each insert loaded, and the
    //! recorder that receives every effect, answer, and event of a run
    //! and the result each run ends with, beside an in-memory recorder.

    pub use harness_plugin_loader::Plugin;
    pub use harness_plugin_loader::PluginReport;
    pub use harness_plugin_loader::PluginSource;
    pub use harness_plugin_loader::insert_plugin;
    pub use harness_plugin_loader::insert_plugin_dir;
    pub use harness_runner::Packs;
    pub use harness_runner::display_chain;
    pub use harness_runner::performers::InferenceBroker;
    pub use harness_runner::performers::Timer;
    pub use harness_runner::recorder::MemoryRecorder;
    pub use harness_runner::recorder::Record;
    pub use harness_runner::recorder::RecordKind;
    pub use harness_runner::recorder::RunId;
    pub use harness_runner::recorder::RunMeta;
    pub use harness_runner::recorder::RunRecorder;
    pub use harness_runner::recorder::make_recorder_error;
}

pub mod plugin {
    //! The Plugin contract: a Pack and the factory that makes each run's
    //! instance of it, the registry a Host's Plugins insert Packs into,
    //! where each of its Packs came from, the services map a Host fills
    //! for its Plugins with the keys that name its services, and what a
    //! tool call and a render are lent. The Plugin crates themselves are
    //! the Host's own dependencies.

    pub use runvil_plugin::HostServices;
    pub use runvil_plugin::Pack;
    pub use runvil_plugin::PackFactory;
    pub use runvil_plugin::PackInfo;
    pub use runvil_plugin::PackOrigin;
    pub use runvil_plugin::PackRegistry;
    pub use runvil_plugin::RenderContext;
    pub use runvil_plugin::ServiceId;
    pub use runvil_plugin::ServiceKey;
    pub use runvil_plugin::TOKIO_RUNTIME;
    pub use runvil_plugin::Tool;
    pub use runvil_plugin::ToolContext;
    pub use runvil_plugin::ToolPack;
}

pub mod effect {
    //! The model round an inference broker answers, and who made a tool
    //! call.

    pub use runvil_engine::effect::Round;
    pub use runvil_engine::effect::ToolCallOrigin;
    pub use runvil_engine::effect::ToolCaller;
}

pub mod event {
    //! The events a run reports for its recorder to log.

    pub use runvil_engine::event::Event;
    pub use runvil_engine::event::ReplyOrigin;
}

pub mod ids {
    //! Task, chain, and round ids, and the provenance that orders a run's
    //! records.

    pub use runvil_engine::ids::AbandonReason;
    pub use runvil_engine::ids::ChainId;
    pub use runvil_engine::ids::Provenance;
    pub use runvil_engine::ids::RoundId;
    pub use runvil_engine::ids::TaskId;
}

pub mod model {
    //! Model descriptions and catalogs, the models a prompt's roles are
    //! bound to, and the chat vocabulary of a model round.

    pub use runvil_engine::model::ArgumentNames;
    pub use runvil_engine::model::Completion;
    pub use runvil_engine::model::CompletionOptions;
    pub use runvil_engine::model::CompletionResult;
    pub use runvil_engine::model::Message;
    pub use runvil_engine::model::ModelBinding;
    pub use runvil_engine::model::ModelCatalog;
    pub use runvil_engine::model::ModelId;
    pub use runvil_engine::model::ModelInfo;
    pub use runvil_engine::model::ModelInvocation;
    pub use runvil_engine::model::ModelRole;
    pub use runvil_engine::model::RawExchange;
    pub use runvil_engine::model::Role;
    pub use runvil_engine::model::Temperature;
    pub use runvil_engine::model::ThinkingMode;
    pub use runvil_engine::model::ToolArguments;
    pub use runvil_engine::model::ToolCall;
    pub use runvil_engine::model::ToolSchema;
    pub use runvil_engine::model::make_completion_error;
    pub use runvil_engine::model::make_context_overflow_error;
    pub use runvil_engine::model::make_empty_reply_error;
    pub use runvil_engine::model::phrase;
}

pub mod packs {
    //! Pack ids, the `uses:` entries that declare Packs, and the global
    //! names a Pack's Lua prelude may define.

    pub use runvil_engine::packs::GlobalName;
    pub use runvil_engine::packs::PackId;
    pub use runvil_engine::packs::PackRef;
}

pub mod prompt {
    //! What a prompt's frontmatter declares: its files, Packs,
    //! arguments, and model roles.

    pub use runvil_engine::prompt::ArgDecl;
    pub use runvil_engine::prompt::ArgType;
    pub use runvil_engine::prompt::ArgsDecl;
    pub use runvil_engine::prompt::ArgsDeclIter;
    pub use runvil_engine::prompt::FileDecl;
    pub use runvil_engine::prompt::Frontmatter;
    pub use runvil_engine::prompt::RoleRequirements;
}

pub mod vfs {
    //! A run's files: the handle to the virtual filesystem a Host gives
    //! each run, the store every section shares, real directories beside
    //! it, the policy that decides what the run may change, the access
    //! each tool call receives through its context, and the origin each
    //! file operation carries to say who asked for it.

    pub use runvil_engine::vfs::Access;
    pub use runvil_engine::vfs::AcquireContext;
    pub use runvil_engine::vfs::AllowAll;
    pub use runvil_engine::vfs::Entry;
    pub use runvil_engine::vfs::ExecId;
    pub use runvil_engine::vfs::FileType;
    pub use runvil_engine::vfs::MemoryBackend;
    pub use runvil_engine::vfs::Mode;
    pub use runvil_engine::vfs::ModeHandle;
    pub use runvil_engine::vfs::ModePolicy;
    pub use runvil_engine::vfs::Op;
    pub use runvil_engine::vfs::OpEvent;
    pub use runvil_engine::vfs::Origin;
    pub use runvil_engine::vfs::PathReason;
    pub use runvil_engine::vfs::Policy;
    pub use runvil_engine::vfs::RealBackend;
    pub use runvil_engine::vfs::ScopeHandle;
    pub use runvil_engine::vfs::Stat;
    pub use runvil_engine::vfs::Verdict;
    pub use runvil_engine::vfs::Vfs;
    pub use runvil_engine::vfs::VfsAccess;
    pub use runvil_engine::vfs::VfsError;
    pub use runvil_engine::vfs::VfsPath;
    pub use runvil_engine::vfs::VfsPathBuf;
    pub use runvil_engine::vfs::VfsRef;
    pub use runvil_engine::vfs::VfsRefBuilder;
    pub use runvil_engine::vfs::Writability;
}

pub mod metrics {
    //! The token counts and timings of a model call.

    pub use runvil_engine::metrics::CallMetrics;
    pub use runvil_engine::metrics::ClientTiming;
    pub use runvil_engine::metrics::LlamaTimings;
    pub use runvil_engine::metrics::ToolCallEvent;
    pub use runvil_engine::metrics::Usage;
    pub use runvil_engine::metrics::VllmMetrics;
}
