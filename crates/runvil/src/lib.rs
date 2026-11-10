//! The Harness, which runs one prompt for a Host and records the run.
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
pub use runvil_engine::PromptFinder;
pub use runvil_engine::RunLimits;
pub use runvil_engine::RunSeed;

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
    pub use harness_runner::performers::BoxFuture;
    pub use harness_runner::performers::InferenceBroker;
    pub use harness_runner::performers::Timer;
    pub use harness_runner::recorder::MemoryRecorder;
    pub use harness_runner::recorder::Record;
    pub use harness_runner::recorder::RecordKind;
    pub use harness_runner::recorder::RecorderFuture;
    pub use harness_runner::recorder::RunId;
    pub use harness_runner::recorder::RunMeta;
    pub use harness_runner::recorder::RunRecorder;
    pub use harness_runner::recorder::make_recorder_error;
}

pub mod packs {
    //! Pack ids.

    pub use runvil_engine::packs::PackId;
}

pub mod plugin {
    //! The registry a Host's Plugins insert Packs into, where each of its
    //! Packs came from, and the services map a Host fills for its Plugins.
    //! The Plugin crates themselves are the Host's own dependencies.

    pub use runvil_plugin::HostServices;
    pub use runvil_plugin::PackOrigin;
    pub use runvil_plugin::PackRegistry;
}

pub mod timestamp {
    //! The start time a Host's timer stamps each run with.

    pub use runvil_engine::timestamp::Timestamp;
}

pub mod vfs {
    //! The handle to the virtual filesystem a Host gives each run, the
    //! access each tool call receives through its context, and the origin
    //! each file operation carries to say who asked for it.

    pub use runvil_engine::vfs::Access;
    pub use runvil_engine::vfs::Origin;
    pub use runvil_engine::vfs::VfsError;
    pub use runvil_engine::vfs::VfsRef;
}
