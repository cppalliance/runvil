//! The recorder seam: where the Harness writes a run's history, and the
//! values it writes.
//!
//! The Harness hands a [`RunRecorder`] each run's start, then every
//! effect, answer, and event in effect-loop order, then its end. The
//! Host supplies the recorder, so the Harness holds no database and no
//! storage path. [`MemoryRecorder`] keeps runs in memory for tests and
//! simple Hosts.
//!
//! The Harness awaits each call before it goes on, so a step's events
//! reach the recorder before the step's effects start. Calls within one
//! run never overlap. Calls from different runs may, so an implementation
//! serializes its own state. A recorder that fails stops its run: the
//! Harness never skips a record. A Host that prefers to keep running logs
//! the failure inside its recorder and returns `Ok`.
//!
//! A run ends with its result: the final text, or the error that ended
//! it, where a cancel is the error whose code is `canceled`. A run the
//! Engine runs ends with the result its last step returned. A run that
//! ends before the Engine runs it ends with an error of its own: the
//! cancel, when a cancel comes before or during the model lookup, or the
//! failure itself, when the model lookup fails, the prompt does not
//! parse, or its input cannot be put in place. A run the effect loop
//! loses track of ends with an `Internal` error.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use runvil::RunSeed;
use runvil::error::{Error, ErrorCode, FactKey};

pub use crate::recorder_memory::MemoryRecorder;

/// The future that each [`RunRecorder`] method returns.
///
/// It is boxed, can be sent between threads, and borrows the recorder.
pub type RecorderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, Error>> + Send + 'a>>;

/// Builds the `RecorderFailed` error a recorder returns when it fails to
/// store a write, from the error or the message that made it fail.
///
/// The Harness stops the run that received it. Its message is `the run
/// recorder failed: ` followed by the cause chain, and its prose is the
/// code's default.
#[must_use]
pub fn recorder_error(cause: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Error {
    let cause = cause.into();
    let code = ErrorCode::RecorderFailed;
    Error::new(code, "the run recorder failed", code.prose()).with_cause(&*cause)
}

/// Whether `error` is a recorder's refusal, which ends no run, rather than
/// the result a run ended with: its code is `RecorderFailed`.
///
/// It tells the two apart in what `prepare` and the effect loop return.
/// Both pass every recorder error through [`refused`], and no result of
/// theirs has that code: the Engine turns a caller code into `Internal`,
/// and the failures they make themselves are parse, input, and `Internal`
/// failures.
pub(crate) fn is_refusal(error: &Error) -> bool {
    error.code == ErrorCode::RecorderFailed
}

/// A recorder's refusal of a write for run `run`: `RecorderFailed` with
/// the run's `Run` fact, the refusal built with [`recorder_error`] when
/// the recorder returned another code.
pub(crate) fn refused(run: RunId, error: Error) -> Error {
    let error = if is_refusal(&error) {
        error
    } else {
        recorder_error(error)
    };
    of_run(run, error)
}

/// `error` with run `run`'s id as its `Run` fact.
pub(crate) fn of_run(run: RunId, mut error: Error) -> Error {
    error.facts.insert(FactKey::Run, run.get().into());
    error
}

/// The storage that the Harness writes each run's history to.
///
/// For each run, the Harness calls [`begin_run`](Self::begin_run) once,
/// then [`append`](Self::append) for every record, then
/// [`end_run`](Self::end_run) once. The recorder issues each run's
/// [`RunId`]. If a write returns an error, the Harness stops the run it
/// belongs to and hands the error on as a `RecorderFailed` refusal,
/// whatever code the recorder gave it.
pub trait RunRecorder: Send + Sync {
    /// Opens a run's history and returns its id.
    ///
    /// # Errors
    ///
    /// Returns the recorder's own failure, built with [`recorder_error`].
    fn begin_run(&self, meta: RunMeta) -> RecorderFuture<'_, RunId>;

    /// Adds one record to an open run.
    ///
    /// # Errors
    ///
    /// Returns the recorder's own failure, built with [`recorder_error`].
    fn append(&self, run: RunId, record: Record) -> RecorderFuture<'_, ()>;

    /// Closes a run with its result: the run's final text, or the error
    /// that ended it, where a cancel is the error whose code is
    /// `canceled`. The Harness calls it once for each run it began.
    ///
    /// # Errors
    ///
    /// Returns the recorder's own failure, built with [`recorder_error`].
    fn end_run(&self, run: RunId, result: Result<String, Error>) -> RecorderFuture<'_, ()>;
}

/// The id a recorder assigns to a run.
///
/// [`RunRecorder::begin_run`] returns it. It means something only to the
/// recorder that issued it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RunId(i64);

impl RunId {
    /// Wraps a raw id, for callers that stored one.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// The raw id.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What the Harness knows about a run when it begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunMeta {
    /// The run's name, as the Host requested it.
    pub name: String,
    /// A content hash of the prompt file, so a transcript can be matched
    /// to the exact text that produced it.
    pub prompt_hash: String,
    /// The seed the Harness drew and passed to the Engine. A replay passes
    /// it back, and the run's id derives from it.
    pub seed: RunSeed,
    /// Reserved. No flag is defined, and the Harness records 0.
    pub flags: u32,
    /// When the run started, in UTC milliseconds since the Unix epoch.
    /// The Engine receives the same value as `started_at`, so the record
    /// and the run agree.
    pub started_at: i64,
}

/// Which side of the effect loop a record came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordKind {
    /// An effect the Engine issued. The record's payload is the JSON
    /// `Effect::record` returns.
    Effect,
    /// The answer to an effect. The record's payload is the JSON
    /// `EffectAnswer::record` returns.
    Answer,
    /// An event the Engine emitted. The record's payload is an `Event`.
    Event,
}

impl RecordKind {
    /// The text that stores this kind: `effect`, `answer`, or `event`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Effect => "effect",
            Self::Answer => "answer",
            Self::Event => "event",
        }
    }

    /// Parses the stored text of a kind. Returns `None` for any other
    /// text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "effect" => Some(Self::Effect),
            "answer" => Some(Self::Answer),
            "event" => Some(Self::Event),
            _ => None,
        }
    }
}

/// One entry in a run's history, as the Harness appends it.
///
/// The recorder decides each record's position and time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The id of the nearest enclosing task, in the text form of the
    /// Engine's `TaskId`.
    ///
    /// The id is a dot-separated path of child indices. The run's root
    /// task is `0`, and its second child task is `0.1`.
    pub task_id: String,
    /// The record's position within its task.
    pub task_seq: u32,
    /// Which side of the effect loop the record came from.
    pub kind: RecordKind,
    /// The in-flight effect handle, for effects and their answers.
    pub effect_id: Option<u64>,
    /// The effect's or answer's record, or the serialized `Event`.
    pub payload: serde_json::Value,
}
