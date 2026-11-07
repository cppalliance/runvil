//! The run log's tables.
//!
//! Two tables. `runs` holds one row per run: written at `begin_run`, closed
//! exactly once at `end_run` when `ended_at` and the result columns fill.
//! `status` is `completed`, `failed`, or `canceled`. A completed run fills
//! `final_text`; a failed or canceled run fills `error_code` with its
//! error's `ErrorCode::tag` and `error_message` with its located message.
//! `records` holds, per run, every effect, answer, and event in effect-loop
//! order. `seq` is the loop's order, not the clock's; `at` is the wall
//! clock at append, kept for display only. `(task_id, task_seq)` is the
//! record's `Provenance`, stored beside the payload.
//!
//! Every integer that is a `u64` in Rust (`effect_id`) is stored
//! as its two's-complement `i64` reinterpretation, since SQLite integers
//! are signed; see `append::signed` and `append::unsigned`. `seed` is the
//! run's 32-byte seed as 64 lowercase hex characters. `task_id` is the
//! Engine's hierarchical task path (`0`, `0.2`, `0.2.1`) stored as text,
//! since a path of unbounded depth has no integer form. Timestamps are UTC
//! milliseconds since the Unix epoch.

/// The DDL, idempotent so an existing file opens without change.
pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS runs (
    run_id        INTEGER PRIMARY KEY,
    agent         TEXT    NOT NULL,
    seed          TEXT    NOT NULL,
    flags         INTEGER NOT NULL,
    started_at    INTEGER NOT NULL,
    ended_at      INTEGER,
    status        TEXT    CHECK (status IN ('completed', 'failed', 'canceled')),
    final_text    TEXT,
    error_code    TEXT,
    error_message TEXT
);

CREATE TABLE IF NOT EXISTS records (
    run_id    INTEGER NOT NULL REFERENCES runs (run_id),
    seq       INTEGER NOT NULL,
    task_id   TEXT    NOT NULL,
    task_seq  INTEGER NOT NULL,
    kind      TEXT    NOT NULL CHECK (kind IN ('effect', 'answer', 'event')),
    effect_id INTEGER,
    payload   TEXT    NOT NULL,
    at        INTEGER NOT NULL,
    PRIMARY KEY (run_id, seq)
);
";

/// Opens a run: every column of `runs` that `begin_run` knows.
pub(crate) const INSERT_RUN: &str = "INSERT INTO runs \
    (agent, seed, flags, started_at) \
    VALUES (?1, ?2, ?3, ?4)";

/// Whether a run exists and whether it has ended: one row with `ended_at`.
pub(crate) const SELECT_RUN_ENDED_AT: &str = "SELECT ended_at FROM runs WHERE run_id = ?1";

/// The next `seq` for a run: one past its maximum, `0` when it has none.
pub(crate) const SELECT_NEXT_SEQ: &str =
    "SELECT COALESCE(MAX(seq), -1) + 1 FROM records WHERE run_id = ?1";

/// Appends one record.
pub(crate) const INSERT_RECORD: &str = "INSERT INTO records \
    (run_id, seq, task_id, task_seq, kind, effect_id, payload, at) \
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";

/// Closes a run. The `ended_at IS NULL` guard makes a second close a
/// no-op the caller detects through the change count.
pub(crate) const UPDATE_RUN_ENDED: &str = "UPDATE runs SET \
    ended_at = ?2, status = ?3, final_text = ?4, error_code = ?5, error_message = ?6 \
    WHERE run_id = ?1 AND ended_at IS NULL";

/// One run row, columns in [`crate::read`]'s order.
pub(crate) const SELECT_RUN: &str = "SELECT \
    agent, seed, flags, started_at, \
    ended_at, status, final_text, error_code, error_message \
    FROM runs WHERE run_id = ?1";

/// A run's records, newest first by `seq`, optionally one kind only
/// (`?2` null means every kind), at most `?3` of them (`-1` means all).
/// Newest first so `LIMIT` keeps the final records; the reader reverses.
/// Columns in [`crate::read`]'s order.
pub(crate) const SELECT_RECORDS: &str = "SELECT \
    seq, task_id, task_seq, kind, effect_id, payload, at \
    FROM records WHERE run_id = ?1 AND (?2 IS NULL OR kind = ?2) \
    ORDER BY seq DESC LIMIT ?3";
