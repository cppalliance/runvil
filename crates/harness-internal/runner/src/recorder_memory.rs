//! The in-memory recorder: every run's records in a vector.

use std::sync::{Mutex, MutexGuard, PoisonError};

use runvil::{Error, ErrorFact};
use serde_json::Value;

use crate::recorder::{Record, RecorderFuture, RunId, RunMeta, RunRecorder, recorder_error};

/// A recorder that keeps every run in memory, for tests and doc examples.
///
/// It forgets everything when dropped. Run ids start at `1` and count up
/// in the order runs begin. A write to an id outside the runs this recorder
/// began, a write to a run that has ended, and a second
/// [`end_run`](RunRecorder::end_run) each fail, so a test notices a Harness
/// that breaks the call order. Each such failure has the `Reason` fact
/// `unknown_run` or `run_ended` and the run id as the `Run` fact.
#[derive(Debug, Default)]
pub struct MemoryRecorder {
    runs: Mutex<Vec<MemoryRun>>,
}

#[derive(Debug)]
struct MemoryRun {
    meta: RunMeta,
    records: Vec<Record>,
    result: Option<Result<String, Error>>,
}

impl MemoryRecorder {
    /// An empty recorder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the records of run `run` in the order they were appended.
    ///
    /// The list is empty when `run` lies outside the runs this recorder began.
    #[must_use]
    pub fn records(&self, run: RunId) -> Vec<Record> {
        self.lock()
            .get(slot(run))
            .map(|found| found.records.clone())
            .unwrap_or_default()
    }

    /// Returns the `RunMeta` that run `run` began with.
    ///
    /// Returns `None` when `run` lies outside the runs this recorder began.
    #[must_use]
    pub fn meta(&self, run: RunId) -> Option<RunMeta> {
        self.lock().get(slot(run)).map(|found| found.meta.clone())
    }

    /// Returns the result run `run` ended with: its final text, or the
    /// error that ended it.
    ///
    /// Returns `None` while the run is still open, or when `run` lies outside
    /// the runs this recorder began.
    #[must_use]
    pub fn result(&self, run: RunId) -> Option<Result<String, Error>> {
        self.lock()
            .get(slot(run))
            .and_then(|found| found.result.clone())
    }

    fn lock(&self) -> MutexGuard<'_, Vec<MemoryRun>> {
        // Every writer leaves the vector whole between its pushes and
        // stores, so a poisoned lock still holds a usable value.
        self.runs.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Where run `run` sits in the vector: id `1` is the first run. An id the
/// recorder never issued maps past the end, so a lookup misses.
fn slot(run: RunId) -> usize {
    usize::try_from(run.get())
        .ok()
        .and_then(|id| id.checked_sub(1))
        .unwrap_or(usize::MAX)
}

/// The run `run`, if it exists and still takes writes.
fn open_run(runs: &mut [MemoryRun], run: RunId) -> Result<&mut MemoryRun, Error> {
    let found = runs.get_mut(slot(run)).ok_or_else(|| {
        refused(
            run,
            "unknown_run",
            format!("memory recorder: unknown run {run}"),
        )
    })?;
    if found.result.is_some() {
        return Err(refused(
            run,
            "run_ended",
            format!("memory recorder: run {run} has ended"),
        ));
    }
    Ok(found)
}

/// A write to `run` the recorder refuses for `reason`, with `text` as its
/// cause.
fn refused(run: RunId, reason: &str, text: String) -> Error {
    let mut error = recorder_error(text);
    error.facts.insert(ErrorFact::Reason, Value::from(reason));
    error.facts.insert(ErrorFact::Run, Value::from(run.get()));
    error
}

impl RunRecorder for MemoryRecorder {
    fn begin_run(&self, meta: RunMeta) -> RecorderFuture<'_, RunId> {
        Box::pin(async move {
            let mut runs = self.lock();
            let id = i64::try_from(runs.len() + 1).map_err(recorder_error)?;
            runs.push(MemoryRun {
                meta,
                records: Vec::new(),
                result: None,
            });
            Ok(RunId::from_raw(id))
        })
    }

    fn append(&self, run: RunId, record: Record) -> RecorderFuture<'_, ()> {
        Box::pin(async move {
            let mut runs = self.lock();
            open_run(&mut runs, run).map(|open| open.records.push(record))
        })
    }

    fn end_run(&self, run: RunId, result: Result<String, Error>) -> RecorderFuture<'_, ()> {
        Box::pin(async move {
            let mut runs = self.lock();
            open_run(&mut runs, run).map(|open| open.result = Some(result))
        })
    }
}
