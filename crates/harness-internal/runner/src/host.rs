//! What every run of one Host shares: [`Host`], its installed Packs, how
//! it finds prompts, and its timer, and [`Host::parse`], which finds a
//! prompt by its name, parses it, and checks its `uses:` list against the
//! Host's Packs.

use std::fmt::{self, Write as _};
use std::sync::Arc;
use std::time::Duration;

use runvil_engine::{Error, ErrorCode, Prompt, PromptFinder};
use runvil_plugin::{BoxFuture, Pack, PackId, PackInfo, PackRegistry};

use crate::packs::Packs;
use crate::performers::Timer;


struct Clock;

impl Timer for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()> {
        // TODO: consider tokio::sleep to avoid blocking the thread
        Box::pin(async move { std::thread::sleep(duration) })
    }
}


/// What every run of one Host shares: its installed Packs, how it finds
/// prompts, and its timer. A Host builds one and shares it behind an
/// `Arc`.
pub struct Host {
    /// Every Pack the Host installed in its registry, whose factories
    /// make each run's instances.
    pub packs: Packs,
    /// Finds each prompt's source by its name.
    pub prompts: Arc<dyn PromptFinder>,
    /// Performs every run's `Timer` effects.
    pub timer: Arc<dyn Timer>,
}

impl Host {
    /// Creates a new [`Host`] with the given folder containing prompts.
    /// 
    /// The `Host` is given a default implementation of timer using `Clock`.
    pub fn new(prompt_folder: impl AsRef<std::path::Path>) -> Self {
        let prompt_location = prompt_folder.as_ref().to_path_buf();

        let prompts = move | name: &str| {
            let prompt_location             = prompt_location.join(name);
            std::fs::read_to_string(&prompt_location)
                .map_err(|io| {
                    let code = ErrorCode::from(io.kind());
                    let message = format!("{}: {io}", prompt_location.display());
                    Error::new(code, message,code.prose())
                })

        };

        Self {
            packs: Packs::new(PackRegistry::new()),
            prompts: Arc::new(prompts),
            timer: Arc::new(Clock),
        }
    }

    /// Replace the current [`PromptFinder`] with `finder`.
    pub fn with_prompt_finder(mut self, finder: impl PromptFinder + 'static) -> Self {
        self.prompts = Arc::new(finder);
        self
    }

    /// Replace the current [`Packs`] with those installed by `registry`
    pub fn with_packs(mut self, registry: PackRegistry) -> Self {
        self.packs = Packs::new(registry);
        self
    }

    /// Replace the current [`Timer`] with `timer`.
    pub fn with_timer(mut self, timer: impl Timer + 'static) -> Self {
        self.timer = Arc::new(timer);
        self
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
