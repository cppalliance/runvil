//! The read side: one run's row and its records, whole or sliced by kind.

use runvil::{Error, ErrorCode, RunSeed};

use crate::append::{RunLog, unsigned};
use crate::error::LogError;
use crate::record::{Record, RecordFilter, RecordKind, RunId, RunMeta, RunRow, Seq, StoredRecord};
use crate::schema;

impl RunLog {
    /// Reads `run`'s row.
    ///
    /// # Errors
    /// Returns [`LogError::UnknownRun`] when `run` was never begun here,
    /// [`LogError::Corrupt`] when the row does not fit the schema, and
    /// [`LogError::Database`] when the database cannot read it.
    pub async fn run(&self, run: RunId) -> Result<RunRow, LogError> {
        let mut rows = self.conn().query(schema::SELECT_RUN, (run.get(),)).await?;
        let Some(row) = rows.next().await? else {
            return Err(LogError::UnknownRun(run));
        };
        let meta = RunMeta {
            seed: to_seed(row.get::<String>(1)?)?,
            flags: to_u32(row.get(2)?, "flags")?,
            started_at: row.get(3)?,
        };
        let agent = row.get(0)?;
        let ended_at: Option<i64> = row.get(4)?;
        let result = match row.get::<Option<String>>(5)? {
            None => None,
            Some(status) => {
                let error_code: Option<String> = row.get(7)?;
                Some(parse_result(
                    &status,
                    row.get(6)?,
                    error_code.as_deref(),
                    row.get(8)?,
                )?)
            }
        };
        Ok(RunRow {
            id: run,
            meta,
            agent,
            ended_at,
            result,
        })
    }

    /// Reads the records of `run` that `filter` selects, in the order
    /// [`RecordFilter`] documents. A run with nothing selected reads as
    /// empty; an unknown run is refused.
    ///
    /// # Errors
    /// Returns [`LogError::UnknownRun`] when `run` was never begun here,
    /// [`LogError::Corrupt`] when a row does not fit the schema,
    /// [`LogError::Payload`] when a payload does not parse, and
    /// [`LogError::Database`] when the database cannot read.
    pub async fn records(
        &self,
        run: RunId,
        filter: RecordFilter,
    ) -> Result<Vec<StoredRecord>, LogError> {
        self.run(run).await?;
        let kind = filter.kind.map(RecordKind::as_str);
        let limit = filter.last.map_or(-1, i64::from);
        let mut rows = self
            .conn()
            .query(schema::SELECT_RECORDS, (run.get(), kind, limit))
            .await?;
        // The query reads newest first so `LIMIT` keeps the final
        // records; oldest first is the order callers expect.
        let mut records = Vec::new();
        while let Some(row) = rows.next().await? {
            let kind: String = row.get(3)?;
            let kind = RecordKind::parse(&kind).ok_or_else(|| {
                LogError::Corrupt(format!(
                    "expected kind effect, answer, or event, found {kind:?}"
                ))
            })?;
            let payload: String = row.get(5)?;
            records.push(StoredRecord {
                seq: Seq::from_raw(unsigned(row.get(0)?)),
                at: row.get(6)?,
                record: Record {
                    task_id: row.get(1)?,
                    task_seq: to_u32(row.get(2)?, "task_seq")?,
                    kind,
                    effect_id: row.get::<Option<i64>>(4)?.map(unsigned),
                    payload: serde_json::from_str(&payload)?,
                },
            });
        }
        records.reverse();
        Ok(records)
    }

    /// Every `event` record of `run` in `seq` order, the loop's order:
    /// what a session view renders.
    ///
    /// # Errors
    /// Returns [`LogError::UnknownRun`] when `run` was never begun here,
    /// [`LogError::Corrupt`] when a row does not fit the schema,
    /// [`LogError::Payload`] when a payload does not parse, and
    /// [`LogError::Database`] when the database cannot read.
    pub async fn transcript(&self, run: RunId) -> Result<Vec<StoredRecord>, LogError> {
        let filter = RecordFilter {
            kind: Some(RecordKind::Event),
            last: None,
        };
        self.records(run, filter).await
    }
}

/// A stored `u32` column, refusing anything outside the type.
fn to_u32(value: i64, column: &str) -> Result<u32, LogError> {
    u32::try_from(value)
        .map_err(|_| LogError::Corrupt(format!("expected {column} to fit u32, found {value}")))
}

/// A stored `seed` column, parsed as `RunSeed`'s serde form defines it.
fn to_seed(text: String) -> Result<RunSeed, LogError> {
    serde_json::from_value(serde_json::Value::String(text))
        .map_err(|error| LogError::Corrupt(format!("seed: {error}")))
}

/// A stored `error_code` column, parsed as [`ErrorCode::from_tag`] reads it.
fn to_code(tag: &str) -> Result<ErrorCode, LogError> {
    ErrorCode::from_tag(tag).ok_or_else(|| {
        LogError::Corrupt(format!(
            "expected error_code to be an ErrorCode tag, found {tag:?}"
        ))
    })
}

/// Rebuilds a run's result from its four result columns.
///
/// Three shapes read back: `completed` with its final text and no error
/// columns, `failed` with a code other than `canceled` and a message, and
/// `canceled` with the code `canceled` and a message. The rebuilt error
/// has its code's default prose and no location or facts, since the
/// stored message is the located message and already ends with any
/// location.
fn parse_result(
    status: &str,
    final_text: Option<String>,
    error_code: Option<&str>,
    error_message: Option<String>,
) -> Result<Result<String, Error>, LogError> {
    if !matches!(status, "completed" | "failed" | "canceled") {
        return Err(LogError::Corrupt(format!(
            "expected status completed, failed, or canceled, found {status:?}"
        )));
    }
    let code = error_code.map(to_code).transpose()?;
    match (status, final_text, code, error_message) {
        ("completed", Some(final_text), None, None) => Ok(Ok(final_text)),
        ("failed", None, Some(code), Some(message)) if code != ErrorCode::Canceled => {
            Ok(Err(Error::new(code, message, code.prose())))
        }
        ("canceled", None, Some(code @ ErrorCode::Canceled), Some(message)) => {
            Ok(Err(Error::new(code, message, code.prose())))
        }
        (status, final_text, _, error_message) => Err(LogError::Corrupt(format!(
            "expected result columns to match {status:?}, found final_text={final_text:?} \
             error_code={error_code:?} error_message={error_message:?}"
        ))),
    }
}
