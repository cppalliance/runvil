//! End-to-end tests for the loader against the fixture's real dynamic
//! library: `insert_plugin_dir` opens it, loads its Plugin, inserts its
//! Pack, and closes it again, and a prompt calls its tools through the
//! Harness.
#![expect(
    clippy::expect_used,
    reason = "test helpers panic on setup failure, which is the desired behavior"
)]
#![expect(
    unsafe_code,
    reason = "reads the fixture's counters through symbols of a library the loader opened"
)]

mod insert;
mod lifecycle;
mod run;
mod support;
