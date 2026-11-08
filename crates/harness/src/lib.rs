//! The Harness, which runs one prompt for a Host and records the run.
//!
//! A Host makes a [`pack::PackRegistry`], calls each Plugin it links,
//! which inserts its Packs into the registry, and builds one
//! [`pack::Packs`] over it, which keeps each Pack's factory.
//! It builds one [`Host`] at startup over those Packs, a
//! [`PromptFinder`], and its [`Timer`], which also stamps each run's
//! start as a [`Timestamp`], and shares it behind an `Arc`.
//! [`Host::parse`] finds a prompt by its name and parses it. For each run
//! the launcher builds a [`RunSetup`] with the run's recorder, inference
//! broker, services, filesystem, model selection, workspace roots,
//! [`RunLimits`], tool-call cap, arguments, input text, and whether a
//! user answers it, and [`Harness::new`] takes the Host, the parsed
//! prompt, and that setup. [`Harness::run`] runs the prompt to its end and
//! returns its [`RunReport`]. The run makes its own instance of each Pack
//! it holds, handing each factory the setup's services, and drops the
//! instances when it ends. While the run goes on, the launcher sends its
//! main chain messages and steers it through the [`RunControl`] it took
//! from `Harness::control` before the run started.
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
pub use harness_runner::display_chain;
pub use harness_runner::performers::BoxFuture;
pub use harness_runner::performers::InferenceBroker;
pub use harness_runner::performers::Timer;
pub use runvil::PromptFinder;
pub use runvil::RunLimits;
pub use runvil::timestamp::Timestamp;

pub mod pack {
    //! The registry a Host's Plugins insert Packs into, the Packs
    //! installed from it, whose factories make each run's instances, the
    //! services maps a Host fills, and the Pack ids. The Plugin crates
    //! themselves are the Host's own dependencies.

    pub use harness_runner::Packs;
    pub use runvil_plugin::HostServices;
    pub use runvil_plugin::PackId;
    pub use runvil_plugin::PackOrigin;
    pub use runvil_plugin::PackRegistry;
}

pub mod plugin {
    //! Inserting Plugins into the Pack registry: from the dynamic libraries
    //! in a Plugin directory, or linked into the Host, and what each insert
    //! loaded.

    pub use harness_plugin_loader::Plugin;
    pub use harness_plugin_loader::PluginReport;
    pub use harness_plugin_loader::PluginSource;
    pub use harness_plugin_loader::insert_plugin;
    pub use harness_plugin_loader::insert_plugin_dir;
}

pub mod record {
    //! The recorder that receives every effect, answer, and event of a run
    //! and the result each run ends with, an in-memory recorder, and the
    //! seed each run's meta records.

    pub use harness_runner::recorder::MemoryRecorder;
    pub use harness_runner::recorder::Record;
    pub use harness_runner::recorder::RecordKind;
    pub use harness_runner::recorder::RecorderFuture;
    pub use harness_runner::recorder::RunId;
    pub use harness_runner::recorder::RunMeta;
    pub use harness_runner::recorder::RunRecorder;
    pub use harness_runner::recorder::make_recorder_error;
    pub use runvil::RunSeed;
}

pub mod vfs {
    //! The handle to the virtual filesystem a Host gives each run, the
    //! access each tool call receives through its context, and the origin
    //! each file operation carries to say who asked for it.

    pub use runvil::vfs::Access;
    pub use runvil::vfs::Origin;
    pub use runvil::vfs::VfsError;
    pub use runvil::vfs::VfsRef;
}
