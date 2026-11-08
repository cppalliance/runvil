//! What every run of one Host shares: [`Host`], its installed Packs, how
//! it finds prompts, and its timer, and [`Host::parse`], which finds a
//! prompt by its name, parses it, and checks its `uses:` list against the
//! Host's Packs.

use std::fmt::{self, Write as _};
use std::sync::Arc;

use runvil::{Error, ErrorCode, Prompt, PromptFinder};

use crate::packs::Packs;
use crate::performers::Timer;

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
    /// Parses the prompt `name` with `Prompt::parse` over `prompts`, then
    /// checks its `uses:` against `packs`. Returns the parse failure, a
    /// finder's included, as given, or `RequirementsUnmet` naming each
    /// Pack that `Packs::refusals` reports, with its reason: one not
    /// installed, or inserted unavailable. Parsing makes no Pack, so a
    /// factory that would refuse a run is refused at that run's first step
    /// instead.
    ///
    /// The parse events go to `tracing` at debug level; no run exists yet
    /// to record them.
    ///
    /// # Errors
    /// Returns the finder's or the parser's failure unchanged, and
    /// `RequirementsUnmet` whose message and prose list each declared
    /// Pack this Host cannot serve with the reason.
    pub fn parse(&self, name: &str) -> Result<Prompt, Error> {
        let (parsed, events) = Prompt::parse(&*self.prompts, name);
        for event in &events {
            tracing::debug!(prompt = name, ?event, "prompt parse event");
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
