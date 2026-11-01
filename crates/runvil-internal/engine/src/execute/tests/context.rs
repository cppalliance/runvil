//! Shared config builders, prompt sources, parsers, stores, task ids, and
//! recorders for the execution suites.

use runvil_parser::test_support::PromptSource;
use runvil_types::ids::TaskId;

use super::*;
use crate::test_support::scenario::{EXECUTION, TEST_SEED, TEST_STARTED_AT};

/// The scaffold the fanout-arm capability tests share: a `## Parent` that
/// fans `### Worker` out over one `alpha` member and returns the first arm's
/// text. The worker body and its sibling sections follow.
pub(super) fn arm_fanout() -> PromptSource {
    PromptSource::new().section(
        "Parent",
        "
        local r = tasks.fanout('### Worker', {'alpha'})
        return r[1].text",
    )
}

/// The task `id` names.
#[track_caller]
pub(super) fn task(id: &str) -> TaskId {
    id.parse().expect("a task id parses")
}

/// The `(section, detail)` sequence the run itself reported, as the suites'
/// observer sees it: the parse events ahead of it are left out.
pub(super) fn sequence(trace: &Trace) -> Vec<(String, String)> {
    trace
        .run_records()
        .into_iter()
        .map(|record| (record.section, record.detail))
        .collect()
}

/// A [`RunConfig`] for the run `name` under the fixed test inputs.
pub(super) fn test_config(name: impl Into<String>) -> RunConfig {
    RunConfig::new(name, TEST_SEED, TEST_STARTED_AT)
}

/// Compile-time proof that the public execution types are thread-safe.
///
/// `RunConfig` holds `Arc<dyn Observer>` / `Arc<dyn DebugCapture>` (shared
/// trait objects) and must be `Send + Sync + 'static` to cross the run's task
/// boundaries; the typed error/limit/result surfaces must be too.
const fn _public_execution_types_are_send_sync_static() {
    const fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync_static::<RunConfig>();
    assert_send_sync_static::<RunLimits>();
    assert_send_sync_static::<RunOutcome>();
    assert_send_sync_static::<RunErrorKind>();
}

/// Parses `md`, adding a `# Test prompt` title when the source has none.
pub(super) fn parse(md: &str) -> Prompt {
    let source = if md.lines().any(|line| line.starts_with("# ")) {
        md.to_string()
    } else {
        md.replacen("---\n\n", "---\n\n# Test prompt\n\n", 1)
    };
    Prompt::parse(&source, EXECUTION).0.unwrap()
}

/// An [`Observer`] that keeps every observation it is handed, in order, so a test
/// can assert on the whole sequence rather than on a count.
#[derive(Default)]
pub(super) struct Recorder(Mutex<Vec<(String, String, String)>>);

impl Observer for Recorder {
    fn observe(&self, execution: &str, section: &str, event: Observation) {
        self.0
            .lock()
            .expect("the recorder mutex must not be poisoned")
            .push((execution.to_owned(), section.to_owned(), event.to_string()));
    }
}

impl Recorder {
    /// The full correlated records recorded so far, in order.
    pub(super) fn records(&self) -> Vec<(String, String, String)> {
        self.0
            .lock()
            .expect("the recorder mutex must not be poisoned")
            .clone()
    }

    /// The observations recorded so far, in order.
    pub(super) fn events(&self) -> Vec<(String, String)> {
        self.0
            .lock()
            .expect("the recorder mutex must not be poisoned")
            .iter()
            .map(|(_, section, detail)| (section.clone(), detail.clone()))
            .collect()
    }
}
