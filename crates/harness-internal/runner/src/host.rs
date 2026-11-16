//! What every run of one Host shares: [`Host`], its installed Packs, how
//! it finds prompts, and its timer, and [`Host::parse`], which finds a
//! prompt by its name, parses it, and checks its `uses:` list against the
//! Host's Packs.

use std::fmt::{self, Write as _};
use std::sync::Arc;
use std::time::Duration;

use runvil_engine::{Error, ErrorCode, ErrorFact, Prompt, PromptFinder};
use runvil_plugin::{BoxFuture, PackRegistry};

use crate::packs::Packs;
use crate::performers::{InferenceBroker, Timer};
use crate::recorder::{MemoryRecorder, RunRecorder};
use crate::{RunBuilder, RunSetup};

/// An executor-independent timer backed by the timer library's shared thread.
struct Clock;

impl Timer for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()> {
        Box::pin(futures_timer::Delay::new(duration))
    }
}

/// What every run of one Host shares: its installed Packs, how it finds
/// prompts, its timer, and its default broker and recorder. Configure it
/// once, then call [`run`](Self::run) for each invocation. Clones share
/// services and Pack factories; every run gets fresh Pack instances and
/// a fresh in-memory filesystem unless the caller supplies one.
#[derive(Clone)]
pub struct Host {
    /// Every Pack the Host installed in its registry, whose factories
    /// make each run's instances.
    pub(crate) packs: Arc<Packs>,
    /// Finds each prompt's source by its name.
    pub(crate) prompts: Arc<dyn PromptFinder>,
    /// Performs every run's `Timer` effects.
    pub(crate) timer: Arc<dyn Timer>,
    broker: Arc<dyn InferenceBroker>,
    recorder: Option<Arc<dyn RunRecorder>>,
}

impl Host {
    /// Creates a new [`Host`] with the given folder containing prompts.
    ///
    /// Uses an executor-independent, nonblocking timer and the built-in
    /// Packs. Prompt paths are resolved relative to the folder; this
    /// finder does not confine paths to that folder.
    #[must_use]
    pub fn new(prompt_folder: impl AsRef<std::path::Path>) -> Self {
        let folder = prompt_folder.as_ref().to_path_buf();
        Self {
            packs: Arc::new(Packs::new(PackRegistry::new())),
            prompts: Arc::new(move |name: &str| {
                let path = folder.join(name);
                std::fs::read_to_string(&path).map_err(|io| {
                    let code = ErrorCode::from(io.kind());
                    let mut error = Error::new(
                        code,
                        format!(
                            "the prompt `{name}` could not be read from `{}`",
                            path.display()
                        ),
                        code.prose(),
                    )
                    .with_cause(&io);
                    error.facts.insert(ErrorFact::Name, name.into());
                    error
                        .facts
                        .insert(ErrorFact::Path, path.to_string_lossy().as_ref().into());
                    error
                })
            }),
            timer: Arc::new(Clock),
            broker: Arc::new(crate::offline::Offline),
            recorder: None,
        }
    }

    /// Replaces the current [`PromptFinder`] with `finder`.
    #[must_use]
    pub fn with_prompt_finder(mut self, finder: impl PromptFinder + 'static) -> Self {
        self.prompts = Arc::new(finder);
        self
    }

    /// Replaces the current [`Packs`] with those installed by `registry`.
    #[must_use]
    pub fn with_packs(mut self, registry: impl Into<Packs>) -> Self {
        self.packs = Arc::new(registry.into());
        self
    }

    /// Replaces the current [`Timer`] with `timer`.
    #[must_use]
    pub fn with_timer(mut self, timer: impl Timer + 'static) -> Self {
        self.timer = Arc::new(timer);
        self
    }

    /// Uses `broker` for every run unless that run overrides it.
    /// Accepts a concrete broker or an `Arc` shared with the application.
    #[must_use]
    pub fn with_broker(mut self, broker: impl InferenceBroker + 'static) -> Self {
        self.broker = Arc::new(broker);
        self
    }

    /// Records every run through `recorder` unless that run overrides it.
    /// Without this setting each run has its own temporary memory record.
    #[must_use]
    pub fn with_recorder(mut self, recorder: impl RunRecorder + 'static) -> Self {
        self.recorder = Some(Arc::new(recorder));
        self
    }

    /// Configures a named run. Await it for a full report, use
    /// [`RunBuilder::text`] or [`RunBuilder::output`] for a result, or call
    /// [`RunBuilder::prepare`] to take its control before starting it.
    /// Nothing is parsed or executed until then. The builder owns its
    /// configuration and can outlive this Host.
    pub fn run(&self, name: impl Into<String>) -> RunBuilder {
        let recorder = self
            .recorder
            .clone()
            .unwrap_or_else(|| Arc::new(MemoryRecorder::new()));
        let setup = RunSetup::new(recorder, Arc::clone(&self.broker));
        RunBuilder::new(self.clone(), name.into(), setup)
    }

    /// Parses the prompt `name` with `Prompt::parse` over `prompts`, then
    /// checks its `uses:` against `packs`. Returns the parse failure, a
    /// finder's included, as given, or `RequirementsUnmet` naming each
    /// Pack that `Packs::refusals` reports, with its reason: one not
    /// installed, or inserted unavailable. Parsing makes no Pack, so a
    /// factory that would refuse a run is refused at that run's first step
    /// instead.
    ///
    /// The parse's outcome goes to `tracing` at debug level, as one line;
    /// no run exists yet to record it.
    ///
    /// # Errors
    /// Returns the finder's or the parser's failure unchanged, and
    /// `RequirementsUnmet` whose message and prose list each declared
    /// Pack this Host cannot serve with the reason.
    pub fn parse(&self, name: &str) -> Result<Prompt, Error> {
        let parsed = Prompt::parse(&*self.prompts, name);
        match &parsed {
            Ok(_) => tracing::debug!(prompt = name, "prompt parsed"),
            Err(error) => tracing::debug!(prompt = name, %error, "prompt failed to parse"),
        }
        let prompt = parsed?;
        let refusals = self.packs.refusals(&prompt);
        if refusals.is_empty() {
            return Ok(prompt);
        }
        let mut notice = String::from("the environment cannot satisfy this prompt:");
        for (id, reason) in &refusals {
            // Writing to a String is infallible.
            let _ = write!(notice, "\n- {id} is unavailable: {reason}");
        }
        Err(Error::new(
            ErrorCode::RequirementsUnmet,
            notice.clone(),
            notice,
        ))
    }
}

impl fmt::Debug for Host {
    /// Shows the Packs; omits the finder and the timer.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Host")
            .field("packs", &self.packs)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "host-tests.rs"]
mod tests;
