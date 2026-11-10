//! Runs Runvil prompts for a Host. This is the one crate a Host depends on.
//!
//! The crate holds three things: the Harness, which runs one prompt for a
//! Host and records the run; the Engine's items a Host reads and builds,
//! such as a [`Prompt`], an [`Error`], the events of a run, and a run's
//! files; and the Plugin contract. Code that steps a run itself, as the
//! Harness does, takes the Engine's stepping items from `runvil-engine`.
//!
//! A Host makes a [`plugin::PackRegistry`], calls each Plugin it links,
//! which inserts its Packs into the registry, and builds one
//! [`harness::Packs`] over it, which keeps each Pack's factory.
//! It builds one [`Host`] at startup over those Packs, a
//! [`PromptFinder`], and its [`harness::Timer`], which also stamps each
//! run's start as a [`timestamp::Timestamp`], and shares it behind an
//! `Arc`.
//! [`Host::parse`] finds a prompt by its name and parses it. For each run
//! the launcher builds a [`RunSetup`] with the run's recorder, inference
//! broker, filesystem, model selection, workspace roots, [`RunLimits`],
//! tool-call cap, arguments, input text, and whether a user answers it,
//! and [`Harness::new`] takes the Host, the parsed prompt, and that setup.
//! [`Harness::run`] runs the prompt to its end and returns its
//! [`RunReport`]. The run makes its own instance of each Pack it holds
//! and drops the instances when it ends. While the run goes on, the
//! launcher sends its main chain messages and steers it through the
//! [`RunControl`] it took from `Harness::control` before the run started.
//!
//! Every run gets a record and ends with its result: the final text, or
//! the error that ended it, where a cancel is the error whose code is
//! `canceled`. The record begins before anything else, so a run canceled
//! before it starts, or one whose model cannot be resolved, is recorded
//! too. The only error [`Harness::run`] returns is a `RecorderFailed`
//! refusal: a recorder that refuses a write. A prompt that fails to parse
//! is the caller's error from [`Host::parse`] and leaves no run record.
//!
//! The crate's `run-prompt` example runs one prompt from start to end.

pub use harness_runner::Harness;
pub use harness_runner::Host;
pub use harness_runner::RunControl;
pub use harness_runner::RunReport;
pub use harness_runner::RunSetup;

/* The Engine's prompts, run limits, and run seed
*/
pub use runvil_engine::Prompt;
pub use runvil_engine::PromptFinder;
pub use runvil_engine::RunLimits;
pub use runvil_engine::RunSeed;

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
    pub use runvil_plugin::ToolContext;
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

pub mod timestamp {
    //! The start time a Host's timer stamps each run with, which a prompt
    //! reads as `sys.when`.

    pub use runvil_engine::timestamp::Timestamp;
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
