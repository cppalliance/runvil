//! A named run's inputs, its ordinary result, and access to its controls.

use std::future::IntoFuture;

use runvil_engine::{BoxFuture, Error};

use crate::recorder::of_run;
use crate::{Harness, Host, RunReport, RunSetup};

/// One configurable run, created by [`Host::run`].
///
/// Await this value to get a complete [`RunReport`], including execution
/// failures. Use [`text`](Self::text) or [`output`](Self::output) when a
/// failure should propagate through `?`, or [`prepare`](Self::prepare) to
/// get the [`Harness`] and take its control before executing it.
/// The builder owns a Host snapshot and its run settings, so it can
/// outlive the original Host and move into a spawned task.
#[derive(Debug)]
#[must_use = "a run request does nothing until awaited or prepared"]
pub struct RunBuilder {
    host: Host,
    name: String,
    setup: RunSetup,
}

impl RunBuilder {
    pub(crate) fn new(host: Host, name: String, setup: RunSetup) -> Self {
        Self { host, name, setup }
    }

    /// Passes argument text to the prompt as `args`.
    pub fn args(mut self, args: impl Into<String>) -> Self {
        self.setup.args = args.into();
        self
    }

    /// Stages text at the prompt's declared `input:` file.
    pub fn input(mut self, text: impl Into<String>) -> Self {
        self.setup.input_text = Some(text.into());
        self
    }

    /// Selects a model from the configured broker's catalog.
    /// Without a selection, the run uses the catalog's first model.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.setup.selected_model = Some(model.into());
        self
    }

    /// Enables the user inbox. Take the control from [`prepare`](Self::prepare)
    /// to answer it while the run is pending.
    pub fn interactive(mut self) -> Self {
        self.setup.has_user = true;
        self
    }

    /// Changes any run setting, including its filesystem, resource limits,
    /// workspace roots, tool concurrency, broker, or recorder. It receives
    /// this request's defaults, including the Host's broker and recorder.
    /// Ordinary inputs have the shorter [`args`](Self::args),
    /// [`input`](Self::input), and [`model`](Self::model) methods.
    pub fn configure(mut self, configure: impl FnOnce(&mut RunSetup)) -> Self {
        configure(&mut self.setup);
        self
    }

    /// Finds and parses the prompt, then returns its unstarted Harness.
    /// Take [`Harness::control`] before calling [`Harness::run`].
    /// Parsing makes no Pack and writes no run record.
    ///
    /// # Errors
    /// Returns the prompt finder's or parser's failure, or a refusal of
    /// the prompt's Pack requirements.
    pub fn prepare(self) -> Result<Harness, Error> {
        let prompt = self.host.parse(&self.name)?;
        Ok(Harness::new(self.host, prompt, self.setup))
    }

    /// Executes the prompt and returns its final text.
    /// Every execution failure propagates as an error carrying its run id.
    /// A prompt that returns no text produces an empty string.
    ///
    /// # Errors
    /// Returns a parse, recording, or execution failure.
    pub async fn text(self) -> Result<String, Error> {
        let report = self.await?;
        report.result.map_err(|error| of_run(report.run_id, error))
    }

    /// Executes the prompt and reads its declared `output:` file.
    /// An execution failure takes precedence over an absent output.
    ///
    /// # Errors
    /// Returns a parse, recording, execution, or output-read failure.
    /// `NoOutput` means the prompt declares no output file.
    pub async fn output(self) -> Result<String, Error> {
        let report = self.await?;
        report
            .result
            .map_err(|error| of_run(report.run_id, error))?;
        report.output.map_err(|error| of_run(report.run_id, error))
    }
}

impl IntoFuture for RunBuilder {
    type Output = Result<RunReport, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    /// Executes the request and returns the full report. Parse and
    /// recording failures are errors; execution failures are retained
    /// in [`RunReport::result`] beside the run id and unread messages.
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { self.prepare()?.run().await })
    }
}
