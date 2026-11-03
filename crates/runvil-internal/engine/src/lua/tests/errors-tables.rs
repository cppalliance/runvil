//! The universal error at the coroutine boundary: an Engine failure
//! answered through the envelope reaches the author's `pcall` as the table
//! its code names. The Lua crate's own tests pin the way back, a kept
//! table mapping onto the failure it stands in for.

use runvil_types::error::{ErrorCode, FactKey};
use runvil_types::ids::TaskId;

use crate::lua::{CoroStep, LuaBlockResult};
use crate::model::ModelSet;

use super::super::{compile_block, scheduler_vm};

/// What a block's `pcall` of `models.infer` sees when the Engine answers
/// the round with `error`: the table's `kind`, the fields `fields` names,
/// and its `tostring`, joined by `|`.
fn caught_engine_failure(error: crate::Error, fields: &[&str]) -> String {
    let vm = scheduler_vm(&ModelSet::default(), None);
    let names = fields
        .iter()
        .map(|name| format!("tostring(err.{name})"))
        .collect::<Vec<_>>()
        .join(" .. '|' .. ");
    let program = compile_block(&format!(
        "local ok, err = pcall(models.infer, \"hi\")\n\
         assert(not ok, \"the failure envelope must raise\")\n\
         return err.kind .. '|' .. {names} .. '|' .. tostring(err)"
    ));
    let CoroStep::Yielded(thread, _values) =
        vm.start_block_coro(&program).expect("the block suspends")
    else {
        panic!("the shim yield must suspend the block");
    };
    match vm
        .resume_block_coro_answer(&program, &thread, Err(error))
        .expect("the pcall'd block resumes")
    {
        CoroStep::Done(LuaBlockResult::Returned(Some(text))) => text,
        other => panic!("expected the caught failure's rendering, got {other:?}"),
    }
}

/// The task id `path` names.
fn task(path: &str) -> TaskId {
    path.parse().expect("the test task id parses")
}

#[test]
fn an_engine_failure_answered_through_the_envelope_is_the_table_its_code_names() {
    assert_eq!(
        caught_engine_failure(crate::error::task_cancelled(&task("0.1")), &["task"]),
        "cancelled|0.1|task `0.1` was cancelled"
    );
    assert_eq!(
        caught_engine_failure(
            crate::error::tasks_live(&[task("0.1"), task("0.2")]),
            &["tasks"]
        ),
        "tasks_live|0.1, 0.2|chain ended with tasks still live: 0.1, 0.2; wait on or cancel \
         every task a chain spawns before it ends"
    );
    assert_eq!(
        caught_engine_failure(crate::error::context_exhausted("precheck"), &["reason"]),
        format!(
            "context_exhausted|precheck|context exhausted: {}",
            runvil_lua::exhaustion_detail("precheck")
        )
    );
    let anchor = runvil_vfs::VfsError::Anchor {
        path: "notes.md".to_owned(),
        anchor: "TODO".to_owned(),
        count: 2,
    };
    let op = crate::lua::VfsOp::StrReplace {
        path: "notes.md".to_owned(),
        old: "TODO".to_owned(),
        new: "DONE".to_owned(),
    };
    assert_eq!(
        caught_engine_failure(
            crate::error::store_op(&op, &anchor),
            &["reason", "path", "anchor", "count", "rule"]
        ),
        format!(
            "store|anchor|notes.md|TODO|2|nil|{}",
            crate::lua::store_error_message(&op, &anchor)
        )
    );
    let mut empty = crate::model::completion_error(
        ErrorCode::EmptyReply,
        "the model replied with no text and no tool calls",
    )
    .with_cause(&std::io::Error::other("a detail the table leaves out"));
    empty
        .facts
        .insert(FactKey::FinishReason, serde_json::Value::from("length"));
    assert_eq!(
        caught_engine_failure(empty, &["finish_reason"]),
        "empty_model_reply|length|the model replied with no text and no tool calls"
    );
}
