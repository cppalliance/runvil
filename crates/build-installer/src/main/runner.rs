//! The real subprocess runner and its cancel controller.

use std::io::{self, Read as _};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::Duration;

use super::{
    CancelController, CancelState, CommandResult, CommandRunner, CommandSpec, OutputMode,
    ProcessRunner,
};

impl CancelController {
    fn isolated() -> Self {
        Self {
            state: Arc::new(CancelState {
                generation: AtomicUsize::new(0),
                active_child: Mutex::new(None),
                termination_error: Mutex::new(None),
            }),
        }
    }

    fn generation(&self) -> usize {
        self.state.generation.load(Ordering::SeqCst)
    }

    fn request(&self) {
        let _ =
            self.state
                .generation
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |generation| {
                    Some(generation.saturating_add(1))
                });
        let mut active_child = self.lock_active_child();
        if let Some(child) = active_child.as_mut()
            && let Err(error) = child.kill()
            && error.kind() != io::ErrorKind::InvalidInput
        {
            let mut termination_error = self.lock_termination_error();
            if termination_error.is_none() {
                *termination_error = Some(error.to_string());
            }
        }
    }

    fn lock_active_child(&self) -> MutexGuard<'_, Option<Child>> {
        match self.state.active_child.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn lock_termination_error(&self) -> MutexGuard<'_, Option<String>> {
        match self.state.termination_error.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn clear_termination_error(&self) {
        *self.lock_termination_error() = None;
    }

    fn take_termination_error(&self) -> Option<String> {
        self.lock_termination_error().take()
    }
}

static PROCESS_CANCEL: OnceLock<Result<CancelController, Arc<anyhow::Error>>> = OnceLock::new();

pub(super) fn install_cancel_handler() -> Result<CancelController, Arc<anyhow::Error>> {
    match PROCESS_CANCEL.get_or_init(|| {
        let cancel = CancelController::isolated();
        let handler_cancel = cancel.clone();
        ctrlc::set_handler(move || handler_cancel.request()).map_err(|error| {
            Arc::new(anyhow::anyhow!(
                "cannot install the Ctrl-C handler: {error}"
            ))
        })?;
        Ok(cancel)
    }) {
        Ok(cancel) => Ok(cancel.clone()),
        Err(error) => Err(Arc::clone(error)),
    }
}

impl ProcessRunner {
    pub(super) fn new(cancel: CancelController) -> Self {
        Self { cancel }
    }

    fn canceled_error(&self) -> io::Error {
        match self.cancel.take_termination_error() {
            Some(error) => io::Error::new(
                io::ErrorKind::Interrupted,
                format!("canceled; active child termination failed: {error}"),
            ),
            None => io::Error::new(io::ErrorKind::Interrupted, "canceled"),
        }
    }

    fn finish_child(
        mut child: Child,
        status: ExitStatus,
        output_mode: OutputMode,
    ) -> io::Result<CommandResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        if output_mode == OutputMode::Capture {
            if let Some(mut pipe) = child.stdout.take() {
                pipe.read_to_string(&mut stdout)?;
            }
            if let Some(mut pipe) = child.stderr.take() {
                pipe.read_to_string(&mut stderr)?;
            }
        }
        Ok(CommandResult {
            success: status.success(),
            stdout,
            stderr,
        })
    }
}

impl CommandRunner for ProcessRunner {
    fn run(&mut self, command: &CommandSpec) -> io::Result<CommandResult> {
        let generation = self.cancel.generation();
        if generation != 0 {
            return Err(self.canceled_error());
        }

        let mut process = Command::new(&command.program);
        process
            .args(&command.args)
            .current_dir(&command.current_dir)
            .envs(command.envs.iter().map(|(key, value)| (key, value)));
        if command.output_mode == OutputMode::Capture {
            process.stdout(Stdio::piped()).stderr(Stdio::piped());
        }

        {
            let mut active_child = self.cancel.lock_active_child();
            if self.cancel.generation() != generation {
                return Err(self.canceled_error());
            }
            if active_child.is_some() {
                return Err(io::Error::other(
                    "another build subprocess is already active",
                ));
            }
            self.cancel.clear_termination_error();
            *active_child = Some(process.spawn()?);
        }

        loop {
            let completed = {
                let mut active_child = self.cancel.lock_active_child();
                let child = active_child
                    .as_mut()
                    .ok_or_else(|| io::Error::other("active build subprocess disappeared"))?;
                match child.try_wait()? {
                    Some(status) => active_child.take().map(|child| (child, status)),
                    None => None,
                }
            };
            if let Some((child, status)) = completed {
                let result = Self::finish_child(child, status, command.output_mode)?;
                if self.cancel.generation() != generation {
                    return Err(self.canceled_error());
                }
                return Ok(result);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn cancel_observed(&self) -> bool {
        self.cancel.generation() != 0
    }
}
