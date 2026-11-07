//! How a prompt's source is found by its name: [`PromptFinder`].

use runvil_types::error::Error;

/// Finds a prompt's source by its name, such as `research.md`. The name is
/// the prompt's identity: [`Prompt::file`](crate::Prompt::file) and every
/// failure use it.
pub trait PromptFinder: Send + Sync {
    /// The source of the prompt named `name`, or the error saying why it
    /// cannot be found or read.
    ///
    /// # Errors
    /// Whatever error the finder chooses; [`Prompt::parse`](crate::Prompt::parse)
    /// returns it unchanged.
    fn find(&self, name: &str) -> Result<String, Error>;
}

/// Any `Fn(&str) -> Result<String, Error>` is a finder, so a test or a
/// caller with one prompt needs no type of its own.
impl<Func> PromptFinder for Func
where
    Func: Fn(&str) -> Result<String, Error> + Send + Sync,
{
    fn find(&self, name: &str) -> Result<String, Error> {
        self(name)
    }
}
