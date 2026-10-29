//! Tests for the scheduler-mode section VM built through the executor's
//! real `section_vm` setup path: the yield shims (`shims`), the error
//! table and failure envelope contract (`errors`), the coroutine
//! mechanics the shims rely on (`coroutine`), the Lua loop's
//! instruction cost (`quota`), the loop's one contract case no real round
//! produces (`models_loop_contract`), and the reserved-name list against
//! the globals setup installs (`globals`). This file holds the fixtures every
//! sibling drives: the test model and tool sets, the VM builder, and the
//! start-and-parse helpers.
//!
//! These sit in `runvil-engine` (not in `runvil-lua`)
//! because the real setup path they exercise is the executor's
//! `section_vm` composition, which stays with the executor to keep the
//! dependency one-directional.

mod coroutine;
mod errors;
mod globals;
mod models_loop_contract;
mod quota;
mod shims;

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use mlua::{MultiValue, Thread};
use runvil_lua::test_support::answer_envelope;
use runvil_lua::{Error, LuaProgram};
use runvil_types::tools::{ToolId, ToolInfo};
use runvil_types::untrusted::GuardNonce;
use serde_json::json;

use crate::execute::section_vm::{SectionVmSetup, VmSeed, setup_section_vm};
use crate::lua::{Answer, Argv, Library, Request, SectionText, SectionVm, ToolSet, YieldParse};
use crate::model::ModelSet;
use crate::test_support::fresh_access;
use crate::test_support::recording::null_emitter;
use crate::test_support::scenario::{TEST_SEED, test_binding};

#[track_caller]
fn test_models() -> ModelSet {
    ModelSet::from_parts(
        vec![test_binding("fast", "a fast model", "test-model")],
        None,
    )
}

/// A minimal tool info behind a bound alias, for handle-form
/// dispatch tests; dispatch never reaches an implementation through the
/// yield boundary, so the tool is data alone.
#[track_caller]
fn stub_tool() -> ToolInfo {
    ToolInfo::new(
        ToolId::parse("tools/echo").expect("valid id"),
        "echo tool",
        json!({ "type": "object" }),
    )
}

/// One tool set whose catalog holds the stub tool, under its default wire
/// name `tools_echo`.
#[track_caller]
fn test_tools() -> ToolSet {
    ToolSet::new(
        Vec::new(),
        std::collections::BTreeMap::new(),
        runvil_types::tools::ToolCatalog::new([stub_tool()]).expect("one tool"),
    )
}

/// Builds a section VM through the real setup path: construction, Engine
/// injection, the control surface with the yield shims, `require`, and the
/// store yield shims.
#[track_caller]
fn scheduler_vm(models: &ModelSet, var: Option<&serde_json::Value>) -> SectionVm {
    scheduler_vm_with_tools(models, &ToolSet::default(), var)
}

/// [`scheduler_vm`] with an explicit tool set, so `tools.get` reads its
/// tool objects.
#[track_caller]
fn scheduler_vm_with_tools(
    models: &ModelSet,
    tools: &ToolSet,
    var: Option<&serde_json::Value>,
) -> SectionVm {
    set_up_vm(&VmBinds {
        section: "Test",
        tools,
        models,
        argv: Argv::Frozen(None),
        seed: VmSeed { var, item: None },
        ui: None,
    })
}

/// What a test's section VM binds beyond an empty run's: its section, the
/// run's tool set and model set, and the conditional Engine globals.
struct VmBinds<'a> {
    section: &'a str,
    tools: &'a ToolSet,
    models: &'a ModelSet,
    argv: Argv<'a>,
    seed: VmSeed<'a>,
    ui: Option<&'a Arc<serde_json::Value>>,
}

/// Builds the section VM `binds` describes through the real setup path,
/// with no args or preludes and a `require` whose resolver reaches no
/// library.
#[track_caller]
fn set_up_vm(binds: &VmBinds<'_>) -> SectionVm {
    let emitter = null_emitter();
    let vm = SectionVm::new_for_section(
        &GuardNonce::from_seed(&TEST_SEED),
        &Arc::new(binds.tools.clone()),
        &Arc::new(Mutex::new(binds.models.clone())),
        &emitter,
        binds.section,
    )
    .expect("the section VM builds");
    let sys = json!({});
    let access = fresh_access();
    let setup = SectionVmSetup {
        args: "",
        argv: binds.argv,
        sys: &sys,
        access: &access,
        seed: binds.seed,
        emitter: &emitter,
        section_name: binds.section,
        max_tool_iterations: 24,
        ui: binds.ui,
        preludes: &[],
        raw_shims: false,
    };
    let read = |_: String| -> std::result::Result<Option<SectionText>, crate::Error> { Ok(None) };
    let resolve = |heading: String, _| -> std::result::Result<Library, crate::Error> {
        Err(crate::Error::lua_message(format!(
            "no library `{heading}` in reach"
        )))
    };
    setup_section_vm(vm, &setup, read, resolve, |template, _| {
        Ok(template.to_owned())
    })
    .expect("the setup installs")
}

/// Starts `source` as a coroutine on the VM and runs it to its first
/// yield, returning the thread and the yielded values.
#[track_caller]
fn start(vm: &SectionVm, source: &str) -> (Thread, MultiValue) {
    let function = vm
        .lua()
        .load(source)
        .into_function()
        .expect("the driver chunk compiles");
    let thread = vm
        .lua()
        .create_thread(function)
        .expect("the driver thread creates");
    let yielded = thread
        .resume::<MultiValue>(())
        .expect("the driver yields its request");
    (thread, yielded)
}

#[track_caller]
fn yielded_request(vm: &SectionVm, source: &str) -> Request {
    let (_thread, yielded) = start(vm, source);
    parse_request(vm, yielded)
}

/// Parses one yielded request table, failing on anything malformed.
#[track_caller]
fn parse_request(vm: &SectionVm, yielded: MultiValue) -> Request {
    let value = yielded.into_iter().next().expect("one yielded value");
    match Request::from_yield(vm.lua(), &value) {
        YieldParse::Request(request) => request,
        other => panic!("the shim yield is a well-formed request, got {other:?}"),
    }
}

/// Renders the successful `answer` as the shim's envelope and resumes the
/// thread with it.
#[track_caller]
fn resume_with(vm: &SectionVm, thread: &Thread, answer: Answer<Error>) -> MultiValue {
    let (envelope, _retained) =
        answer_envelope(vm.lua(), Ok(answer)).expect("the envelope renders");
    thread
        .resume::<MultiValue>(envelope)
        .expect("the coroutine accepts the answer")
}

/// Compiles one author block the way the parser's prologue chunks are
/// compiled.
#[track_caller]
fn compile_block(source: &str) -> LuaProgram {
    LuaProgram::compile(
        source,
        "section `Test` prologue",
        NonZeroU32::MIN,
        &null_emitter(),
        "Test",
    )
    .expect("the driver block compiles")
}
