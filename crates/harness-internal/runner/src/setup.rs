//! What one run gets on top of its Host: [`RunSetup`].

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use runvil::vfs::VfsRef;
use runvil_plugin::HostServices;

use crate::performers::InferenceBroker;
use crate::recorder::RunRecorder;

/// Everything one run gets on top of its Host. The launcher builds one
/// per run, and the Harness reads each field once and keeps it for the
/// run.
pub struct RunSetup {
    /// Where the run's history is written.
    pub recorder: Arc<dyn RunRecorder>,
    /// Lists the models and performs model rounds.
    pub broker: Arc<dyn InferenceBroker>,
    /// What each of the run's tool calls is lent.
    pub services: HostServices,
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
}

impl RunSetup {
    /// A setup over `recorder` and `broker`. Every other field starts
    /// empty: no services, a fresh memory store, no selection, no roots,
    /// no args, and no input.
    #[must_use]
    pub fn new(recorder: Arc<dyn RunRecorder>, broker: Arc<dyn InferenceBroker>) -> RunSetup {
        RunSetup {
            recorder,
            broker,
            services: HostServices::new(),
            vfs: VfsRef::default(),
            selected_model: None,
            workspace_roots: Vec::new(),
            args: String::new(),
            input_text: None,
        }
    }
}

impl fmt::Debug for RunSetup {
    /// Shows every field but the recorder and broker.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunSetup")
            .field("services", &self.services)
            .field("vfs", &self.vfs)
            .field("selected_model", &self.selected_model)
            .field("workspace_roots", &self.workspace_roots)
            .field("args", &self.args)
            .field("input_text", &self.input_text)
            .finish_non_exhaustive()
    }
}
