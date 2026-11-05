//! Parses Runvil prompt files and runs them, leaving their outside work
//! to the caller.
//!
//! A [`Prompt`] is a parsed prompt file. A [`Run`] executes one prompt.
//! [`Run::step`] returns [`Step::Pending`] with the effects the run waits on.
//! The caller performs each effect and hands its answer to [`Run::resume`].
//! The run ends at [`Step::Done`] with its [`RunOutcome`].
//! Before the run starts, the caller puts on its [`RunParams`] the tools
//! a run may offer, the Packs it holds, and the role map that fills the
//! five model roles. The run's first step checks the params against what
//! the prompt requires, and params that fall short end the run there
//! with a [`RequirementsUnmet`](RunErrorKind::RequirementsUnmet) failure
//! that names every gap.
//!
//! The crate's `greeter` example is that whole loop in one program.

pub use runvil_engine::Run;
pub use runvil_engine::RunErrorKind;
pub use runvil_engine::RunLimits;
pub use runvil_engine::RunOutcome;
pub use runvil_engine::RunParams;
pub use runvil_engine::Step;
pub use runvil_prompt::Prompt;
pub use runvil_types::seed::RunSeed;

pub mod effect {
    //! The outside work a run asks for, the answers it takes back, and the
    //! records a run log stores for both.

    pub use runvil_engine::Effect;
    pub use runvil_engine::EffectAnswer;
    pub use runvil_engine::EffectId;
    pub use runvil_engine::IssuedEffect;
    pub use runvil_engine::PackToolSets;
    pub use runvil_engine::Round;
    pub use runvil_engine::ToolCallOrigin;
    pub use runvil_engine::ToolCaller;
}

pub mod event {
    //! The events a run reports for the caller to log.

    pub use runvil_types::event::Event;
    pub use runvil_types::event::ReplyOrigin;
}

pub mod error {
    //! The universal error: its code, its text for people and for the model,
    //! where it happened, and the values it names.

    pub use runvil_types::error::Error;
    pub use runvil_types::error::ErrorCode;
    pub use runvil_types::error::FactKey;
    pub use runvil_types::error::Location;
}

pub mod ids {
    //! Task, chain, and round ids, and the provenance that orders a run's
    //! records.

    pub use runvil_types::ids::AbandonReason;
    pub use runvil_types::ids::ChainId;
    pub use runvil_types::ids::Provenance;
    pub use runvil_types::ids::RoundId;
    pub use runvil_types::ids::TaskId;
}

pub mod model {
    //! Model descriptions and catalogs, the models a prompt's roles are
    //! bound to, and the chat vocabulary of a model round.

    pub use runvil_model_client::client::ArgumentNames;
    pub use runvil_model_client::client::Completion;
    pub use runvil_model_client::client::CompletionResult;
    pub use runvil_model_client::client::Message;
    pub use runvil_model_client::client::RawExchange;
    pub use runvil_model_client::client::Role;
    pub use runvil_model_client::client::ToolArguments;
    pub use runvil_model_client::client::ToolCall;
    pub use runvil_model_client::client::ToolSchema;
    pub use runvil_model_client::model::CompletionOptions;
    pub use runvil_model_client::model::ModelBinding;
    pub use runvil_model_client::model::ModelInvocation;
    pub use runvil_model_client::model::Temperature;
    pub use runvil_model_client::model::completion_error;
    pub use runvil_model_client::model::context_overflow;
    pub use runvil_model_client::model::empty_reply;
    pub use runvil_model_client::model::phrase;
    pub use runvil_types::models::ModelCatalog;
    pub use runvil_types::models::ModelId;
    pub use runvil_types::models::ModelInfo;
    pub use runvil_types::models::ModelRole;
    pub use runvil_types::models::ThinkingMode;
}

pub mod tools {
    //! Tool descriptions and catalogs, and the output or error that answers
    //! a tool call.

    pub use runvil_types::tools::OutputTrust;
    pub use runvil_types::tools::ToolCatalog;
    pub use runvil_types::tools::ToolId;
    pub use runvil_types::tools::ToolInfo;
    pub use runvil_types::tools::ToolOutput;
    pub use runvil_types::tools::tool_error;
}

pub mod packs {
    //! Pack ids, the `uses:` entries that declare Packs, how each Pack
    //! stands in a run's roster, the Lua preludes Packs add to a run, and
    //! the global names those preludes may define.

    pub use runvil_lua::RosterEntry;
    pub use runvil_types::names::GlobalName;
    pub use runvil_types::packs::PackId;
    pub use runvil_types::packs::PackRef;
    pub use runvil_types::packs::Prelude;
}

pub mod prompt {
    //! What a prompt's frontmatter declares: its files, Packs,
    //! arguments, and model roles.

    pub use runvil_prompt::ArgDecl;
    pub use runvil_prompt::ArgType;
    pub use runvil_prompt::ArgsDecl;
    pub use runvil_prompt::ArgsDeclIter;
    pub use runvil_prompt::FileDecl;
    pub use runvil_prompt::Frontmatter;
    pub use runvil_prompt::RoleRequirements;
}

pub mod vfs {
    //! A run's files: the store every section shares, real directories
    //! beside it, and the policy that decides what the run may change.

    pub use runvil_engine::perform_vfs_op;
    pub use runvil_lua::VfsOp;
    pub use runvil_lua::VfsOutcome;
    pub use runvil_lua::store_error;
    pub use runvil_vfs::Access;
    pub use runvil_vfs::AcquireContext;
    pub use runvil_vfs::AllowAll;
    pub use runvil_vfs::Entry;
    pub use runvil_vfs::ExecId;
    pub use runvil_vfs::FileType;
    pub use runvil_vfs::MemoryBackend;
    pub use runvil_vfs::Mode;
    pub use runvil_vfs::ModeHandle;
    pub use runvil_vfs::ModePolicy;
    pub use runvil_vfs::Op;
    pub use runvil_vfs::OpEvent;
    pub use runvil_vfs::Origin;
    pub use runvil_vfs::ParsePathReasonError;
    pub use runvil_vfs::PathReason;
    pub use runvil_vfs::Policy;
    pub use runvil_vfs::RealBackend;
    pub use runvil_vfs::Stat;
    pub use runvil_vfs::Verdict;
    pub use runvil_vfs::Vfs;
    pub use runvil_vfs::VfsAccess;
    pub use runvil_vfs::VfsError;
    pub use runvil_vfs::VfsPath;
    pub use runvil_vfs::VfsPathBuf;
    pub use runvil_vfs::VfsRef;
    pub use runvil_vfs::VfsRefBuilder;
    pub use runvil_vfs::Writability;
}

pub mod cancel {
    //! Cancel handles that stop runs and tasks from any thread.

    pub use runvil_types::cancel::CancelHandle;
    pub use runvil_types::cancel::Cancelled;
}

pub mod timestamp {
    //! The start time a run's params carry, which a prompt reads as
    //! `sys.when`.

    pub use runvil_types::timestamp::Timestamp;
}

pub mod metrics {
    //! The token counts and timings of a model call.

    pub use runvil_types::metrics::CallMetrics;
    pub use runvil_types::metrics::ClientTiming;
    pub use runvil_types::metrics::LlamaTimings;
    pub use runvil_types::metrics::ToolCallEvent;
    pub use runvil_types::metrics::Usage;
    pub use runvil_types::metrics::VllmMetrics;
}
