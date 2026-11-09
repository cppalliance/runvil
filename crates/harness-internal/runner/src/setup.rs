//! What one run gets on top of its Host: [`RunSetup`].

use std::fmt;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

use runvil::RunLimits;
use runvil::vfs::VfsRef;

use crate::performers::InferenceBroker;
use crate::recorder::RunRecorder;

/// The tool calls a run has in flight at once unless its setup says
/// otherwise.
const DEFAULT_MAX_TOOL_CALLS_IN_FLIGHT: NonZeroUsize = NonZeroUsize::MIN.saturating_add(9);

/// Everything one run gets on top of its Host. The launcher builds one
/// per run, and the Harness reads each field once and keeps it for the
/// run.
pub struct RunSetup {
    /// Where the run's history is written.
    pub recorder: Arc<dyn RunRecorder>,
    /// Lists the models and performs model rounds.
    pub broker: Arc<dyn InferenceBroker>,
    /// The run's whole filesystem: the input is staged in it, the run
    /// works in it, and the declared output is read from it.
    pub vfs: VfsRef,
    /// The model id the user selected, if any.
    pub selected_model: Option<String>,
    /// The workspace roots granted; the first is the `ui()` snapshot's
    /// `workspace_root`.
    pub workspace_roots: Vec<PathBuf>,
    /// The run's argument text.
    pub args: String,
    /// Text written to the prompt's `input:` file before the run starts.
    pub input_text: Option<String>,
    /// The run's resource limits.
    pub limits: RunLimits,
    /// The most tool calls in flight at once; the
    /// rest wait and start oldest first.
    pub max_tool_calls_in_flight: NonZeroUsize,
    /// A user answers the main chain's inbox, so the
    /// run holds `runvil/user`, which is unavailable
    /// otherwise.
    pub has_user: bool,
}

impl RunSetup {
    /// A setup over `recorder` and `broker`. Every other field starts
    /// empty or at its default: a fresh memory store, no selection, no
    /// roots, no args, no input, `RunLimits::default()`, at most 10 tool
    /// calls in flight, and no user.
    #[must_use]
    pub fn new(recorder: Arc<dyn RunRecorder>, broker: Arc<dyn InferenceBroker>) -> RunSetup {
        RunSetup {
            recorder,
            broker,
            vfs: VfsRef::default(),
            selected_model: None,
            workspace_roots: Vec::new(),
            args: String::new(),
            input_text: None,
            limits: RunLimits::default(),
            max_tool_calls_in_flight: DEFAULT_MAX_TOOL_CALLS_IN_FLIGHT,
            has_user: false,
        }
    }
}

impl fmt::Debug for RunSetup {
    /// Shows every field but the recorder and broker.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunSetup")
            .field("vfs", &self.vfs)
            .field("selected_model", &self.selected_model)
            .field("workspace_roots", &self.workspace_roots)
            .field("args", &self.args)
            .field("input_text", &self.input_text)
            .field("limits", &self.limits)
            .field("max_tool_calls_in_flight", &self.max_tool_calls_in_flight)
            .field("has_user", &self.has_user)
            .finish_non_exhaustive()
    }
}
