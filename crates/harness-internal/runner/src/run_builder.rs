//! Configuration of one named run before it starts.

use std::future::IntoFuture;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

use runvil_engine::vfs::VfsRef;
use runvil_engine::{BoxFuture, Error, RunLimits};

use crate::performers::InferenceBroker;
use crate::recorder::RunRecorder;
use crate::{Harness, Host, RunReport, RunSetup};

/// A named run ready to configure and await, returned by [`Host::run`].
/// Each setter changes only that run. Awaiting parses the prompt and
/// records its execution. For interactive runs, [`prepare`](Self::prepare)
/// returns a Harness whose control can be taken before execution starts.
///
/// Awaiting returns a parse error before recording, or `RecorderFailed`
/// when recording fails. Execution failures remain in [`RunReport::result`].
#[derive(Debug)]
#[must_use = "a run must be awaited or prepared to do any work"]
pub struct RunBuilder {
    host: Host,
    name: String,
    setup: RunSetup,
}

impl RunBuilder {
    pub(crate) fn new(host: Host, name: String, setup: RunSetup) -> Self {
        Self { host, name, setup }
    }

    /// Sets the text written to the prompt's declared input file.
    pub fn input(mut self, text: impl Into<String>) -> Self {
        self.setup.input_text = Some(text.into());
        self
    }

    /// Sets the prompt's argument text.
    pub fn args(mut self, args: impl Into<String>) -> Self {
        self.setup.args = args.into();
        self
    }

    /// Selects a model from the broker's catalog. Without a selection,
    /// the run uses the catalog's first model when one exists.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.setup.selected_model = Some(model.into());
        self
    }

    /// Overrides the Host's inference broker for this run.
    pub fn broker(mut self, broker: impl InferenceBroker + 'static) -> Self {
        self.setup.broker = Arc::new(broker);
        self
    }

    /// Overrides the Host's recorder for this run. Pass a shared `Arc`
    /// when the caller will inspect the recorded history afterward.
    pub fn recorder(mut self, recorder: impl RunRecorder + 'static) -> Self {
        self.setup.recorder = Arc::new(recorder);
        self
    }

    /// Replaces the fresh in-memory filesystem with the supplied one.
    pub fn vfs(mut self, vfs: VfsRef) -> Self {
        self.setup.vfs = vfs;
        self
    }

    /// Sets resource limits for this run.
    pub fn limits(mut self, limits: RunLimits) -> Self {
        self.setup.limits = limits;
        self
    }

    /// Limits the number of tool calls in flight; the default is ten.
    pub fn tool_concurrency(mut self, maximum: NonZeroUsize) -> Self {
        self.setup.max_tool_calls_in_flight = maximum;
        self
    }

    /// Sets the workspace roots reported to the prompt. This does not
    /// grant filesystem access; configure that through [`vfs`](Self::vfs).
    pub fn workspace_roots(mut self, roots: impl IntoIterator<Item = PathBuf>) -> Self {
        self.setup.workspace_roots = roots.into_iter().collect();
        self
    }

    /// Enables the user inbox. Use [`prepare`](Self::prepare) and the
    /// resulting Harness's control to send the user's replies.
    pub fn with_user(mut self) -> Self {
        self.setup.has_user = true;
        self
    }

    /// Replaces all run settings, including inherited broker and recorder,
    /// with an explicit setup. Later setters can override individual fields.
    pub fn setup(mut self, setup: RunSetup) -> Self {
        self.setup = setup;
        self
    }

    /// Parses the named prompt and returns a Harness without starting it.
    /// The Harness owns its configuration and can be moved to another task.
    ///
    /// # Errors
    /// Returns [`Host::parse`]'s failure before any run is recorded.
    pub fn prepare(self) -> Result<Harness, Error> {
        let prompt = self.host.parse(&self.name)?;
        Ok(Harness::new(self.host, prompt, self.setup))
    }
}

impl IntoFuture for RunBuilder {
    type Output = Result<RunReport, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { self.prepare()?.run().await })
    }
}
