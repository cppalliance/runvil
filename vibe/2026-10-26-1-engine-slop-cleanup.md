---
name: runvil-engine slop cleanup
overview: "Cleanup Plan from the vibelight scan of runvil-engine: 84 verified findings, 2 reinstated ones, and 19 from the outline review (R-1 to R-19, verified at their sites before editing), in 28 work items. All 5 open questions are answered: public API changes, quoted error messages, rulebook vocabulary, and the split of the Lua `internal` error kind land now, with downstream breakage accepted by the owner, and the copied gateway wording stays a doc note."
todos:
  - id: w1
    content: "W1: error docs and vestiges (S1-1, S1-2, S1-4, S1-16, S2-18 type doc, S5-3 variant doc, S5-6)"
    status: pending
  - id: w2
    content: "W2: run outcome docs (S1-5, S1-15 docs, S5-2 docs, S5-3, V1-1, V1-2)"
    status: pending
  - id: w3
    content: "W3: config and limits (S1-7, S1-8, S1-9 internal part, S5-1, S1-10)"
    status: pending
  - id: w4
    content: "W4: roster (S1-6)"
    status: pending
  - id: w5
    content: "W5: run state (S1-11, S1-12, V3-1, S1-13, V5-1)"
    status: pending
  - id: w6
    content: "W6: shim modules (S1-17, S4-1, S4-2, S4-4, S4-5, S5-6 module part)"
    status: pending
  - id: w7
    content: "W7: keyword spellings (S1-14)"
    status: pending
  - id: w8
    content: "W8: run and effect docs (S2-1, S2-6)"
    status: pending
  - id: w9
    content: "W9: scheduler docs (S2-9, S2-10, S2-11, S2-16, S2-17, S2-18, S3-2, S3-3, S3-10, S3-25)"
    status: pending
  - id: w10
    content: "W10: effect table (S2-13)"
    status: pending
  - id: w11
    content: "W11: task arena (S2-12, S3-4, S3-16, S3-5, S3-6, S3-13 code)"
    status: pending
  - id: w12
    content: "W12: chain record (S3-7, S3-19, S3-14, S3-22)"
    status: pending
  - id: w13
    content: "W13: small scheduler changes (S2-14, S2-15, S3-9, S3-15, S3-20)"
    status: pending
  - id: w14
    content: "W14: frame (S3-17, S3-18, S3-23)"
    status: pending
  - id: w15
    content: "W15: section VM and model round docs (S3-1, S3-12, V3-4, S3-11, V3-3, S3-27)"
    status: pending
  - id: w16
    content: "W16: `subst` and `heading_address` (S4-6, S4-7 docs, S4-8 doc, S4-9, S4-10, S4-11)"
    status: pending
  - id: w17
    content: "W17: test support (S4-12, V4-1, S4-14 doc, S4-15, V3-2)"
    status: pending
  - id: w18
    content: "W18: READMEs (S5-5, S5-7)"
    status: pending
  - id: w19
    content: "W19: guide (V2-1, S3-13 guide part, V5-1 guide part)"
    status: pending
  - id: w20
    content: "W20: facade API changes (S1-15, S1-18, S2-8, S5-2, R-10 to R-15; S1-9's public part as R-11)"
    status: pending
  - id: w21
    content: "W21: author-facing messages (S4-7, S4-8, S1-3)"
    status: pending
  - id: w22
    content: "W22: rulebook vocabulary (S5-4, S5-5 AGENTS part)"
    status: pending
  - id: w23
    content: "W23: run state (R-1, R-2, R-5, R-19)"
    status: pending
  - id: w24
    content: "W24: scheduler names and docs (S2-3, S2-19, R-6, R-17)"
    status: pending
  - id: w25
    content: "W25: chain lifecycle (R-18)"
    status: pending
  - id: w26
    content: "W26: position types (R-7)"
    status: pending
  - id: w27
    content: "W27: error types (R-3, R-4, R-8, R-9)"
    status: pending
  - id: w28
    content: "W28: Lua error kinds (R-16)"
    status: pending
isProject: false
---

# runvil-engine slop cleanup

<product-contract>

## Product Requirements

- Scope: repository runvil at `9f6f8080e` (clean tree), guide runvil-docs at `22ff9e0`. Crate analyzed: runvil-engine (`crates/runvil-internal/engine`). Context crates, outlined but not analyzed: runvil-lua, runvil-vfs, runvil-types, runvil-parser, runvil-model-client, build-ceiling. Excluded crates: none. Doc roots: `README.md`, `AGENTS.md`, `crates/runvil-internal/engine/README.md` and `AGENTS.md`, `vibe/`, and runvil-docs (`README.md`, `src/`). Analysis limits: vibelight reported 32 items written more than one way (its Spellings section); they were not analyzed.
- Cleanup goals: make every runvil-engine doc comment, README line, and guide passage that verification confirmed wrong say what the code does; remove dead, duplicated, and doubly stored state in the scheduler, the run state, and test support; delete compatibility shims between crates that version together. Public interface, documented message, and rulebook vocabulary changes are in scope, and the owner accepts the downstream breakage they cause. No runtime cost and no behavior change except where a work item says so.
- Non-goals: re-spelling the 32 Spellings items; sibling slop in runvil-lua, runvil-model-client, runvil-parser, and Workshop (see Deferred).
- Success criteria: every work item's check passes; the verification commands in the Testing Plan pass; a fresh vibelight run shows each declaration and comment change listed here and nothing else.

## Functional Specification

### Debt Inventory

Paths are relative to `crates/runvil-internal/engine/src/` unless they start with `crates/`, name a repository-root file, or start with `runvil-docs/`. Reversal cost is easy unless marked hard. R items come from the outline review: they cite declaration lines from the outline at `9f6f8080e` and are not yet source-verified (see the Decision Record).

- Code slop, runvil-engine:
  - Vestiges:
    - S1-1: `#[non_exhaustive]` on the crate-private `Error` and 13 of its variants does nothing, and the type doc claims it protects future variants (`error.rs:66-73`). Target: attributes and sentence gone.
    - S1-16: `From<RunError> for Error` (`execute/error.rs:318-322`) has no user. Target: deleted.
    - S1-17 (merges S4-3): `execute/protocol.rs` re-exports 15 runvil-lua names "so existing `crate::execute::protocol::*` paths keep working", and its list of suspending calls names 5 of the 16 requests. Target: the names come through `crate::lua`; the file is deleted.
    - S1-15: `RunErrorKind::Cancelled` and `RunError::is_cancelled` never reach a caller (`execute/error.rs:164, 179-183`). Target: both deleted (W20).
    - S2-8: `ChatAnswerRecord::new` and its three `with_*` setters are public (`execute/run/effect-record.rs:196-228`) but only in-crate tests call them. Target: `#[cfg(test)] pub(crate)` (W20).
    - S2-14: `Scheduler::max_chains` (`execute/scheduler.rs:222-226`) is a production field that only one test varies. Target: a constant plus a `#[cfg(test)]` field.
    - V4-1: the scripted reply delay (`test_support/scripted-chat.rs:93-97, 138-149, 229-233, 338-354`) has no consumer; both suites that script it hold the round until cancel. Target: deleted. It makes S4-13 moot.
    - R-2: `OwnedInputs` (`execute/context.rs:35-41`) exists only so `RunState::new` and `prepare_state` can borrow `&RunContext` while owning its `preludes`, `roster`, and `ui`, though `Run::new` already owns the context. Target: both take the context by value; `OwnedInputs` goes.
  - State:
    - S1-6: `RunContext::packs` lets `roster` and `vendors` disagree for a name in both lists (`execute/config.rs:282-301`); latent, since the Harness never lists a name twice. Target: a vendor is kept only for a usable roster entry.
    - S1-11: `RunState` holds the model set twice (`execute/context.rs:105-112, 162, 176`). Target: one field.
    - S1-18: `Requirements::reserved` is a public `Vec` kept sorted and unique by hand (`execute/requirements.rs:42-49, 81-83`). Target: a `BTreeSet<PackId>` (W20).
    - S2-12: the task arena is a `HashMap` that every ordered walk re-sorts (`execute/scheduler.rs:204`, `execute/scheduler/waits.rs:76-89`, `execute/scheduler/task_end.rs:140-148`). Target: `BTreeMap`.
    - S2-13: `pending` and `orphaned` are two maps for one effect's state, and `abort_effect` orphans an id even when it was not pending (`execute/scheduler/chain.rs:303-313`), which would leave the run waiting forever. Target: one `outstanding` map.
    - S3-4: a task's owner is stored on `TaskSlot` and on `ChainKind::Task`, kept equal only by `reassign_tasks` (`execute/scheduler/task_end.rs:221-242`). Target: the owner on the slot only.
    - S3-5: `TaskState::Done { ok }` restates `TaskSlot::outcome`, with an unreachable internal error in `deliver` (`execute/scheduler/waits.rs:149-153`). Target: the outcome held in the state.
    - S3-6: `TaskSlot::exec` is `None` exactly for timer slots (`execute/scheduler/timer.rs:53-60`, `execute/scheduler/tasks.rs:250-257`). Target: `exec` inside `TaskBacking::Chain`.
    - S3-7: every chain starts with `access: None`, against its field doc (`execute/scheduler/chain.rs:115`, `execute/scheduler/chain_record.rs:212-216`). Target: the access installed at birth through `ChainStart`.
    - S3-9: `VfsContinuation::observations` is a pure function of the `op` beside it (`execute/scheduler/pending.rs:56-66`). Target: `Continuation::Vfs(VfsOp)`.
    - S3-13: `Chain::note` is written on chains whose notes nothing reads (`execute/scheduler/waits.rs:261-273`). Target: the note on `TaskSlot`.
    - S3-14: `Chain::seed: Option<TaskSeed>` encodes "no seed" two ways (`execute/scheduler/chain_record.rs:183-185`). Target: a plain `TaskSeed`.
    - S3-22: what a chain is parked on spans `blocked`, `waiting_on`, and `batch` (`execute/scheduler/chain_record.rs:186-200`). Target: one `parked` value.
    - S3-23: the counts `Option` is set before the install's model step, then unwrapped again (`execute/section_context.rs:230-233, 300-310`). Target: set only after the install succeeds, and returned directly.
    - S4-11: `Visible::children` is derived from `home` and `caller`, stored beside it (`heading_address.rs:34-38`). Target: computed in `iter`.
    - S4-12: `ScriptedChat` counts rounds in an `AtomicUsize` and in a `Vec` (`test_support/scripted-chat.rs:271-276, 340-348`). Target: the `Vec` only.
    - R-10: `RunContext::model` and its field (`execute/config.rs:116`) only feed `prepare`. Target: a `model` parameter of `prepare` (W20).
    - R-15: the public `Step::Pending::effects` is a `Vec<(EffectId, Provenance, Effect)>` (`execute/run.rs:39`), mirrored by `Scheduler::issued`. Target: a named `IssuedEffect` (W20).
    - R-18: `Chain`'s `frame`, `block`, `pending_prose`, `coroutine`, and `incoming` (`execute/scheduler/chain_record.rs:161`) are loose fields whose docs tie them to one section entry and one running block, so `frame()` and `frame_mut()` keep an `Error::Internal` path for a state the walk rules out. Target: one `entered` value.
  - Duplication and reuse:
    - S3-15 (merges S4-16): `ChatDispatch` and `ToolCallDispatch` are one enum (`execute/scheduler/chat.rs:60-67`, `execute/scheduler/tool_call.rs:69-77`). Target: one `Dispatched`.
    - S3-16: `Scheduler::enclosing` wraps `ChainKind::enclosing` and documents it differently (`execute/scheduler/admission.rs:27-31`). Target: one implementation, following S3-4.
    - S1-10: the `nz!` macro reimplements `const { NonZero::new(N).unwrap() }` (`execute/config-limits.rs:6-23`). Target: macro deleted, subject to the clippy gate.
    - S1-14 (keyword half): `keyword_name` hand-copies runvil-parser's `ModelKeyword` spellings and falls back to `"unknown"` (`execute/context-bound.rs:37-52`). Target: one total function in `runvil_parser::detail`.
    - S4-6: `subst::render_scalar` and `render`'s fallback repeat runvil-lua's `render_item` (`subst.rs:126-144, 385-406`). Target: one rendering rule.
    - R-1: `ReportingHandles { emitter, turns }` (`execute/section_context.rs:112`) exists, yet `RunState` (`execute/context.rs:62`), `SectionContext` (`execute/section_context.rs:75`), and `ArrivedRound` (`execute/scheduler/chat.rs:358`) each hold the same pair as loose fields. Target: each holds one `ReportingHandles`.
    - R-7: `SlicePath` and `SectionIndex` are half adopted: `heading_address` (`heading_address.rs:34, 84, 93`), `RequireOrigin::Section` (`execute/walk_target.rs:148`), `library_key`, `resolve_jump_target`, `JumpTarget` (`execute/walk_target.rs:103`), and `SectionContext::new` take raw `usize` and `&[usize]`, and `ChainTarget` (`execute/scheduler/walk.rs:33`) restates a `JumpTarget` as `child: bool` plus `index`. Target: the newtypes everywhere, in one module, and `ChainTarget` holding its `JumpTarget`.
    - R-8: `Error::Lua(String)` and `Error::LuaRuntime { message, cause }` (`error.rs:73`) differ only in whether a cause exists, which `Error::Internal` expresses with an optional field, and three constructors build `Internal` (`internal`, `internal_owned`, `internal_with_cause`; `error/convert.rs:17`). Target: one `Lua` variant with an optional cause, and two constructors.
    - R-9: `Error::ParseFrontmatter` and `Error::ParseStructured` copy runvil-parser's `ParseError` variant for variant (`error.rs:73`), while `Error::Completion` already holds its foreign error whole. Target: `Error::Parse(ParseError)`.
    - R-19: `RunState` pairs shared inputs with a borrowing getter and an `Arc` getter: `prompt`/`prompt_arc`, `argv`/`argv_shared`, and `args_shared` (`execute/context.rs:131`). Target: one getter each, returning the shared handle.
  - Naming:
    - S1-9: `RunLimits` getters disagree with their setters, and the public `timeout()` reads as a whole-request deadline (`execute/config-limits.rs:36-43, 121-154`). Target: internal getters gone (W3); the public part is R-11, because getters named `request_timeout()` and `max_response_bytes()` would collide with the setters of those names.
    - S1-13: `ctx` names both `RunContext` and `RunState`. Target: a `RunContext` parameter is named `config`.
    - S2-15: `issue_infer` is the one fallible half not named `prepare_*` (`execute/scheduler/dispatch.rs:225`). Target: `prepare_infer`.
    - S3-3: "the same chain" means a shared lineage, while the H1 pass and the walk are two records. Target: the docs say lineage; `admit_chain`'s binding is named `owner`.
    - S3-17: `script_call_counts` serves model calls too (`execute/section_context.rs:217`). Target: `tool_call_counts`.
    - S3-18: "scope install" names the counts-and-model install, while "scope" elsewhere means the frozen sets (`execute/section_context.rs:282-312`). Target: `install_first_call_state`, and "first-call install" in prose.
    - S3-19: `install_root_slots` installs one access capability (`execute/scheduler/walk.rs:85-107`). Target: deleted by S3-7.
    - S2-3 (reinstated): `run::Phase` (`execute/run.rs:92`) and `scheduler::Phase` (`execute/scheduler.rs:164`) each sit in a field named `phase` documented as the run's phase, one nested in the other's `Live` variant. Target: `run::Start` (field `start`), and a scheduler field doc that says it tracks the step loop.
    - R-4: `MergeRule`, `MergeTool`, `Error::ToolMerge`, and the `error::merge` module (`error/merge.rs:30, 55`) read like the guide's same-role merging of messages; they hold the frozen sets' rules. Target: `SetRule`, `SetTool`, `Error::ToolSets`, and `error::sets`.
    - R-6: "slot" names a task's arena record (`TaskSlot`, `owned_slot`, `visible_slot`; `execute/scheduler/tasks.rs:84`), an admission slot, and a batch slot. Target: `TaskEntry`, `owned_entry`, and `visible_entry`, with "slot" kept for admission and batches.
    - R-11 (replaces S1-9's public renames): `RunLimits` (`execute/config-limits.rs:36, 45`) and `RunContext` (`execute/config.rs:116`) give their getters other names than their consuming setters (`timeout`/`request_timeout`, `response_bytes`/`max_response_bytes`, `catalog`/`tools`, `run_flags`/`flags`, `vfs_handle`/`vfs`), and S1-9's planned getters would reuse the setters' names in the same impl, which does not compile. `request_timeout` is also a per-chunk wait that the guide calls the receive timeout (`runvil-docs/src/language/16-limits-and-errors.md:42, 1068-1072`). Target: `RunLimits` as public fields with `receive_timeout`, and `RunContext` with `with_*` setters and field-named getters (W20).
    - R-12: `RunResult::Ok` (`execute.rs:146`) clashes with `Result::Ok`, as its own doc warns. Target: `RunResult::Completed` (W20).
    - R-13: `RunError::cause` (`execute/error.rs:142`) and `Error::cause` (`error.rs:434`) shadow the deprecated `std::error::Error::cause`. Target: `originating_error` (W20).
    - R-14: `Requirements::unmet_requirements` stutters, and `missing_required` and `unavailable` don't say they list Packs (`execute/requirements.rs:16`). Target: `unmet_models`, `missing_packs`, and `unavailable_packs` (W20).
    - R-16: the Lua error kind `internal` names transport and backend failures, the missing-model error, and Engine faults alike (`error/value.rs`; `runvil-docs/src/language/16-limits-and-errors.md:959, 1047`). Target: `model_failure` for model failures and `model_required` for the missing-model error, with `internal` kept for Engine faults (W28, Open Question 5).
    - R-17: "live H1" (`new_live_h1`, `start_live_h1`, `end_live_h1`, `end_live_h1_at_jump`, and about ten doc mentions) appears nowhere in the guide, which says "the H1 pass". Target: "live" dropped.
  - Placement:
    - S3-20: `unbound_tool_call` and `classify_vfs_failure` in `execute/scheduler/dispatch.rs` each serve one other file. Target: private functions in their users.
    - R-3: `FrozenSet` (`error/merge.rs:12-19`) lives in the error module, but `execute::scope` builds and uses the sets. Target: it moves to `execute/scope.rs`.
- Doc comment edits, runvil-engine (new texts are in Execution Instructions):
  - `error`, `error::convert`: S1-2, S1-4, S2-18 (`DepthVia`), S5-3 (`Error::LuaQuota`), S5-6 (`Error::ModelRequired`).
  - `execute`, `execute::error`: S1-5, S1-15 (docs), S5-2 (docs), S5-3, V1-1, V1-2.
  - `execute::config`, `execute::config::limits`: S1-7, S1-8, S5-1.
  - `execute::context`: S1-12, V3-1, V5-1, R-5.
  - `execute::run`, `execute::run::effect`: S2-1, S2-6, and V1-1's sibling on `EffectAnswer::Dropped`.
  - `execute::scheduler` and its children: S2-9, S2-10, S2-11, S2-16, S2-17, S2-18, S2-19, S3-2, S3-3, S3-10, S3-11, S3-25, V5-1.
  - `execute::section_vm`, `execute::section_context`, `execute::walk_target`, `execute::turn`, `execute::infer`: S3-1, S3-12, S3-27, V3-3, V3-4.
  - `subst`: S4-7 (docs), S4-8 (doc), S4-9. `heading_address`: S4-10. `lua`, `parser`, `model`: S4-1, S4-4, S4-5, S5-6. `test_support`: S4-14 (doc), S4-15. Test comments: V3-2.
- Documentation drift:
  - S5-1: the `RunLimits` docs say the round cap is per section; it counts per `models.loop` or handle `loop` call, as the guide says. The docs change.
  - S5-2: `RunErrorKind::Substitution` is documented as an outcome that no run reports; an uncaught substitution failure reports `Lua`. W20 deletes the kind, maps `Error::Substitution` to `Lua`, and updates the guide passages that name the removed kinds.
  - S5-3: the Quota docs list an instruction quota that cannot trip, and the `RunResult` copy omits the depth cap. The docs change.
  - S5-4: `AGENTS.md:14` and four engine docs say the sets freeze at the first model call; they freeze at the first loop round, and `models.infer` never freezes them. The rulebook, the docs, and the test strings change (W22).
  - S5-5: `crates/runvil-internal/engine/README.md:3` and `lib.rs:5-6` say every tool call and file access is an effect; adhoc calls, discovery listings and refusals, prelude install-time store calls, and the start probe are not. The docs change, and `AGENTS.md:9` changes in W22.
  - S5-6: `Error::ModelRequired` and `model.rs` keep the retired rule that prose drives a model turn. The docs change.
  - S5-7: the engine README's determinism claim leaves out the seed, the start time, and the other run inputs. The README changes.
  - V1-1: `RunResult::Cancelled` says the caller cancelled the run, but an uncaught dropped answer ends an uncancelled run the same way. The docs change.
  - V2-1: runvil-docs states the child-counter rule without the timed wait's timer (`runvil-docs/src/language/15-tasks.md:1769`, `05-lua-environment.md:2060`, `08-jump-and-call.md:1730`). The guide changes.
  - V5-1: four engine comments and `runvil-docs/src/language/15-tasks.md:1313` say a task's turns count against its own cap; no cap reads the turn counter. The comments and the guide change.
  - S3-13 (guide part): `runvil-docs/src/language/15-tasks.md:1365` names only the main walk. The guide changes with the code.
  - R-5: `RunState`'s doc calls it "Immutable for the run's lifetime" (`execute/context.rs`, above `:62`), though its cancel flag, event buffer, turn counter, and model set change behind shared handles. The doc changes.
  - S2-19 (reinstated, extended): six `dispatch_*` docs restate one failure rule in drifting words, and the `# Errors` clauses that end "which only a scheduler bug produces" or "which only a shim bug produces" restate one meaning of `Error::Internal`. The scheduler module doc states each rule once (W24).
- Report defects in the vibelight output: none observed.
- Extinguished and rejected: 87 candidates raised and 9 added by verification. 8 extinguished (false: S2-5; intentional: S2-4, S3-8; weak: S2-2, S2-7, S3-21, S3-24, S3-26), 2 reinstated by the outline review (S2-3, S2-19), 2 merged (S4-3 into S1-17, S4-16 into S3-15), and two partial rejections (S1-14's `thinking_name` half, whose arms cannot run and whose spelling is documented; S2-18's `DepthVia::Spawn` option, which would change documented vocabulary). The outline review added R-1 to R-19.

</product-contract>
<implementation-contract>

## Technical Design

- Scheduler types, all scheduler-private:
  - `outstanding: HashMap<EffectId, Outstanding>` with `enum Outstanding { Parked(Pending), Orphaned }` replaces `pending` and `orphaned` (S2-13).
  - `tasks: BTreeMap<TaskId, TaskSlot>` (S2-12).
  - `TaskSlot` holds `backing`, `owner`, the state, `target`, and `note: Option<String>` (S3-13). `TaskBacking::Chain { chain: ChainIndex, exec: ExecId }` and `TaskBacking::Effect(EffectId)` replace `TaskSlot::exec` (S3-6). The owner lives only on the slot (S3-4). The slot's own state holds the outcome as `Done(Result<String>)`, which drops `TaskSlot::outcome`; today's `Copy` `TaskState` stays as a derived view for `task_phase`, `ok()`, and the test hooks (S3-5).
  - `ChainKind::Task { admission }` loses `owner`; `Scheduler::enclosing` reads a task chain's slot, and `ChainKind::enclosing` and `ChainKind::owner` go (S3-4, S3-16).
  - `ChainStart` gains `access: Arc<Access>`; `Chain::access` stays an `Option` only so the chain-end paths can take it (S3-7). `install_root_slots` goes (S3-19).
  - `Chain::seed: TaskSeed` (S3-14). `Chain::parked: Option<Parked>` replaces `blocked`, `waiting_on`, and `batch`, with a `blocked()` projection for `tasks.status` (S3-22).
  - `Continuation::Vfs(VfsOp)`; `VfsContinuation` goes (S3-9).
  - `enum Dispatched { Issued, Answered(Answer<Error>) }` in `execute/scheduler/dispatch.rs` replaces `ChatDispatch` and `ToolCallDispatch` (S3-15).
  - `const MAX_CHAINS: usize = u32::MAX as usize;` plus a `#[cfg(test)] max_chains` field (S2-14).
- Run state and config: `RunState` drops `models`, and `models()` returns `&*self.model_set` (S1-11). `RunLimits` fields become `pub(crate)`, the field `concurrency` becomes `max_concurrency`, and the four `pub(crate)` getters go (S1-9). `RunContext::packs` keeps a vendor only for a name the roster maps to `None` (S1-6).
- Modules: `execute/protocol.rs`, `tools.rs`, and `untrusted.rs` go. The 15 protocol names join `lua.rs`'s `pub(crate) use runvil_lua::{...}` list; the tool and nonce names are imported from `runvil_types` where they are used (S1-17, S4-2).
- runvil-parser: `pub fn keyword_name(keyword: ModelKeyword) -> &'static str` in `crates/runvil-internal/parser/src/detail.rs`, a module the facade never re-exports (S1-14).
- Renames: `issue_infer` to `prepare_infer` (S2-15), `script_call_counts` to `tool_call_counts` (S3-17), `install_section_scope` to `install_first_call_state` (S3-18), and every `RunContext` parameter named `ctx` to `config` (S1-13, which changes one line of `crates/runvil/public-api.txt`).
- Test support: `ScriptedChat` loses `calls` and the delay mechanism: `delayed`, `ScriptedReply::Delayed`, `delay`, `timed_out`, `complete_now`'s `timeout` parameter, and `Answers::timeout` (S4-12, V4-1).
- Facade (W20): `RunLimits` is `#[non_exhaustive]` with six `pub` fields, `request_timeout` renamed `receive_timeout`, and no setters, getters, or `new`; `RunContext`'s setters are `with_*`, its getters `tools`, `flags`, and `vfs`, and `prepare` takes the current model as a parameter (R-10, R-11, in place of S1-9's getter renames); `RunResult::Ok` is `RunResult::Completed` (R-12); `cause()` on `RunError` and `Error` is `originating_error()` (R-13); `Requirements` names `unmet_models`, `missing_packs`, and `unavailable_packs` (R-14); `Step::Pending::effects` holds `IssuedEffect` values (R-15); `RunErrorKind` loses `Cancelled` and `Substitution`, `kind` classifies `Error::Interrupted` as `Internal` and `Error::Substitution` as `Lua`, and `RunError::is_cancelled` goes (S1-15, S5-2); `Requirements::reserved` is a `BTreeSet<PackId>` (S1-18); the `ChatAnswerRecord` builder is `#[cfg(test)] pub(crate)` (S2-8).
- Docs: per work item. runvil-docs takes three edits (V2-1, S3-13, V5-1), plus W20's passages on the removed kinds and its two quick-reference rows (R-11), W21's guide quotes, and W28's kind tables.
- Outline review (R items):
  - `ReportingHandles` moves to `execute::context`, and `RunState`, `SectionContext`, and `ArrivedRound` each hold one in place of `emitter` and `turns` (R-1). `RunState::new` takes the `RunContext` by value, and `OwnedInputs` goes (R-2). `RunState::prompt()` returns `&Arc<Prompt>`, `argv()` returns `Option<&Arc<serde_json::Value>>`, and `args()` returns `&Arc<str>`; `prompt_arc`, `argv_shared`, and `args_shared` go (R-19).
  - `Chain::entered: Option<Entered>`, where `Entered` holds the frame, the block index, the pending prose, and `running: Option<Running>`, and `Running` holds the coroutine and the delivered answer (R-18).
  - `SlicePath` and `SectionIndex` live together in `heading_address`, below `execute`, and every slice path and slice position uses them; `ChainTarget { jump: JumpTarget, slice: SlicePath }` (R-7).
  - `TaskSlot` is `TaskEntry` (R-6), `run::Phase` is `run::Start` (S2-3), and the H1 functions drop "live" (R-17).
  - `FrozenSet` lives in `execute::scope`; `error::sets` holds `SetRule` and `SetTool`, and `Error::ToolMerge` is `Error::ToolSets` (R-3, R-4). `Error::Lua { message, cause: Option<BoxedSource> }` replaces `Lua` and `LuaRuntime`, and `internal_owned` folds into `internal` and `internal_with_cause` (R-8). `Error::Parse(ParseError)` replaces `ParseFrontmatter` and `ParseStructured` (R-9).

</implementation-contract>
<verification-contract>

## Testing Plan

- After each work item, from the runvil root, per `AGENTS.md`:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`, with `CARGO_BUILD_WARNINGS=deny`
  - `cargo fmt --all --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, and `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`
  - `cargo test -p build-xtask` and `node --test crates/workshop/ui/test/docs-claims.mjs`
  - After any item that changes `crates/runvil/public-api.txt` (S1-13 in W5, and W20): `cargo +<pinned nightly> xtask api --check`, with `CARGO_BUILD_WARNINGS` unset, after regenerating the listing.
- Regression tests that fail before the change:
  - S1-6: call `RunContext::packs` with one name in both the usable and the unavailable list, build the run state, and assert the tool set's `vendor_for(name)` is `None`. Today `vendors` keeps the vendor, so the assertion fails.
  - S2-13: through `execute/scheduler/test_hooks.rs`, abort an effect id that is not outstanding and assert that nothing is recorded as orphaned. Today `abort_effect` inserts unconditionally, so the assertion fails.
  - S3-23: a section whose first tool call fails in the model step, followed by a second tool call; assert the second call runs the model step again. Today the counts slot is already set, so the second call skips it. Build it on the failure seams in `execute/tests/scheduler/failures-script-tools.rs`; if no seam can make `resolve_model_binding` fail, record that in the commit and rely on the new types.
  - R-16: a section that catches a failed model round asserts `err.kind == "model_failure"`. Today the kind is `internal`, so the assertion fails.
- Behavior already pinned, which must stay green: `execute/tests/tasks-chain-end.rs:37-61` and `execute/tests/waits.rs:335` (S2-12); the 14 `TaskState` assertions listed under W11 (S3-5); `execute/tests/happens_before.rs` (S3-6); `execute/tests/observations.rs` (S3-9); `execute/tests/scheduler/concurrency.rs:14-31` (S3-10); `execute/tests/waits.rs:124-146` and `execute/tests/timeouts.rs:32, 185` (S3-22); the rendering tests in `subst-tests.rs` (S4-6); the `call_count()` assertions (S4-12); `execute/run/tests-drops.rs:200-216, 294-320` (V1-1); `error/tests.rs:244-264, 290` (S1-5, S1-2); and `error/tests.rs:60-76` until W21 updates it for S1-3's new message.
- Behavior the outline-review items must keep: `execute/context-tests.rs` (R-1, R-2, R-19); `execute/tests/waits.rs`, `execute/tests/timeouts.rs`, and the walk tests under `execute/tests/scheduler/walk/` (R-18); `heading_address-tests.rs` and the jump tests (R-7); `error/tests.rs`, `error/tests-chain.rs`, and every `RunError::location` assertion, with each message and location unchanged (R-8, R-9); `execute/config-limits.rs:169-204` and the `harness-gateway-client` tests (R-11).
- Optional pin for S5-1: a section under `max_tool_iterations: 1` with two loop calls that each run one tool round and then reply.
- Refreshed outline: before the first item and after the last, run from the runvil root `vibelight crates/runvil-internal/engine --follow "**" --profile analysis --facts --order modules --kill-list counts --md-source-links on --members impl --md-out <temp>/outlines --html-out <temp>/html`, and diff the two `runvil_engine.md` files, ignoring `Source:` lines. Each declaration and doc change in this plan must appear, and nothing else. No outline is committed.

</verification-contract>
<decision-record>

## Decision Record

- Reversible decisions and consequences:
  - S1-17 routes the protocol names through `crate::lua` (S4-3's direction), so `lua.rs` stays the executor's one runvil-lua import list. The error bridge keeps naming `runvil_lua`, because it implements that crate's conversion and `ErrorValue` trait, so `lua.rs` calls itself "the executor's import surface" (S4-1).
  - S3-4 keeps the owner on `TaskSlot`, where 10 of the 14 reads already look, rather than on `ChainKind`, which costs one map lookup per admission ancestor step (depth is capped at 8). S3-16 therefore keeps the scheduler method.
  - S3-5 keeps a `Copy` view of the task state, so the 14 test assertions keep their shape.
  - S3-13 puts the note on `TaskSlot`, which `task_status` already reads and which outlives the chain.
  - S3-22 keeps today's `tasks.status` output: a requeued chain reports `Queued`, and a chain woken inline reports `Tasks` until it resumes.
  - S1-6 takes the local fix in `packs`. The one-map shape that mirrors the Harness's `PackState` needs a runvil-lua interface change and is deferred.
  - V4-1 deletes the delay mechanism rather than pinning it; a suite that needs a timed-out round scripts `ScriptedReply::Failure` with `CompletionErrorKind::Timeout`. S4-13's doc rewrite goes with it.
  - S1-3, S4-7, and S4-8 change their messages in W21, together with the guide quotes.
  - S1-10 is conditional: if the clippy gate flags `const { NonZero::new(N).unwrap() }` under `unwrap_used`, the macro stays, and its doc says why.
  - Shared wording: "the item seeded into this section" for seeded items (S3-1, S4-7), "per `models.loop` or model handle `loop` call" for the round cap (S5-1), and "frame constructor" in place of "driver" (S3-1, S1-12, V3-1).
  - The R items come from the outline review, which read the outlines and the guide but no Rust source. Each work item that holds one verifies it at its site first; where the code contradicts a finding, the item narrows or drops it and the commit says why.
  - R-11 replaces S1-9's public renames, because getters named `request_timeout()` and `max_response_bytes()` would sit beside the setters of the same names in `RunLimits`'s one impl (`execute/config-limits.rs:45`). `RunLimits` is a plain `Copy` value with no invariant beyond its `NonZero` types, so public fields replace all twelve accessors and stay non-breaking to extend under `#[non_exhaustive]`, and `receive_timeout` takes the guide's name. `RunContext` keeps a builder, because `prepare` and `packs` fill private state, so its setters take `with_*`. Rejected: `with_*` setters on `RunLimits` too, which keeps twelve methods on a plain value.
  - R-14 keeps the type name `Requirements`, because the guide calls it the requirements report (`runvil-docs/src/language/04-how-a-prompt-runs.md`), and renames only the three fields.
  - R-15 lands in W20 rather than staying optional: Open Question 1's answer puts R-15 in W20 with the other public changes, and leaving a public type's shape to coding time would let the facade surface depend on who codes it. `Step::Pending::effects` holds `IssuedEffect` values. Reverting to the tuple is easy before release and breaking after.
  - R-4 keeps `merge_tool_sets` in `execute::scope`, which does merge the offers; only the error vocabulary named after that operation changes.
  - R-7 puts the position types in `heading_address`, which sits below `execute`, so the new dependency points down.
  - W21 names `Error::originating_error`, which W20 introduces (R-13); W24 renames the H1 functions that W9's and W15's texts name (R-17).
  - Step granularity: the owner asked for as few steps as possible, with fixes combined per step. Group the work items into seven steps, in this order, and apply each step's work items in the order listed: (1) doc-only edits: W1, W2, W8, W9, W15, W18; (2) config, run state, shim modules, keyword spellings, and frame: W3, W4, W5, W6, W7, W14, W23; (3) scheduler effect table and task arena: W10, W11, W13; (4) chain record: W12, W25; (5) lookup, test support, and scheduler names: W16, W17, W24, W26; (6) facade API and guide: W20, W19; (7) error types, messages, and vocabulary: W21, W22, W27, W28. This supersedes the Risk note's one-work-item-at-a-time landing: the scheduler-core reshapes keep their own steps (3 and 4), and every step ends with its verification.
  - runvil-docs edits: a step that edits the guide leaves those edits in the runvil-docs worktree, outside the step's runvil commit. Once the step's runvil commit is finalized, the run commits them in runvil-docs with a subject that names that runvil commit's short hash. Steps 6 and 7 edit the guide.
- Rejected alternatives, assumptions, and risks:
  - Rejected S2-4's `Extends` type: the two-field `after`/`keep` shape follows the user's recorded constraint in `vibe/2026-10/2026-10-09-2-messages-userdata-refactor.md:60-61`, and changing it alters a closed public enum and the log format.
  - Rejected removing the `Option` around `SectionContext::vm` (S3-8): the consuming `teardown` places the Lua state's destruction between `LUA_TEARDOWN_STARTED` and `LUA_TEARDOWN_SUCCEEDED`, which the guide documents.
  - Risk: S2-13, S3-4, S3-5, S3-6, S3-7, and S3-22 all reshape the scheduler's core types. Land them one work item at a time, with the full suite between them.
  - Assumption: runvil-docs is edited in its own repository, in a commit that names the runvil commit it matches.
  - Rejected: `Requirements` as one list of gaps. The Harness builds and merges these reports, and S1-18 already makes `reserved` a set; renaming three fields gets most of the clarity for less churn.
  - Risk: R-18 and R-7 reshape `Chain` again after W12. Land W25 and W26 one at a time, after W12, with the full suite between them.

### Open Questions

1. Facade API changes (S1-9, S1-15, S1-18, S2-8, S5-2): answered (a), all five land now, in W20; the owner accepts the downstream breakage. S1-9's public part lands as R-11 (see the Decision Record), and the outline review's other public changes, R-10 and R-12 to R-15, join W20 under the same answer at the owner's request.
2. Author-facing messages the guide quotes (S4-7, S4-8, S1-3): answered (a) for all three; each message changes together with its guide quotes, in W21.
3. Rulebook vocabulary (S5-4, and S5-5's `AGENTS.md:9` edit): answered (a) for both, in W22.
4. S4-14's copied gateway wording: stays (a), the doc note W17 already makes, because this question is about adding facade API and moving caller-owned wording into the neutral crate, not about breakage.
5. The Lua error kind `internal` (R-16). Evidence: the Lua error rendering (`error/value.rs`) renders failures outside the prompt, transport and backend included, as `internal`; the guide maps a caught `internal` to `Completion` for a model failure, `Binding` for the missing-model error, and `Internal` for an Engine fault (`runvil-docs/src/language/16-limits-and-errors.md:959, 1047`); `AGENTS.md` asks that messages be written for a model to read. Options: (a) render `Error::Completion` as `model_failure` and `Error::ModelRequired` as `model_required`, keep `internal` for Engine faults, and update the guide, in W28; (b) keep `internal`. Consequences: (a) breaks prompts that test `err.kind == "internal"` to catch model failures; (b) keeps one kind for three unrelated conditions, so a model that reads a caught 503 sees `internal`. Recommendation: (a). Answered (a): W28 lands in this plan.

### Deferred and Out of Scope

- runvil-lua: `RAW_ID_CONTEXT`'s hand-written `NonZero` match (S1-10), the instruction-quota docs and `QuotaResource::Instruction` (S5-3), its "first model call" wording (S5-4), and the one-map `ToolSet` (S1-6). Revisit in runvil-lua's own vibelight pass.
- runvil-model-client: `src/model.rs:1-10` describes a catalog and an H1 `models.bind` that do not exist (S4-5, S5-6). Revisit in its own pass.
- runvil-parser: `src/frontmatter.rs:56, 167-168` say "each section's tool-call loop" (S5-1). Revisit with the parser's pass.
- Workshop's `keyword_wire` (`crates/workshop/server/src/routes/prompts.rs:174-186`) copies the keyword spellings (S1-14); fixing it needs a public method. Revisit in a Workshop pass.
- Whether a panicking performer should end a run as cancelled (V1-1) is a Harness question. Revisit in a Harness pass.
- The 32 items in the Spellings section. Revisit in a spelling pass.
- `script_call_counts`'s inline copy of `vm()`'s let-else (an S3-8 nit). Revisit if the accessor changes.
- Splitting `Scheduler` into an effect ledger and a task arena: W10 and W11 take the first steps. Revisit once W10 to W12 and W25 have landed.
- The mutex around the model set, which exists for the one writer `models.default`: removing it, and `Error`'s `From<ModelSetError>` with it, needs runvil-lua's section VMs and runvil-model-client's `ModelSet` to change. Revisit in those crates' passes.
- A general pass compressing doc comments that narrate event sequences, which make up most of the outline. Revisit with the refreshed outline after this plan lands.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p <crate>`. Plain `cargo build --locked` builds only the default member, the gateway (`crates/gateway/app`, package `gateway`, binary `runvil-gateway`). Building `gateway` or any Workshop crate needs `npm ci --prefix crates/workshop` and `npm ci --prefix crates/gateway/config-ui/ui` first, because their build scripts bundle the UIs into `OUT_DIR` (both `node_modules` trees are present locally). Clippy reports everything `cargo check` does, so no standalone `cargo check --workspace` runs.
- Focused test command pattern: `cargo nextest run --locked -p <crate> --all-features -E 'test(<test_name_substring>)'`. For `workshop`, `workshop-server`, and `workshop-server-api`, drop `--all-features`. cargo-nextest 0.9.128 is installed.
- Component test command pattern: `cargo nextest run --locked -p <crate> --all-features` (Workshop crates without `--all-features`). Engine unit tests live inside `src` under `#[cfg(test)]`, so `-p runvil-engine` runs them; the facade's black-box suite is `-p runvil`; the Harness suite is `-p harness-runner`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`.
- Linter command: `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`, both with `CARGO_BUILD_WARNINGS=deny` (PowerShell: `$env:CARGO_BUILD_WARNINGS='deny'`), plus `cargo check -p gateway --no-default-features` for the headless build shape. Structural gates beside it:
  - `cargo test -p build-xtask` - product and container boundaries, Workshop tier graph, `## Invariants` marker, lint inheritance, and the `fmt::Debug` spelling rule (`cargo xtask tidy` runs the same live checks).
  - `node --test crates/workshop/ui/test/docs-claims.mjs` - the Definitions wording rules; needs no `npm ci`, runs from any directory.
  - `cargo +<pinned nightly> xtask api --check` - facade surface against `crates/runvil/public-api.txt`, using the nightly named in `crates/build-xtask/src/api/toolchain.rs`, run with `CARGO_BUILD_WARNINGS` unset (`Remove-Item Env:CARGO_BUILD_WARNINGS`).
  - `cargo deny check` - supply chain, run by the pre-push hook when installed.
- Formatter check command: `cargo fmt --all --check` (`rustfmt.toml` sets `style_edition = "2024"`; the pre-commit hook runs it).
- Docs command: `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api` and `cargo doc -p runvil --no-deps` (default features), both with `RUSTDOCFLAGS="-D warnings"` (PowerShell: `$env:RUSTDOCFLAGS='-D warnings'`). CI also runs `cargo doc -p harness --no-deps` and `cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`.
- Test placement and naming conventions:
  - Unit tests sit in `src` as a `#[cfg(test)]` child module in a kebab sibling wired by path attribute: `foo.rs` declares `#[cfg(test)] #[path = "foo-tests.rs"] mod tests;`. A tests module that grows splits into `foo-tests-<label>.rs` siblings, or into a `tests/` subdirectory once it holds three or more files (for example `crates/runvil-internal/engine/src/execute/tests/discovery.rs` beside `discovery-refusals.rs`).
  - Shared test scaffolding lives in a crate's `src/test_support.rs` and `src/test_support/`, gated by `#[cfg(test)]` or the crate's `test-support` feature (`runvil-engine`, `runvil-lua`), which only benches and companion suites enable.
  - Integration tests compile as one binary per crate: `tests/suite/main.rs` (`runvil`) or `tests/it/main.rs` (`harness-runner`), one module file per topic plus `support.rs`; prompt fixtures sit under `tests/prompts/` (for example `tests/prompts/invalid/missing-h1.md`). Top-level `tests/` and `benches/` follow Cargo layout and are exempt from the flat-directory rule.
  - Benches live in `benches/` and name `required-features = ["test-support"]`.
  - Test functions are `#[test]` snake_case sentences stating the behavior, such as `a_section_that_offers_dynamically_lists_searches_and_calls_through_the_discovery_tools`; helpers carry a `///` line. `unwrap` and `expect` are allowed only in tests (`clippy.toml`).
  - Engine tests never sleep, read the wall clock, wait with a timeout, or draw OS entropy. Pseudo-random input comes from a fixed seed the failure prints (`test_support::seeded::Seeded`); temp directory names come from the test name, process id, and a counter.
  - Workshop UI tests are `node --test` `.mjs` files under `crates/workshop/ui/test/`; `tools/*.mjs` scripts each have a `.test.mjs` sibling.
- Directory map:
  - `crates/` - every Rust crate plus TypeScript UI packages. Manifestless containers hold private families: `runvil-internal/` (Engine internals), `harness-internal/` (Harness internals plus `test-plugin`), `gateway/` (gateway family, with `stt/` nested one level deeper), `workshop/` (Workshop crates and the npm workspace `ui`, `look`, `platform`). `crates/shared-ui` is a TypeScript and CSS package, not a crate.
  - `guide/` - docs site sources (`books/gateway`, `books/language`, `books/workshop`, `landing`, `chrome`), staged by `build-user-guide`.
  - `prompts/` - sample prompt files (`hello.md`, `greet.md`, `echo.md`, `analyst-example.md`, `research-person.md`).
  - `vibe/` - dated plans and debt-removal notes (`YYYY-MM-DD-N-<slug>.md`, older ones in monthly folders).
  - `tools/` - Node scripts that stage the gateway sidecar and run live TTS checks.
  - `images/` - README banners. `local/` - untracked operator config (gateway and MCP service config, env files, profiles, STT fixtures). `target/` - build output.
  - `.github/workflows/` - CI (`ci.yml`), nightly, release, docs site, Miri, and installer smoke jobs. `.githooks/` - pre-commit (fmt) and pre-push (wording scan, headless check, clippy, deny). `.config/nextest.toml` - nextest profiles and the heavy STT test group. `.cargo/config.toml` - `rust-lld` and static CRT on Windows MSVC, `cargo xtask` and `cargo workshop` aliases.
  - Root config: `Cargo.toml` (workspace members, shared dependency pins, lint levels), `clippy.toml`, `deny.toml`, `rustfmt.toml`, `rust-toolchain.toml` (stable), `dist-workspace.toml` (cargo-dist), `gateway.local.example.toml`.
  - Rulebooks: root `AGENTS.md`, `crates/workshop/AGENTS.md`, `.cursor/rules/workshop-architecture.mdc`, `.cursor/rules/workshop-spa.mdc`.
- Component boundaries:
  - Engine: the `runvil` facade over `crates/runvil-internal/*`. `runvil-types` and `runvil-vfs` are leaves; `runvil-model-client` depends on types; `runvil-lua` on model-client, types, vfs; `runvil-parser` on lua, types; `runvil-engine` on lua, model-client, parser, types, vfs; `runvil` re-exports all six. Engine crates never depend on `tokio`, `tokio-util`, `async-trait`, `reqwest`, `getrandom`, or `fastrand`, even in tests. The facade surface is pinned by `crates/runvil/public-api.txt`.
  - Plugin contract: `runvil-plugin` depends on runvil-types and runvil-vfs. The Packs `plugin-mcp`, `plugin-web`, and `plugin-user-input` depend only on `runvil-plugin`.
  - Harness: `harness-runner` depends on runvil and runvil-plugin; `harness-plugin-loader` on runvil-plugin; the `harness` facade on harness-runner, harness-plugin-loader, runvil, runvil-plugin; `harness-gateway-client` on harness, plugin-web, runvil.
  - Gateway: `gateway` depends on its private `gateway-*` family plus `gateway-api-types`, `gateway-api-discovery`, and `shared-loopback`, and on nothing from the Engine or Harness.
  - Workshop (a Host): `workshop-server` depends on harness, harness-gateway-client, the three Packs, runvil, runvil-plugin, gateway-api-discovery, shared-loopback, and the `workshop-*` family; the Tauri desktop app `workshop` depends on workshop-server-api and gateway-api-discovery.
  - Build tooling: `build-ceiling` (every crate's build script fails on a Rust file over 500 lines), `build-ui` (esbuild bundling into `OUT_DIR`), `build-xtask` (structural checks and the facade API check), `build-workshop`, `build-user-guide`, `build-llama-cuda`. Shared helpers: `shared-error-source`, `shared-loopback`.
- Conventions summary:
  - Seven capitalized defined terms (Engine, Harness, Host, Pack, Plugin, Static Set, Dynamic Set) each carry one meaning. Engine crates call their driver "the caller" and never mention the Host; the docs-claims scan enforces it.
  - Reuse an existing facility first, then make the smallest improvement to one, and only then add a new facility with a material benefit.
  - Behavior changes ship with tests in the same change. Prefer types and compiler checks, then behavior tests and deterministic fault injection; gate on yes/no properties.
  - Strict workspace lints that every crate inherits: clippy `all` and `pedantic` deny, `unwrap_used`, `expect_used`, `todo`, `allow_attributes` deny (suppress with `#[expect(..., reason = "...")]`), `unsafe_code` deny, `missing_docs`, `missing_debug_implementations`, `unreachable_pub` warn and are denied in the gate; rustdoc broken and private intra-doc links deny.
  - The `Debug` trait is always written `fmt::Debug` with `use std::fmt;` in scope.
  - Rust files stay at or under 500 lines. Source directories are flat; a subdirectory needs three or more files, otherwise `parent-label.rs` siblings wired with `#[path = "..."]`, converting in either direction as groups grow or shrink.
  - Files open with a `//!` summary and items carry `///` docs. Comments explain only non-obvious constraints; every workaround cites its upstream issue URL.
  - Error and status messages target model consumption: concise, self-contained, naming required versus actual.
  - JSON that reaches a recorder or replay round-trips exactly: sorted keys, finite numbers, `serde_json` with `float_roundtrip`.
  - Process-global installers (panic hook, logger, global tracing subscriber, rustls default provider) appear only in binary entry points, enforced by `clippy.toml` disallowed-methods.
  - Cargo features gate real constraints (toolchain, heavy native builds, `test-support`), never product shape; runtime and serve paths never compile native dependencies.
  - Dependency versions are centralized in `[workspace.dependencies]`, each unusual pin explained in a comment.
  - Commit subjects are imperative sentences (for example "Ask Packs to render their tools and route calls by Pack"); a finished plan closes with "Close plan: <slug>".
  - Workshop UI CSS uses only `@workshop/look` and `ui/src/tokens/component.css` custom properties; every persisted UI value goes through the `ui-storage` adapter to the server.

</project-survey>
<execution-plan>

## Execution Instructions

Seven steps, in the order the Decision Record's step-granularity entry fixes, grouped into six components. Each step is one commit holding its code and its tests, and ends with its Tests.

Components, in dependency order:

1. engine docs (step 1): doc comment and README corrections, plus W1's two vestige deletions. Placed first because it changes no behavior and later steps edit texts it writes: step 3 carries the task-slot docs onto new shapes, step 5 renames the H1 functions and "slot" inside them, and step 7 rewrites the `Error::Lua` doc. One piece, built jointly, because the edits touch separate doc comments and none reads another's result.
2. run setup (step 2): run inputs, run state, shim modules, keyword spellings, and the section frame. Placed before the scheduler because it settles the `pub(crate)` `RunLimits` fields that step 6 makes public and the `RunState`, `SectionContext`, and `ArrivedRound` shapes the scheduler reads. One piece, built sequentially in the listed order, because W23 needs W5's renames and W14's frame changes.
3. scheduler state (steps 3 and 4): the scheduler's core types. Placed after engine docs because W11 carries W9's slot wording. Two pieces, built sequentially, because each reshapes types the next one reads and the Decision Record's risk notes ask for the full suite between core reshapes: the effect table and task arena (W10, W11, W13), then the chain record (W12, then W25, which reshapes the `Chain` W12 leaves).
4. names and positions (step 5): rendering and lookup, test support, scheduler vocabulary, and position types. Placed after scheduler state because W24 renames what W9, W11, W13, and W15 wrote, and W26 needs W12's chain record and W16's `heading_address`. One piece, built sequentially for W16 then W26; W17 and W24 depend on neither.
5. public facade (step 6): the public API changes the owner accepted, plus the guide passages for tasks. Placed after the internal reshapes so the public surface changes once, on settled internals; R-15's `IssuedEffect` also types `Scheduler::issued`. One piece, built jointly, because W20's code and W19's guide edits touch separate files.
6. error vocabulary (step 7): author-facing messages, rulebook vocabulary, error types, and Lua error kinds. Placed last because W21 names `Error::originating_error` from W20, W27 needs W1, W21, and W22, and W28 needs W27. One piece, built sequentially in the listed order.

<step-1>

### Step 1: Correct the engine docs [completed]

- Component: engine docs
- Tests: run each from the runvil root in PowerShell, as written:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
- Commit: one commit, subject "Correct the engine's doc comments and README to match the code"

Paths are relative to `crates/runvil-internal/engine/src/` unless they start with `crates/`, name a repository-root file, or start with `runvil-docs/`, the sibling repository at `c:\Users\Vinnie\cursor\runvil-docs`. Line numbers cite `9f6f8080e`; earlier steps shift them, so find each site by the item it names and treat the number as a hint. Work item labels (W1 to W28) and finding ids (S, V, R) name parts of this plan; a reference to an item outside this step names work another step owns.

Work items, applied in this order:

- W1, error docs and vestiges (S1-1, S1-2, S1-4, S1-16, S2-18 type doc, S5-3 variant doc, S5-6):
  - `error.rs`: delete `#[non_exhaustive]` from `Error` (`:72-73`) and its variants (`:80, 98, 155, 170, 190, 207, 284, 302, 313, 332, 363, 380, 402`), and delete the sentence "Marked `#[non_exhaustive]` so future variants are not a breaking change." (`:67-68`).
  - `error.rs:133-140` (`Error::Lua`), new doc: "A condition the prompt author caused and can fix in Lua, with no originating `mlua` error to keep, such as an empty `messages` list or a heading that names no section. It also holds the message of a raised error table whose kind needs structure the table does not hold (see `Error::from_raised`). Failures that do have an `mlua` cause use [`Error::LuaRuntime`], and Engine misuse uses [`Error::Internal`]. The message is the specific failure as a noun phrase; the public wrapper classifies this as a Lua failure, so no redundant `lua error:` type label is prepended."
  - The module-doc exception sentence after `error.rs:13`, if kept, names only [`Error::Completion`], because W21 moves `BindSchema` to the module rule.
  - `error/convert.rs:208`: "The field `name` of a raised error table, when it is a string."
  - `error.rs:44-45` (`DepthVia`): "The call name [`Error::CallDepthExceeded`] reports for a nesting that passed the call-depth cap: `tasks.fanout` for a fanout arm, and `call` for a `call` or a `tasks.spawn`, which shares its cap."
  - `error.rs:376` (`Error::LuaQuota`), new first sentence: "A Lua log quota (log events or log bytes) ran out."
  - `error.rs:310-311` (`Error::ModelRequired`): "A `models.infer` or `models.loop` round found no `models.use` selection and no prompt-wide `models.default`." `error.rs:315` (its `section`): "The name of the section whose round had no binding, or the prompt's H1 title for a round in the H1 pass."
  - Delete `From<RunError> for Error` (`execute/error.rs:318-322`).
  - Check: the build and suite pass; `error/tests.rs:60-76` and `:290` stay green.
- W2, run outcome docs (S1-5, S1-15 docs, S5-2 docs, S5-3, V1-1, V1-2):
  - Skip the `Cancelled`, `is_cancelled`, and `Substitution` doc edits below, because W20 deletes those items. Make every other edit.
  - `execute/error.rs`: append "Lua code cannot catch this failure." to `RunErrorKind::Determinism`, and give `RunErrorKind::RequirementsUnmet` the full list now at `execute.rs:136-140` (a missing or unavailable required Pack, a service the caller left out, a model requirement a bound model fails, or an H1 block that failed the prompt's hard gate). `Quota` (`:39-41`): "A Lua log quota (log events or log bytes) ran out, or a `call`, `tasks.spawn`, or `tasks.fanout` arm would have nested past the call-depth cap." `Substitution` (`:47`): "No run reports this kind: an uncaught `{{ }}` substitution failure is reported as `Lua`, or as `RequirementsUnmet` in the H1 pass." `Cancelled`: "Classifies the cancelled error while a run is still going. A run that ends on it ends as [`RunResult::Cancelled`], so a finished run's error never has this kind." `is_cancelled` (`:179-183`): "Returns `true` when the error is the cancelled error a run cancel or a dropped answer raises. That error ends a run as `RunResult::Cancelled`, so the error a finished run reports never returns `true`." First sentence of `:10`: "Each variant names the condition that failed the run."
  - `execute.rs:108-140`: replace the kind list with "A failure's [`RunError`] has a [`kind`](RunError::kind) that classifies the failure by condition; [`RunErrorKind`] documents each kind."
  - `execute.rs:105`: "[`RunResult::Cancelled`] - the caller cancelled the run, or dropped an effect the run was waiting on and no `pcall` caught the cancelled error it resumed with." `execute.rs:151`: "The caller cancelled the run, or dropped an effect the run was waiting on and the cancelled error went uncaught." Append to `EffectAnswer::Dropped` (`execute/run/effect.rs:322-326`): "Uncaught, that error ends the run as `RunResult::Cancelled`."
  - `execute.rs:35-36`: "`section_vm` - the section VM setup both frame constructors share: a walked section's (a spawned task's first entry included) and the H1 pass's."
  - Check: both rustdoc gates (intra-doc links) and the suite.
- W8, run and effect docs (S2-1, S2-6):
  - `execute/run.rs:1-8`, new module doc: "The run: the Engine's boundary with its caller, which `Run`'s methods cross by exchanging effects and events as values. The caller's loop is documented on the `runvil` facade's crate page and its `effect` and `cancel` modules. The effect vocabulary - [`Effect`] with its [`Round`] and [`PackToolSets`], [`EffectAnswer`], [`EffectId`], the serializable records of effects and answers, and the [`ToolCallOrigin`] and [`ToolCaller`] it re-exports from `runvil-types` - comes from the `effect` child module and is re-exported here."
  - `execute/run/effect-record.rs:51-55`: replace "The `Chat` record keeps only what identifies the round: its id, the alias of the model slot it ran under, and the frozen invocation settings (temperature, generation cap, and thinking switch). It stores the request's messages in wire form after the first `keep`, which repeat round `after`'s request." with "The `Chat` record holds the round's id, the alias of the model slot it ran under, the frozen invocation settings (temperature, generation cap, and thinking switch), the advertised tool names, and the request's messages in wire form after the first `keep`, which repeat round `after`'s request." Keep the paragraph's remaining sentences.
  - Check: both rustdoc gates; `execute/run/tests.rs:43-78` stays green.
- W9, scheduler docs (S2-9, S2-10, S2-11, S2-16, S2-17, S2-18, S3-2, S3-3, S3-10, S3-25). Where W11 reshapes a field, carry the new wording onto the new shape.
  - `Scheduler::resuming` (`execute/scheduler.rs:187-191`): "Chains parked on a task wait that gave their admission slots back, waiting to take them again: a task chain, or a call chain that gave back its nearest holding ancestor's slots. Each keeps its Lua VM and holds its delivered answer; readmission marks the holder holding and requeues the chain. Admitted ahead of every fresh spawn." `Scheduler::spawned` (`:192-194`): "Freshly spawned task chains waiting for admission, in spawn order; admitted after every resuming chain. A queued spawn holds no Lua VM yet: admission is what starts it." Module doc (`:33-34`): say the admission queues hold resumed chains, admitted first, and fresh spawns, which wait there until the concurrency limits admit them.
  - Child counter: `allocate_child_id` (`execute/scheduler/chain.rs:60-63`) and `Counters::next_child` (`execute/scheduler.rs:155-156`) name the third user: "which `call` children, spawned tasks, and the wait shims' timers share". `Counters` (`:149`): replace "Set once at chain start" with "Set at chain start and advanced as the chain hands out ids". `execute/scheduler/tasks.rs:214-216`: "The task's id is the spawner's next child index, shared with `call` children and the wait shims' timers, so it depends only on the spawner's own dispatch order; the task is its chain, named from the other side." `execute/scheduler/dispatch.rs:336-338`: "The child's id is the caller's next child index: `call` children, spawned tasks, and the wait shims' timers share the caller's counter, so the id depends only on the caller's own dispatch order."
  - Timer slots: `Scheduler::tasks` (`execute/scheduler.rs:197-204`): "one slot per task the run has started, keyed by the task's id: its backing chain's id, or, for an internal timer, the id it took from its owner's child counter." `TaskState::Done`: "The backing chain or request ended; its outcome waits in the slot. `ok` is whether it returned rather than failed, always `true` for a fired timer." `TaskSlot::target`: "The name of the section the task's chain started at, which its terminal observations report under; the placeholder `timer` for an internal timer, which reports none." `TaskSlot::outcome`: "The backing chain's final text or failure, or an empty text for a fired timer, held from its end until the owner takes it."
  - `Scheduler::ready` (`execute/scheduler.rs:184-186`): "Chains eligible to resume (FIFO); empty whenever a step returns, because `drain` runs it dry or the run's end clears it."
  - `GENERIC_COMPLETION` (`execute/scheduler.rs:95-97`): "The run's final result when no section produced a reply, which three fallback sites share: a prompt with neither H1 blocks nor sections, a top-level walk that ends without a scalar, and an H1 pass with no walk after it."
  - `MAX_CALL_DEPTH` (`execute/scheduler.rs:92-93`): "The deepest nesting a chain may run at, counting `call` children and spawned tasks, fanout arms included; the first level counts." In `prepare_spawn`'s doc (`execute/scheduler/tasks.rs:147-150`), the refusal is "named `tasks.fanout` for an arm, and `call` for a `call` or a `tasks.spawn`".
  - `execute/scheduler/h1.rs:1-5`: "...under the walk's rules with these deltas: the frame takes the root chain's entry 0 and fires no section observations, a scalar return short-circuits the run, a Lua failure is the prompt's failed hard gate, mapped to [`Error::RequirementsUnmet`], and the pass's end hands off to the root walk. The frame's own deltas, writable `argv` among them, are listed on `SectionContext::new_live_h1`." `ChainKind::H1` (`execute/scheduler/chain_record-kind.rs:30-37`): "The H1 pass: the prompt's H1 blocks under its title. It runs the walk's rules with the deltas the `h1` module lists."
  - Lineage wording: where the docs say the H1 pass and the walk are "the same chain" or "one chain", say they share the root lineage `0` and are two arena records: `execute/scheduler/chain_record.rs:2-4, 162-164`, `execute/scheduler/h1.rs:5-6, 42-43, 87`, `execute/scheduler/task_end.rs:11-13, 214-216`, `execute/scheduler/walk.rs:8-9`, `execute/scheduler/tasks.rs:87-89`, `execute/scheduler/chain_record-kind.rs:50-51`, and `execute/scheduler/waits.rs:16-17`. In `admit_chain` (`execute/scheduler/admission.rs:162-184`), rename the binding `spawner` to `owner`, and say the start event fires "on its owner's sequence (its spawner, or the root walk for a task the H1 pass spawned)".
  - `execute/scheduler/admission.rs:10-11` and `Chain::concurrency` (`execute/scheduler/chain_record.rs:261-266`): "`tasks.concurrency` sets it, clamped to the parent's", in place of "lowers it".
  - `Chain::ctx` (`execute/scheduler/chain_record.rs:204-206`): "The chain's fork of the run state: the run's own for the H1 pass and for a root walk with no pass before it, the hand-off fork holding H1's frozen `argv` (`with_walk_state`) for the walk after the pass, and the caller's for a `call` child or the spawner's for a task, through `with_args` when the call or spawn gave an input. A task's fork also reports under its own task id with its own turn counter (`with_task`)."
  - Check: both rustdoc gates; `execute/tests/scheduler/concurrency.rs:14-31`, `:184`, and `execute/tests/timeouts.rs:57-68` stay green.
- W15, section VM and model round docs (S3-1, S3-12, V3-4, S3-11, V3-3, S3-27):
  - `execute/section_vm.rs:4-24`, first paragraph: "Both frame constructors - `SectionContext::new` for a walked section (a spawned task's first entry included) and `SectionContext::new_live_h1` for the H1 pass - run the sequence [`setup_section_vm`] lists. Only the deltas live at the call site:" followed by the deltas (the `sys` extras, a task's first entry adding `index`; the `VmSeed`; the `read` callback, the library resolver, and the prose renderer), and drop "run-global" and "the arm finishes its finalizer". `VmSeed::item` (`:46`): "The item seeded into this section, on a spawned task's first entry; `None` otherwise." Apply the same frame-constructor wording to `VmSeed::var` (`:42-44`), `SectionVmSetup` (`:52-53`) and its `sys`, `access`, and `seed` fields (`:62-70`), `setup_section_vm` (`:96`), `execute/section_context.rs:10` and `:195`, and `execute/walk_target.rs:8-9` ("`call` and `tasks.spawn` targets, fanout workers included").
  - Complete `setup_section_vm`'s list (`execute/section_vm.rs:95-103`) to match `install_after_injection` (`:167-202`): after the Engine globals, "the `ui()` global when the run supplied a snapshot"; after the coroutine shims, "the section's `models.loop` shim". Replace the other copies with a pointer to `setup_section_vm`: `execute/section_vm.rs:4-8`, `execute/section_context-construct.rs:6-8, 40-46, 153-160`, and the inline comment at `:123-127`, which becomes "The shared setup half ([`section_vm::setup_section_vm`]) follows; only the seed, the `sys` extras, and the callbacks' position are the walk's own."
  - Chat module doc (`execute/scheduler/chat.rs:17-21`): "emitting the round's events (the turn advance, then `report_model_turn`'s sequence, which ends in a text reply's report, or the tool-call batch after it) through the chain's own task-scoped emitter". `accept_chat` (`:259-273`), second paragraph: "A failure classifies as `ArrivedRound::failed` describes. A served completion advances the turn and reports through `report_model_turn`, a text reply's report included; a tool-call batch reports after it." Point `ArrivedRound::served` (`:402-406`) at `report_model_turn` instead of restating its sequence.
  - `accept_infer_completion` (`execute/infer.rs:27-33`): "Reports one completed infer round through [`turn::report_model_turn`], whose sequence ends in the `assistant_reply` tagged `origin = infer`, and renders its text. The no-tools-advertised violation check is what keeps the round tool-free."
  - `advance_turn` (`execute/turn.rs:16-22`): "Advances the shared turn counter, returning the 1-based index of the turn just started. The counter saturates at `u32::MAX`, so a run past four billion turns repeats that index rather than wrapping to reuse small ones." If the body's fallback needs a word, one comment: "The closure always returns `Some`, so the fallback never applies."
  - Check: both rustdoc gates; `execute/turn-tests.rs` stays green.
- W18, READMEs (S5-5, S5-7): `crates/runvil-internal/engine/README.md:3`, second sentence: "Each model round, Pack render, catalog tool call, `store` operation, and timer leaves the executor as an effect for the Harness to perform and answer, and every boundary is reported as an event. Adhoc tool calls, `GetDynamicTools` calls, refused discovery calls, and the store calls a Pack prelude makes while it installs run inside the Engine and are reported only as events, and the run's start probe reads the store directly." Last sentence: "Given the same prompt, arguments, and `RunContext` inputs, its seed and start time included, and the same answers, a run produces the same effects and events." `lib.rs:5-6` takes the same qualification in one clause ("every model round, Pack render, catalog tool call, store operation, and timer"). Check: the rustdoc gates.

Constraints:

- Shared wording across the plan: "the item seeded into this section" for seeded items, "per `models.loop` or model handle `loop` call" for the round cap, and "frame constructor" in place of "driver".
- Keep today's names (`new_live_h1` and the other live-H1 functions, `TaskSlot`, "slot"); step 5 renames them and rewrites these texts. Keep `RunErrorKind::Cancelled`, `RunError::is_cancelled`, and `RunErrorKind::Substitution` and their docs; step 6 deletes them.
- No message, `Display` output, or behavior changes, and `crates/runvil/public-api.txt` stays unchanged (`Error` is crate-private).

Pinned tests that must stay green, beside each item's Check:

- `error/tests.rs:60-76`, `:244-264`, and `:290` (S1-5, S1-2).
- `execute/run/tests-drops.rs:200-216, 294-320` (V1-1) and `execute/run/tests.rs:43-78`.
- `execute/tests/scheduler/concurrency.rs:14-31` and `:184`, and `execute/tests/timeouts.rs:57-68` (S3-10).
- `execute/turn-tests.rs`.

</step-1>

<step-2>

### Step 2: Simplify run setup and the section frame [completed]

- Component: run setup
- Tests: run each from the runvil root in PowerShell, as written:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
- Commit: one commit, subject "Simplify the run inputs, run state, and section frame, and drop the shim modules"

Paths are relative to `crates/runvil-internal/engine/src/` unless they start with `crates/`, name a repository-root file, or start with `runvil-docs/`, the sibling repository at `c:\Users\Vinnie\cursor\runvil-docs`. Line numbers cite `9f6f8080e`; earlier steps shift them, so find each site by the item it names and treat the number as a hint. Work item labels (W1 to W28) and finding ids (S, V, R) name parts of this plan; a reference to an item outside this step names work another step owns.

Work items, applied in this order (W23 needs W5 and W14):

- W3, config and limits (S1-7, S1-8, S1-9 internal part, S5-1, S1-10). Do S1-7 before S1-8.
  - `execute/config.rs:305-310` (`preludes`): "A prelude is Lua source that a Pack the prompt declares contributed. The caller passes them in the order the prompt declares the Packs. Every section VM of the run installs each prelude after the other Engine globals and before `require` and any block, so a prelude cannot call `require` while it installs, while every block, and every library a block loads with `require`, can call what the preludes define."
  - Shrink each `pub(super)` field doc in `execute/config.rs:54-113` whose setter or `new` states the contract (`provenance_start`, `report_debug`, `cancel`, `ui`, `vfs`, `tools`, `roster`, `vendors`, `preludes`; `model` goes in W20, R-10) to one line naming what the field holds and where it is set. Cut the type doc's copy of the seed and start-time rule (`:42-49`) to "The caller supplies the run's clock and randomness through `new`." Delete the lookup sentence from `model_bindings()` (`:380-381`).
  - `execute/config-limits.rs`: make the six fields `pub(crate)`, rename `concurrency` to `max_concurrency`, delete the getters `tool_iterations`, `concurrency`, `lua_memory`, and `lua_logs`, and read the fields at `execute/context.rs:301`, `execute/section_context-construct.rs:107-108, 194-195`, `execute/scheduler/waits.rs:249`, `execute/scheduler/walk.rs:69`, `execute/scheduler/h1.rs:54`, `execute/config-limits.rs:172-191`, `execute/tests/tool_loop.rs:90, 106`, and `execute/tests/scheduler/fanout/scheduling.rs:190`. Add no warning to the public `timeout()` doc: W20 replaces it with the public field `receive_timeout`, whose doc says the wait restarts with each part of the response (R-11).
  - Replace "per section" with "per `models.loop` or model handle `loop` call" at `execute/config-limits.rs:27, 48, 63`, `execute/context.rs:297`, and `execute/section_vm.rs:77`. `execute/config-limits.rs:65`: "A tool iteration is one model round of such a call; `models.infer` rounds do not count."
  - Replace each `const { nz_<T>(N) }` (`execute/config-limits.rs:54-58, 180-196`) with `const { NonZero::new(N).unwrap() }`, and delete the `nz!` macro and its three invocations, unless the clippy gate flags it.
  - Check: the clippy gate; `execute/config-limits.rs:169-204`; `crates/runvil/public-api.txt` unchanged.
- W4, roster (S1-6): in `RunContext::packs` (`execute/config.rs:282-301`), build `roster` first, then keep a usable entry's vendor only when `roster` maps its name to `None`. Add the S1-6 regression test from the Testing Plan. Check: it fails before the change and passes after.
- W5, run state (S1-11, S1-12, V3-1, S1-13, V5-1):
  - Delete `RunState::models` (`execute/context.rs:105-109`), its assignment (`:162`, `:176`), and its `Debug` line (`:424`); `models()` returns `&*self.model_set`. Update the `model_set` and `with_walk_state` docs that mention the second field.
  - `vm_setup` (`execute/context.rs:345-351`): drop the parenthesized input list, write "both frame constructors share" for "both section drivers share" and "the constructor supplies" for "the driver supplies". `sys_json` (`:376-380`): "The `sys` JSON for one section entry of this run", "with the constructor supplying only", and "A task's first entry adds `index` at its own call site."
  - Rename every `RunContext` parameter named `ctx` to `config`: `execute/context.rs:43, 144`, `execute/context-bound.rs:21, 61`, `execute/run.rs:132, 153, 296`, and the test helpers `execute/run/tests.rs:224`, `execute/context-tests.rs:31`, `test_support/scenario.rs:434`. Write "the chain's fork of the run state" at `execute/scheduler/chain.rs:40` (W9 rewrites `chain_record.rs:204-206`). Regenerate `crates/runvil/public-api.txt:484`.
  - At `execute/context.rs:334-336`, `execute/scheduler/chat.rs:354-356`, and `execute/scheduler/tasks.rs:178-181, 219-221`, replace "so its turns count against its own cap" (or "so the task's turns count against its own cap") with "so its turn numbers, and `tasks.status`'s `turns`, count only its own rounds".
  - Check: the suite and `xtask api --check`.
- W6, shim modules (S1-17, S4-1, S4-2, S4-4, S4-5, S5-6 module part):
  - Delete `execute/protocol.rs`, its `mod` line (`execute.rs:47`), and its layout bullet (`execute.rs:20-21`). Add its 15 names to `lua.rs`'s `pub(crate) use runvil_lua::{...}` list, repoint the 28 import lines (17 production, 11 test) from `crate::execute::protocol::` to `crate::lua::`, and import `execute.rs:70`'s two names from `crate::lua`.
  - Delete `tools.rs`, `untrusted.rs`, and their `mod` lines (`lib.rs:27-28`). Import `ToolCatalog` at `execute/config.rs:21`, and `GuardNonce` at `execute/context.rs:24`, `execute/tests.rs:29`, `lua/tests.rs:38`, and `model/tests.rs:11`, from `runvil_types`.
  - `lua.rs:1-12`, new doc: "The executor's import surface for `runvil-lua`, which implements the sandboxed section VM, the Engine globals, and the coroutine protocol; that crate documents them." `parser.rs:1-20`, new doc: "The executor's import surface for `runvil-parser`, which parses a prompt file into a [`Prompt`] tree; that crate documents the prompt format."
  - Move the comment at `lua.rs:14-16`, unchanged, to directly above the `use` at `lua.rs:26`.
  - `model.rs:6-7`: "and the model identity a run's roles are bound to ([`ModelInfo`], [`ModelId`])." `model.rs:9-12`: "`models.default` sets the prompt-wide default role, conventionally from the H1 body, and `models.use` sets a section entry's current selection, which a later call replaces. A `models.infer` or `models.loop` round with neither fails with a model-binding failure surfaced through [`crate::RunError`]." `model.rs:14-15`: say the binding vocabulary comes from the private `runvil-model-client` crate and `ModelId`, `ModelInfo`, and `ThinkingMode` from `runvil-types`.
  - Check: the build, the suite, and both rustdoc gates.
- W7, keyword spellings (S1-14): add `pub fn keyword_name(keyword: ModelKeyword) -> &'static str` with a total match to `crates/runvil-internal/parser/src/detail.rs`. Delete the Engine's `keyword_name` (`execute/context-bound.rs:37-52`) and call the new function at `:85` and for the `required` strings in `execute/fill.rs:39-47`. Optionally let each failing arm in `execute/fill.rs:39-47` hold its actual word (`("thinking", "Never")`, `("no-thinking", "Always")`) and delete `thinking_name` (`:62-73`). Check: the build; the guide's notice spellings `Never` and `Always` are unchanged.
- W14, frame (S3-17, S3-18, S3-23):
  - Rename `script_call_counts` to `tool_call_counts` (`execute/section_context.rs:217`, `execute/scheduler/tool_call.rs:241`).
  - Rename `install_section_scope` to `install_first_call_state` (`execute/section_context.rs:282-312`), with the doc "Installs what a section's first tool call fixes: the counts behind `tools.stats` and the model binding (`sys.model`)." Say "first-call install" for "scope install" at `execute/section_context.rs:205, 215, 230, 233, 293` and in the comments at `execute/tests/model_and_reply-globals.rs:2, 15, 65` and `execute/tests/scheduler/failures-script-tools.rs:201`.
  - Make the install return the counts and store them only after the model step and the `sys` re-seal succeed, so the slot means "fully installed", and delete the second unwrap (`execute/section_context.rs:217-238, 282-312`). Add the S3-23 regression test first.
  - Check: `execute/tests/scheduler/failures-script-tools.rs:200-216` stays green.
- W23, run state (R-1, R-2, R-5, R-19). After W5 and W14. Verify each finding at its site first.
  - R-1: move `ReportingHandles` (`execute/section_context.rs:112`) to `execute/context.rs`, documented as "A task's reporting handles: its task-scoped event emitter and its model-turn counter." Replace the `emitter` and `turns` fields of `RunState`, `SectionContext`, and `ArrivedRound` (`execute/scheduler/chat.rs:358`) with one `reporting: ReportingHandles`. `with_task` builds a new one; `RunState::emitter()` and `turns()` read through it, and `SectionContext::reporting_handles()` returns a reference.
  - R-2: `RunState::new` (`execute/context.rs:131`) and `prepare_state` (`execute/run.rs`) take `config: RunContext` by value and destructure it, moving `preludes`, `roster`, and `ui` where `OwnedInputs::take` moved them; delete `OwnedInputs` (`execute/context.rs:35-41`). `Run::new` takes what it still needs from the context, such as its cancel handle, before the call. If a caller needs the context after the state is built, keep the borrow and record why in the commit.
  - R-5: in `RunState`'s type doc, replace "Immutable for the run's lifetime and cheap to clone" with "Shared by every chain of the run and cheap to clone", and add: "Its handles are fixed for the run, though the cancel flag, the event buffer, the turn counter, and the model set's prompt-wide default change behind them."
  - R-19: `prompt()` returns `&Arc<Prompt>` and `prompt_arc` goes; `argv()` returns `Option<&Arc<serde_json::Value>>` and `argv_shared` goes; `args_shared` becomes `args()`, returning `&Arc<str>`. A caller that needs an owned handle clones it.
  - Check: the suite and `execute/context-tests.rs`.

Regression tests. Write each first; it must fail before its change and pass after:

- S1-6 (W4): call `RunContext::packs` with one name in both the usable and the unavailable list, build the run state, and assert the tool set's `vendor_for(name)` is `None`. Today `vendors` keeps the vendor, so the assertion fails.
- S3-23 (W14): a section whose first tool call fails in the model step, followed by a second tool call; assert the second call runs the model step again. Today the counts slot is already set, so the second call skips it. Build it on the failure seams in `execute/tests/scheduler/failures-script-tools.rs`; if no seam can make `resolve_model_binding` fail, record that in the commit message and rely on the new types.
- Optional pin for S5-1 (W3): a section under `max_tool_iterations: 1` with two loop calls that each run one tool round and then reply.

Decisions and constraints:

- S1-10 is conditional: if the clippy gate flags `const { NonZero::new(N).unwrap() }` under `unwrap_used`, the `nz!` macro stays, and its doc says why.
- S1-6 takes the local fix in `packs`. The one-map shape that mirrors the Harness's `PackState` needs a runvil-lua interface change and stays out of this plan.
- S1-17 routes the protocol names through `crate::lua`, so `lua.rs` stays the executor's one runvil-lua import list. The error bridge keeps naming `runvil_lua`, because it implements that crate's conversion and `ErrorValue` trait.
- S1-14's `thinking_name` half was rejected as a finding (its arms cannot run and its spelling is documented), so W7's deletion of `thinking_name` stays optional; the guide's notice spellings `Never` and `Always` must not change. Leave Workshop's `keyword_wire` copy alone.
- Shared wording: "per `models.loop` or model handle `loop` call" for the round cap, and "frame constructor" in place of "driver".
- Leave the deferred sibling code and wording alone: runvil-lua's `RAW_ID_CONTEXT` `NonZero` match, and runvil-parser's "each section's tool-call loop" (`crates/runvil-internal/parser/src/frontmatter.rs:56, 167-168`).
- W23's R items come from an outline review that read no Rust source. Verify each at its site first; where the code contradicts a finding, narrow or drop it and say why in the commit message.
- `crates/runvil/public-api.txt`: W3 leaves it unchanged; W5's `ctx` to `config` rename changes one line (`:484`). Regenerate it with `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --bless` and include it in the commit.

Pinned tests that must stay green, beside each item's Check:

- `execute/config-limits.rs:169-204`.
- `execute/context-tests.rs` (R-1, R-2, R-19).
- `execute/tests/scheduler/failures-script-tools.rs:200-216`.

</step-2>

<step-3>

### Step 3: Merge the effect maps and reshape the task arena [completed]

- Component: scheduler state
- Tests: run each from the runvil root in PowerShell, as written:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
- Commit: one commit, subject "Merge the scheduler's effect maps and reshape the task arena"

Paths are relative to `crates/runvil-internal/engine/src/` unless they start with `crates/`, name a repository-root file, or start with `runvil-docs/`, the sibling repository at `c:\Users\Vinnie\cursor\runvil-docs`. Line numbers cite `9f6f8080e`; earlier steps shift them, so find each site by the item it names and treat the number as a hint. Work item labels (W1 to W28) and finding ids (S, V, R) name parts of this plan; a reference to an item outside this step names work another step owns.

These items reshape the scheduler's core types. Apply them one at a time, in this order, and run `cargo nextest run --locked -p runvil-engine --all-features` after each before starting the next:

- W10, effect table (S2-13): replace `pending` and `orphaned` (`execute/scheduler.rs:195-197, 208-216`) with `outstanding: HashMap<EffectId, Outstanding>`, `enum Outstanding { Parked(Pending), Orphaned }`, and one field doc. `issue` inserts `Parked` (`execute/scheduler.rs:310`). `resume` (`execute/scheduler/drive.rs:103-105`) removes the entry once and matches: missing fails with the internal error, `Orphaned` is discarded, and `Parked` goes to `apply_answer`, which takes the `Pending` instead of looking it up (`execute/scheduler/apply.rs:78-83`). The stall check (`execute/scheduler/drive.rs:49-55`) tests for no `Parked` entry. `Done` (`:60-62`) waits for `outstanding.is_empty()`. `teardown` (`:230-231`) flips every entry to `Orphaned`. `abort_subtree` (`execute/scheduler/chain.rs:259-268`) and `reassign_tasks` (`execute/scheduler/task_end.rs:230-234`) filter on `Parked`. `abort_effect` (`execute/scheduler/chain.rs:303-313`) flips an existing entry and does nothing for a missing id. Add the S2-13 regression test. Check: it fails before the change; `execute/tests/scheduler/failures.rs:209, 373-398`, `execute/run/tests.rs:300`, and `execute/tests/parallel_tool_calls.rs:234` stay green.
- W11, task arena (S2-12, S3-4, S3-16, S3-5, S3-6, S3-13 code). Depends on W9 for the slot docs.
  - `tasks: BTreeMap<TaskId, TaskSlot>` (`execute/scheduler.rs:74, 204, 261`); delete the sort and the hash-map comment in `live_tasks_of` (`execute/scheduler/waits.rs:76-79, 87`) and `abandon_owned_tasks` (`execute/scheduler/task_end.rs:140-141, 148`), keeping their `collect`.
  - Remove `owner` from `ChainKind::Task` (`execute/scheduler/chain_record-kind.rs:49-55`) with `ChainKind::owner` and `ChainKind::enclosing` (`:64-85`). Make `Scheduler::enclosing` the one implementation, reading a task chain's slot owner and a call chain's parent, and use it in `slot_ancestors` (`execute/scheduler/admission.rs:27-39`). `finish`'s `is_task` (`execute/scheduler/chain.rs:147`) becomes `matches!(kind, ChainKind::Task { .. })`; `admit_chain` reads the slot it already fetches (`execute/scheduler/admission.rs:175-201`); `prepare_spawn` stops writing the kind's owner (`execute/scheduler/tasks.rs:231-237`); `reassign_tasks` drops its third loop (`execute/scheduler/task_end.rs:235-241`).
  - Move `exec` into `TaskBacking::Chain { chain, exec }` and delete `TaskSlot::exec` (`execute/scheduler/tasks.rs:39-40, 98-102, 251, 256`, `execute/scheduler/timer.rs:59`, `execute/scheduler/task_end.rs:101, 118, 157`, `execute/scheduler/waits.rs:214, 268, 301`).
  - Hold the outcome in the slot's state as `Done(Result<String>)`, drop `TaskSlot::outcome`, let `deliver` move the result out with `std::mem::replace(&mut slot.state, Delivered { ok })`, and delete its unreachable internal error (`execute/scheduler/waits.rs:149-153`). Keep today's `Copy` `TaskState` as a derived view for `task_phase`, `ok()`, `execute/scheduler/test_hooks.rs:18, 57`, and `test_support/rig.rs:19, 201`. Sites: `execute/scheduler/tasks.rs:46-80, 94-97, 254-255`, `execute/scheduler/task_end.rs:46-48`, `execute/scheduler/timer.rs:57-58, 84-85`, `execute/scheduler/waits.rs:46-52, 119-129, 147-162, 205-206, 290`, `execute/scheduler/apply.rs:312-316`.
  - Move the note to `TaskSlot::note`: `dispatch_note` writes the slot of the task the chain runs inside and records nothing for task `0`, `task_status` reads the slot, and `Chain::note` goes (`execute/scheduler/chain_record.rs:201-203`, `execute/scheduler/chain.rs:113`, `execute/scheduler/waits.rs:16-23, 214-218, 261-273`). Update `dispatch_note`'s doc and the waits module doc to say a note outside a slotted task records nothing.
  - Check: the 14 `TaskState` assertions (`execute/tests/waits.rs:303`, `execute/tests/tasks-chain-end.rs:71, 107, 130, 251, 314, 354, 402`, `execute/tests/tasks.rs:144, 188`, `execute/tests/scheduler/failures.rs:340`, `execute/tests/timeouts.rs:61, 92, 168`), `execute/tests/tasks-chain-end.rs:37-61, 320`, `execute/tests/waits.rs:335`, `execute/tests/happens_before.rs`, and the `tasks.note` tests in `execute/tests/waits.rs`.
- W13, small scheduler changes (S2-14, S2-15, S3-9, S3-15, S3-20):
  - `const MAX_CHAINS: usize = u32::MAX as usize;` in `execute/scheduler.rs`, with a doc giving the `u32` index-space reason; a `#[cfg(test)] max_chains: usize` field initialized to it in `Scheduler::new` (`:266`); `start_chain` (`execute/scheduler/chain.rs:89-91`) compares against the field under `#[cfg(test)]` and against `MAX_CHAINS` otherwise. `set_max_chains_for_test` and its test stay.
  - Rename `issue_infer` to `prepare_infer` (`execute/scheduler/dispatch.rs:217, 225`).
  - `Continuation::Vfs(VfsOp)`: delete `VfsContinuation` (`execute/scheduler/pending.rs:44-49, 56-66`), build the continuation from the op (`execute/scheduler/dispatch.rs:271-281`), and have `accept_vfs` call `vfs_observations(&op)` (`execute/scheduler/apply.rs:284-298`); update `execute/scheduler.rs:89`.
  - Replace `ChatDispatch` and `ToolCallDispatch` with `pub(super) enum Dispatched { Issued, Answered(Answer<Error>) }` in `execute/scheduler/dispatch.rs`, documented once; move the arm-specific remarks to the preparers' docs. Sites: `execute/scheduler/chat.rs:60-67, 84-85, 113, 146, 186, 205`, `execute/scheduler/tool_call.rs:69-77, 114-115, 324, 330`, `execute/scheduler/tool_call-adhoc.rs:16, 28, 47`, `execute/scheduler/tool_call-discovery.rs:36, 84, 137, 151, 161`.
  - Move `unbound_tool_call` (`execute/scheduler/dispatch.rs:30-46`) into `execute/scheduler/tool_call.rs` and `classify_vfs_failure` (`:104-119`) with `vfs_observations` into `execute/scheduler/apply.rs`, as private functions.
  - Check: the build and the suite; `execute/tests/observations.rs` and `execute/tests/scheduler/failures.rs:329` stay green.

Regression test. Write it first; it must fail before W10's change and pass after:

- S2-13 (W10): through `execute/scheduler/test_hooks.rs`, abort an effect id that is not outstanding and assert that nothing is recorded as orphaned. Today `abort_effect` inserts unconditionally, so the assertion fails. If no hook there reaches `abort_effect` or the orphaned state, add `#[cfg(test)] pub(crate)` hooks beside the existing `*_for_test` methods.

Decisions and constraints:

- S3-4 keeps the owner on `TaskSlot`, where 10 of the 14 reads already look, rather than on `ChainKind`; that costs one map lookup per admission ancestor step, and depth is capped at 8. S3-16 therefore keeps `Scheduler::enclosing` as the one implementation.
- S3-5 keeps a `Copy` view of the task state, so the 14 test assertions keep their shape.
- S3-13 puts the note on `TaskSlot`, which `task_status` already reads and which outlives the chain.
- Step 1 (W9) wrote the task-slot docs; carry them onto the new shapes rather than restoring older text. They read: `Scheduler::tasks`: "one slot per task the run has started, keyed by the task's id: its backing chain's id, or, for an internal timer, the id it took from its owner's child counter." `TaskState::Done`: "The backing chain or request ended; its outcome waits in the slot. `ok` is whether it returned rather than failed, always `true` for a fired timer." `TaskSlot::target`: "The name of the section the task's chain started at, which its terminal observations report under; the placeholder `timer` for an internal timer, which reports none." `TaskSlot::outcome`: "The backing chain's final text or failure, or an empty text for a fired timer, held from its end until the owner takes it." When W11 drops `TaskSlot::outcome`, move that sentence onto the `Done` state that now holds the outcome.
- Keep today's names (`TaskSlot`, "slot", the live-H1 functions); step 5 renames them.
- Splitting `Scheduler` into an effect ledger and a task arena stays out of this plan; W10 and W11 are only its first steps.

Pinned tests that must stay green, beside each item's Check:

- `execute/tests/tasks-chain-end.rs:37-61` and `execute/tests/waits.rs:335` (S2-12).
- The 14 `TaskState` assertions listed in W11 (S3-5).
- `execute/tests/happens_before.rs` (S3-6).
- `execute/tests/observations.rs` (S3-9).

</step-3>

<step-4>

### Step 4: Reshape the chain record [completed]

- Component: scheduler state
- Tests: run each from the runvil root in PowerShell, as written:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
- Commit: one commit, subject "Install a chain's access at birth and fold its parked and entered state into single values"

Paths are relative to `crates/runvil-internal/engine/src/` unless they start with `crates/`, name a repository-root file, or start with `runvil-docs/`, the sibling repository at `c:\Users\Vinnie\cursor\runvil-docs`. Line numbers cite `9f6f8080e`; earlier steps shift them, so find each site by the item it names and treat the number as a hint. Work item labels (W1 to W28) and finding ids (S, V, R) name parts of this plan; a reference to an item outside this step names work another step owns.

R-18 reshapes `Chain` again after W12. Apply W12, run `cargo nextest run --locked -p runvil-engine --all-features`, and only then apply W25:

- W12, chain record (S3-7, S3-19, S3-14, S3-22):
  - Add `access: Arc<Access>` to `ChainStart` and install it in `start_chain` (`execute/scheduler/chain.rs:33-57, 104-129`). `prepare_call` passes `Arc::clone(chain.access()?)` (`execute/scheduler/dispatch.rs:326-354`); the H1 pass, the walk after it, and tasks pass the capability they already hold (`execute/scheduler/h1.rs:45-56, 107-118`, `execute/scheduler/tasks.rs:225-243`); `start_root_walk` acquires before `start_chain` (`execute/scheduler/walk.rs:52-107`). Delete the four post-start installs and `install_root_slots`. Update the `Chain::access` doc: `None` only after the chain ends.
  - `Chain::seed: TaskSeed` (`execute/scheduler/chain_record.rs:183-185`), consumed with `std::mem::take(&mut chain.seed)` in `enter_section` (`execute/scheduler/walk.rs:162`); `start_chain` uses the default (`execute/scheduler/chain.rs:109`), `prepare_spawn` sets the seed (`execute/scheduler/tasks.rs:244`), and `admit_chain` reads `chain.seed.item.clone()` and `chain.seed.index` (`execute/scheduler/admission.rs:194-195`).
  - Replace `blocked`, `waiting_on`, and `batch` (`execute/scheduler/chain_record.rs:186-200`) with `parked: Option<Parked>`, whose variants carry the join set and the batch slots, and add a `blocked()` projection that `tasks.status` reads (`execute/scheduler/waits.rs:224`). Keep today's report for the moment between a wake and its resume: a requeued chain reports `Queued`, and a chain woken inline reports `Tasks`. Sites: `execute/scheduler/dispatch.rs:129`, `execute/scheduler/tasks.rs:247`, `execute/scheduler/admission.rs:157, 192, 278`, `execute/scheduler/step.rs:63`, `execute/scheduler/waits.rs:104, 178`, `execute/scheduler/apply.rs:323`, `execute/scheduler/tool_call.rs:160, 283-301`, and the two triple clears at `execute/scheduler/chain.rs:275-277` and `execute/scheduler/drive.rs:224-226`.
  - Check: `execute/tests/waits.rs:124-146`, `execute/tests/timeouts.rs:32, 185`, the spawn-seed tests (`execute/tests/suite/exec_flow/chains.rs:281-309`, `execute/tests/suite/exec_flow/fanout_collections.rs`), and `execute/tests/parallel_tool_calls.rs`.
- W25, chain lifecycle (R-18). After W12; land it alone.
  - First confirm the invariants the field docs state: a coroutine exists only inside an entered section, an answer is delivered only to a suspended coroutine, and `block` and `pending_prose` reset at every entry. Keep outside the new type any field that a site contradicts, and name the site in the commit.
  - Replace `frame`, `block`, `pending_prose`, `coroutine`, and `incoming` on `Chain` (`execute/scheduler/chain_record.rs:161`) with `entered: Option<Entered>`. `Entered` holds the frame, the block index, the pending prose, and `running: Option<Running>`; `Running` holds the coroutine and the delivered answer. `enter_section` and the H1 pass's start build it; `end_section`, `finish`, and `abort_subtree` take it, and the frame's drop stays the teardown boundary; `start_lua` sets `running`, and a block's return clears it.
  - Paths that reach a chain through a running or suspended block borrow the `Entered` directly, so they lose the `Error::Internal` path of `frame()` and `frame_mut()`; keep those accessors only for callers that can meet a chain between sections.
  - Check: the suite, with `execute/tests/waits.rs`, `execute/tests/timeouts.rs`, and the walk tests under `execute/tests/scheduler/walk/` green.

Decisions and constraints:

- S3-22 keeps today's `tasks.status` output: a requeued chain reports `Queued`, and a chain woken inline reports `Tasks` until it resumes.
- S3-7: `ChainStart` gains `access: Arc<Access>`, and `Chain::access` stays an `Option` only so the chain-end paths can take it. `install_root_slots` goes (S3-19).
- The frame's drop stays the teardown boundary, and the `Option` around `SectionContext::vm` stays: the consuming `teardown` places the Lua state's destruction between `LUA_TEARDOWN_STARTED` and `LUA_TEARDOWN_SUCCEEDED`, which the guide documents.
- W25's R-18 comes from an outline review that read no Rust source. Confirm its invariants at their sites first; where the code contradicts the finding, narrow or drop it and say why in the commit message.

Pinned tests that must stay green, beside each item's Check:

- `execute/tests/waits.rs:124-146` and `execute/tests/timeouts.rs:32, 185` (S3-22).
- All of `execute/tests/waits.rs` and `execute/tests/timeouts.rs`, and the walk tests under `execute/tests/scheduler/walk/` (R-18).

</step-4>

<step-5>

### Step 5: Unify rendering, positions, and scheduler names [completed]

- Component: names and positions
- Tests: run each from the runvil root in PowerShell, as written:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `cargo bench -p runvil-engine --features test-support --no-run`
- Commit: one commit, subject "Share one rendering rule and one set of position types, and rename the task arena's entries"

Paths are relative to `crates/runvil-internal/engine/src/` unless they start with `crates/`, name a repository-root file, or start with `runvil-docs/`, the sibling repository at `c:\Users\Vinnie\cursor\runvil-docs`. Line numbers cite `9f6f8080e`; earlier steps shift them, so find each site by the item it names and treat the number as a hint. Work item labels (W1 to W28) and finding ids (S, V, R) name parts of this plan; a reference to an item outside this step names work another step owns.

Work items, applied in this order (W26 moves the position types into the `heading_address` module W16 edits):

- W16, `subst` and `heading_address` (S4-6, S4-7 docs, S4-8 doc, S4-9, S4-10, S4-11):
  - `render` (`subst.rs:385-406`) checks null first, keeping `NullValue` and its message, and otherwise returns `Ok(crate::lua::render_item(value))`. Delete `render_scalar` (`:126-134`) and the forwarder `subst::render_item` (`:136-144`); the item arm (`:278`) and the `Sources` doc link (`:154`) name `crate::lua::render_item`. `SubstErrorKind::Serialize` (`:61-62`): "A bare global was not JSON data." Move the null assertion from `subst-tests.rs:398-407` into runvil-lua's `render_item_renders_by_type` (`crates/runvil-internal/lua/src/collection.rs:337`) and delete the forwarder test.
  - Describe `item` as "the item seeded into this section" (a fanout arm's or a `tasks.spawn` task's first entry) at `subst.rs:7-8, 15, 56, 152-153, 161`.
  - `SubstErrorKind::NotATable` (`subst.rs:50`): "A key path after `args` or `item`, which render whole only." `MissingKey` (`subst.rs:52`): "A key path found no value: under `var`, `sys`, `argv`, or a bare global." `substitute`'s `# Errors` (`subst.rs:176-180`) adds a bare `{{ var }}` or `{{ sys }}` with no key (`BadPath`) and a key path after `args` or `item` (`NotATable`).
  - `heading_address.rs:4-18`: keep only the module's own contract (which surfaces resolve headings through `find_visible` and `resolve_visible`, and `require`'s wider `Reachable` view), and drop the fanout member-order, shim, spawning, and admission sentences, which runvil-lua's collection module, its `FANOUT_SOURCE` doc, and the guide state.
  - Drop `Visible::children` (`heading_address.rs:34-38, 53, 63`) and compute it in `Visible::iter` as `self.caller.and_then(|c| self.home.get(c)).map_or(&[][..], Section::children)`.
  - Check: the `subst-tests.rs` tests named in the Testing Plan and `heading_address-tests.rs`.
- W17, test support (S4-12, V4-1, S4-14 doc, S4-15, V3-2):
  - Delete `ScriptedChat::calls` (`test_support/scripted-chat.rs:271-276`): `complete_now` takes the `requests` lock once, uses its `len()` as the reply index, and pushes before unlocking; `call_count()` returns the locked `len()`.
  - Delete `delayed`, `ScriptedReply::Delayed`, `ScriptedReply::delay`, `timed_out`, `complete_now`'s `timeout` parameter and comparison, and `Answers::timeout` (`test_support/scenario-driving.rs:42, 74`, its source at `test_support/scenario.rs:336`, and `benches/models_loop.rs:108`). Script `text("too late")` in `execute/tests/scheduler/walk/driving.rs:49-52` and `execute/tests/live_infer.rs:204-207`, and fix the "30-second reply" comment at `driving.rs:46-47`. Drop the timeout clauses from the module doc (`test_support/scripted-chat.rs:7-9`), `test_support/scenario.rs:115`, and `test_support/scenario-serial.rs:13-16`.
  - Module doc (`test_support/scripted-chat.rs:6-9`): replace "The answers follow the rules the gateway client's reader applies" with "The empty-reply wording copies `harness-gateway-client`'s (`src/wire/parse.rs`), because the Engine cannot depend on that crate; the two copies change together." (Open Question 4, answered (a).)
  - `fail` (`test_support/scripted-chat.rs:79-82`): say it builds the kind's phrase with the status appended and the body as the detail for every kind, skipping the gateway client's classification (no credentials wording for 401 and 403, no parsed overflow counts), so each test names its kind directly.
  - `execute/tests/suite/exec_flow/fanout_arms.rs:272-275`: "Dropping the `+ 1` in `CallDepth::deeper`, which every spawn applies, must fail this test." `execute/tests/suite/exec_flow/chains.rs:311-320`: "`sys.index` exists only on a spawned task's first entry (a fanout arm's, or a `tasks.spawn` with `opts.index`); reading it in a walked section raises the sealed-sys unknown-field error", with the message "sys.index is absent in a walked section". `execute/tests/suite/exec_flow/globals.rs:40-42`: "so only a spawned task's first entry (a fanout arm's included) may reference it".
  - Check: the suite and the bench build (`cargo bench -p runvil-engine --features test-support --no-run`).
- W24, scheduler names and docs (S2-3, S2-19, R-6, R-17). After W9, W11, W13, and W15.
  - S2-3: rename `run::Phase` (`execute/run.rs:92`) to `run::Start` and `Run::phase` to `start`, keeping the variants. The scheduler's `phase` field doc (`execute/scheduler.rs`): "Where the step loop stands: not started, running, ending with the outcome held until the orphans are answered, or done."
  - S2-19: add to the scheduler module doc's protocol paragraph: "Every `dispatch_*` arm resumes its fallible half's error into the calling chain as the call's answer, so an author `pcall` catches it. An `Error::Internal` from a scheduler method marks a state the scheduler's own invariants rule out: a scheduler or shim bug." Cut the restated rule from `dispatch_chat`, `dispatch_spawn`, `dispatch_tool_call`, `dispatch_infer`, and `dispatch_call`, keeping each arm's own failures, and from `dispatch_timer`, keeping its note that the wait shim raises the error before any wait. Drop "which only a scheduler bug produces" and "which only a shim bug produces" from each `# Errors` clause, keeping the invariant the clause names.
  - R-6: rename `TaskSlot` (`execute/scheduler/tasks.rs:84`) to `TaskEntry`, `owned_slot` to `owned_entry`, and `visible_slot` to `visible_entry`, with their uses in `execute/scheduler/test_hooks.rs` and `test_support/rig.rs`. Where a doc means the arena record (the tasks and waits module docs, `Scheduler::tasks`, `TaskBacking`, `TaskState`, and the texts W9 and W11 wrote), say "entry"; "slot" stays for admission slots and batch slots.
  - R-17: rename `new_live_h1` to `new_h1`, `start_live_h1` to `start_h1`, `end_live_h1` to `end_h1`, and `end_live_h1_at_jump` to `end_h1_at_jump`, and write "the H1 pass" for "the live H1 pass" in every doc and comment, the texts W9 and W15 wrote included.
  - Check: both rustdoc gates and the suite; a grep finds no `TaskSlot` and no "live H1".
- W26, position types (R-7). After W12 and W16; land it alone.
  - Move `SlicePath` (`execute/walk_target.rs:29`) and `SectionIndex` (`execute/scheduler/chain_record.rs:84`) into `heading_address.rs`, which sits below `execute`.
  - `Reachable::path` and `Reached::path` (`heading_address.rs:84, 93`) take `SlicePath`. `Visible::of`'s and `Reachable::of`'s `caller`, `Reached::index`, `RequireOrigin::Section` (`execute/walk_target.rs:148`), `library_key`, `resolve_jump_target`'s `jumper`, `JumpTarget`'s variants (`execute/walk_target.rs:103`), and `SectionContext::new`'s `index` take `SectionIndex`.
  - `ChainTarget` (`execute/scheduler/walk.rs:33`) becomes `{ jump: JumpTarget, slice: SlicePath }`, and `resolve_chain_target` builds it from `resolve_jump_target`'s result.
  - Check: `heading_address-tests.rs`, the jump and walk tests, and the suite.

Decisions and constraints:

- W16 changes only the docs for S4-7 and S4-8. Keep the `NilItem` message "{{ item }} is nil (not inside a fanout arm)" and the `item` key-path message "item is a string, not a table" unchanged here; step 7 changes them together with their guide quotes.
- V4-1 deletes the scripted delay rather than pinning it; a suite that needs a timed-out round scripts `ScriptedReply::Failure` with `CompletionErrorKind::Timeout`. S4-13's doc rewrite goes with it.
- S4-14: the copied gateway wording stays, with the doc note W17 writes (Open Question 4, answered (a)).
- R-7 puts the position types in `heading_address`, which sits below `execute`, so the new dependency points down.
- R-6: "slot" stays for admission slots and batch slots. R-17: the guide says "the H1 pass", so "live" goes from names, docs, and comments.
- W24's and W26's R items come from an outline review that read no Rust source. Verify each at its site first; where the code contradicts a finding, narrow or drop it and say why in the commit message.
- Acceptance for W24: `rg -n "TaskSlot|live H1|live_h1" crates/runvil-internal/engine` finds no match.

Pinned tests that must stay green, beside each item's Check:

- The rendering tests in `subst-tests.rs` (S4-6).
- The `call_count()` assertions (S4-12).
- `heading_address-tests.rs` and the jump tests (R-7).

</step-5>

<step-6>

### Step 6: Reshape the facade API and fix the guide [completed]

- Component: public facade
- Tests: run each from the runvil root in PowerShell, as written:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
- Commit: one runvil commit, subject "Reshape the facade's run limits, context, results, errors, and requirements"; the guide edits stay outside it (see below)

Paths are relative to `crates/runvil-internal/engine/src/` unless they start with `crates/`, name a repository-root file, or start with `runvil-docs/`, the sibling repository at `c:\Users\Vinnie\cursor\runvil-docs`; W19's `src/language/` paths are in runvil-docs. Line numbers cite `9f6f8080e`; earlier steps shift them, so find each site by the item it names and treat the number as a hint. Work item labels (W1 to W28) and finding ids (S, V, R) name parts of this plan; a reference to an item outside this step names work another step owns.

Work items, applied in this order:

- W20, facade API changes (S1-15, S1-18, S2-8, S5-2, and R-10 to R-15, with S1-9's public part as R-11). The listing lines S1-9 to S5-2 change are `crates/runvil/public-api.txt:507, 515-519, 527-530, 828, 832, 841`; the R items also change the `RunLimits`, `RunContext`, `RunResult`, `RunError`, `Requirements`, and `Step` entries. Verify each R item at its site first.
  - R-11, in place of S1-9's getter renames (getters named `request_timeout()` and `max_response_bytes()` would sit beside the setters of those names in `RunLimits`'s impl, `execute/config-limits.rs:45`, and not compile):
    - `RunLimits`: mark it `#[non_exhaustive]`, make its six fields `pub`, rename `request_timeout` to `receive_timeout`, move each setter's doc onto its field (the receive timeout's says the wait restarts with each part of the response, so it is not a whole-request deadline), move `new`'s list of defaults to the `Default` impl, and delete the six setters, `timeout`, `response_bytes`, and `new`. A caller writes `let mut limits = RunLimits::default(); limits.max_concurrency = n;`.
    - `RunContext`: rename the setters `cancel`, `flags`, `limits`, `packs`, `preludes`, `provenance_start`, `report_debug`, `tools`, `ui`, and `vfs` to `with_cancel` and so on, and the getters `catalog`, `run_flags`, and `vfs_handle` to `tools`, `flags`, and `vfs`.
    - Update every caller, `crates/harness-gateway-client/src/broker.rs:73` included, and grep the workspace for the rest. In runvil-docs, `src/language/17-quick-reference.md:263, 276` name `max_concurrency` and `receive_timeout`; re-sort the rows.
  - S1-15: delete `RunErrorKind::Cancelled` and `RunError::is_cancelled`, and map `Error::Interrupted` to `RunErrorKind::Internal` in `kind`, since no finished run reports it. Update `execute/error.rs:164, 179-183`, `error/tests.rs:238`, and `execute.rs:132-134`.
  - S5-2: delete `RunErrorKind::Substitution` and map `Error::Substitution(_)` to `RunErrorKind::Lua` at `execute/error.rs:165`.
  - S1-18: make `Requirements::reserved` a `BTreeSet<PackId>`. Sites: `execute/config.rs:360`, `execute/requirements.rs:42-49, 81-83`, and `execute/requirements-tests.rs:76, 113, 146, 153, 159`.
  - S2-8: mark the `ChatAnswerRecord` builder, `ChatAnswerRecord::new` and its three `with_*` setters (`execute/run/effect-record.rs:196-228`), `#[cfg(test)] pub(crate)`, following the rule of `890b41faa` ("Trim the public API of the Engine": items only tests call are `#[cfg(test)]`).
  - R-10: delete `RunContext::model` and its field; `prepare` takes `model: Option<ModelInfo>` after `prompt` and passes it to the fill (`execute/fill.rs`). Its doc says every declared role binds to `model`, and that `None` skips the fill and the checks. Update the callers.
  - R-12: rename `RunResult::Ok` to `RunResult::Completed`, drop the doc's warning about `Result::Ok`, and update every match in the workspace and the facade docs.
  - R-13: rename `RunError::cause` and `Error::cause` to `originating_error`, with every doc that names them, the error module doc among them.
  - R-14: rename `Requirements::unmet_requirements`, `missing_required`, and `unavailable` to `unmet_models`, `missing_packs`, and `unavailable_packs`, through `merge`, `notice`, `is_satisfied`, the `prepare` doc, `execute/fill.rs`, and `execute/requirements-tests.rs`.
  - R-15: replace `Step::Pending::effects`'s tuple with `pub struct IssuedEffect { pub id: EffectId, pub provenance: Provenance, pub effect: Effect }`, used for `Scheduler::issued` and by `test_support`'s drivers.
  - Update the runvil-docs passages that name the removed kinds: `runvil-docs/src/language/16-limits-and-errors.md:816, 1383` and `runvil-docs/src/language/07-substitution.md:1471`.
  - Regenerate `crates/runvil/public-api.txt` and run `cargo +<pinned nightly> xtask api --check` with `CARGO_BUILD_WARNINGS` unset.
  - Check: the build, the suite, both rustdoc gates, and `xtask api --check`.
- W19, guide (V2-1, S3-13 guide part, V5-1 guide part), in runvil-docs:
  - `src/language/15-tasks.md:1769`: "A chain has one child counter shared by the chains it calls, the tasks it spawns, the fanout arms it starts, and the internal timer of each timed wait (see [The timer behind a time limit](#the-timer-behind-a-time-limit))." `src/language/05-lua-environment.md:2060`: "calls, tasks, fanout arms, and each [timed wait](15-tasks.md#the-timer-behind-a-time-limit)'s internal timer all share that counter". `src/language/08-jump-and-call.md:1730`: "because called sections, spawned tasks, and each timed wait's internal timer share one child counter".
  - `src/language/15-tasks.md:1365`: "On the main walk, in the H1 pass, and in a section reached by `call` from either, `tasks.note` succeeds and records nothing."
  - `src/language/15-tasks.md:1313`, new last sentence: "A task counts its rounds separately from its owner, and `turns` shows that count. The round cap applies to each loop call, not to the task."
  - Check: the guide's own build, if it has one; the anchors resolve.

Decisions and constraints:

- Open Question 1 answered (a): every public change in W20 lands now, and the owner accepts the downstream breakage. Callers inside the runvil workspace still change in this commit; the Workshop clippy run in Tests confirms none was missed.
- R-11 replaces S1-9's public renames: getters named `request_timeout()` and `max_response_bytes()` would sit beside setters of the same names in `RunLimits`'s one impl and not compile. `RunLimits` is a plain `Copy` value with no invariant beyond its `NonZero` types, so public fields replace all twelve accessors and stay non-breaking to extend under `#[non_exhaustive]`, and `receive_timeout` takes the guide's name (`runvil-docs/src/language/16-limits-and-errors.md:42, 1068-1072`). `RunContext` keeps a builder, because `prepare` and `packs` fill private state, so its setters take `with_*`. Rejected: `with_*` setters on `RunLimits`, which keeps twelve methods on a plain value.
- R-14 keeps the type name `Requirements`, which the guide calls the requirements report (`runvil-docs/src/language/04-how-a-prompt-runs.md`), and renames only the three fields. Rejected: `Requirements` as one list of gaps.
- R-15 lands in this step, not as an option (Decision Record): `Step::Pending::effects` holds `IssuedEffect` values.
- S2-8 follows `890b41faa` ("Trim the public API of the Engine"): items only tests call are `#[cfg(test)]`.
- The R items come from an outline review that read no Rust source. Verify each at its site first; where the code contradicts a finding, narrow or drop it and say why in the commit message.
- Regenerate `crates/runvil/public-api.txt` with `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --bless` and include it in the commit.
- Guide edits (W20's passages and quick-reference rows, and all of W19) stay uncommitted in the runvil-docs worktree, outside this step's runvil commit. Once the runvil commit is finalized, they are committed in runvil-docs with a subject that names that runvil commit's short hash.
- runvil-docs has no build. Check instead that each new link to `15-tasks.md#the-timer-behind-a-time-limit` matches the heading "The timer behind a time limit" (`src/language/15-tasks.md:1655` at `22ff9e0`), and that the quick-reference rows stay sorted.

Pinned tests that must stay green, beside each item's Check:

- `execute/config-limits.rs:169-204` and the `harness-gateway-client` tests (R-11).
- `error/tests.rs:60-76`, which step 7 updates for S1-3's new message.

</step-6>

<step-7>

### Step 7: Restructure errors, messages, and Lua kinds [completed]

- Component: error vocabulary
- Tests: run each from the runvil root in PowerShell, as written:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
- Commit: one runvil commit, subject "Restructure the engine's errors and give model failures their own Lua kinds"; the guide edits stay outside it (see below)

Paths are relative to `crates/runvil-internal/engine/src/` unless they start with `crates/`, name a repository-root file, or start with `runvil-docs/`, the sibling repository at `c:\Users\Vinnie\cursor\runvil-docs`. Line numbers cite `9f6f8080e`; earlier steps shift them, so find each site by the item it names and treat the number as a hint. Work item labels (W1 to W28) and finding ids (S, V, R) name parts of this plan; a reference to an item outside this step names work another step owns.

Work items, applied in this order (W27 needs W21 and W22, and W28 needs W27):

- W21, author-facing messages (S4-7, S4-8, S1-3). Each message changes in the same work item as its guide quotes in runvil-docs.
  - S4-7: the `NilItem` message "{{ item }} is nil (not inside a fanout arm)" (`subst.rs:282`) is false in an arm's later sections and for `tasks.spawn` items. Change it to say no item is seeded into this section, for example "{{ item }} is nil (no item is seeded into this section)", and edit the guide quotes at `runvil-docs/src/language/07-substitution.md:808, 1291` and `runvil-docs/src/language/14-fanout.md:627`.
  - S4-8: a dotted `item` path reports "item is a string, not a table" whatever the item is. Split the `args` and `item` arms at `subst.rs:319-325`: `args` keeps "args is a string, not a table", and `item` reports that it takes no key path and renders whole. Update `subst-tests.rs:383-395` and the guide quotes at `runvil-docs/src/language/07-substitution.md:770, 1288, 1309`.
  - S1-3: `BindSchema`'s message names only the alias. Render the schema refusal in the message ("model-facing schema build failure for tool alias {alias:?}: {cause}"), and keep the refusal as a plain `cause` field behind `Error::originating_error` (W20 renames `Error::cause`, R-13), never a `#[source]`, as the error module's rule says. Update `execute/scope.rs:474-477`, `error.rs:184-196, 438-442, 451`, `error/tests.rs:60-76`, `error/tests-chain.rs:72-89`, and the guide quotes at `runvil-docs/src/language/12-tools.md:2227` and `runvil-docs/src/language/16-limits-and-errors.md:836`.
  - Check: the suite, with the updated `subst-tests.rs`, `error/tests.rs`, and `error/tests-chain.rs`; each guide quote matches its new message.
- W22, rulebook vocabulary (S5-4, S5-5 AGENTS part). The sets freeze only in `Scheduler::prepare_chat` (`execute/scheduler/chat.rs:108-160`); `models.infer` sends no tools (`execute/scheduler/dispatch.rs:225-255`); `execute/tests/rendered_tools.rs:338-368` offers a tool after an infer.
  - Reword `AGENTS.md:14` to "**Static Set**: the tools a section entry renders into the model's tool list at its first loop round, the first round a `models.loop` or a model handle's `loop` sends. It is frozen after that."
  - Reword the engine docs and test strings at `error/merge.rs:10`, `execute/scope.rs:2-3`, `error.rs:200-201`, `execute/run/effect.rs:134-135`, `execute/tests/render_effect.rs:1`, and `execute/tests/offering-sets.rs:24` to "first loop round", matching the new definition.
  - `AGENTS.md:9` (S5-5): write "a catalog tool's result" and "a file outside a prelude's install".
  - Check: `node --test crates/workshop/ui/test/docs-claims.mjs` and the suite.
- W27, error types (R-3, R-4, R-8, R-9). After W1, W21, and W22.
  - R-3: move `FrozenSet` and its `Display` (`error/merge.rs:12-19`) to `execute/scope.rs`; the error module imports it.
  - R-4: rename `error/merge.rs` to `error/sets.rs` and the module to `error::sets`, `MergeRule` to `SetRule`, `MergeTool` to `SetTool`, `Error::ToolMerge` to `Error::ToolSets`, and `merge_message` to `sets_message`. The messages stay as they are.
  - R-8: replace `Error::Lua(String)` and `Error::LuaRuntime { message, cause }` with `Error::Lua { message: String, cause: Option<BoxedSource> }`. Keep W1's new `Error::Lua` doc, but replace its sentence that points cause-bearing failures at `Error::LuaRuntime` with "`cause` holds the originating `mlua` error when there is one." Update `From<LuaError>`, `from_raised`, `Display`, `originating_error`, the Lua error rendering, `RunError::kind`, and every construction site. Fold `internal_owned` into `internal` and `internal_with_cause` (`error/convert.rs:17`), which take `impl Into<Cow<'static, str>>` and keep `#[track_caller]`.
  - R-9: replace `Error::ParseFrontmatter` and `Error::ParseStructured` with `Error::Parse(ParseError)`. `Error::parse` builds through runvil-parser's `Error::parse` and `ParseError::from_inner`, `with_prompt_name` through runvil-parser's `Error::with_prompt_context` if that stamps the name, and `RunError::location` reads `ParseError`'s `span`, `line`, `column`, and `name` (`crates/runvil-internal/parser/src/error.rs:26, 227, 289`). If those are not reachable from the Engine, expose them through `runvil_parser::detail`, as W7 does for `keyword_name`.
  - Check: the suite, `error/tests.rs`, `error/tests-chain.rs`, and both rustdoc gates; every message and parse location stays byte for byte.
- W28, Lua error kinds (R-16; Open Question 5 answered (a)). After W27.
  - First grep runvil-lua's Lua sources and shims for the kind string `internal`, and change any check there together with the rendering.
  - In the Lua error rendering (`error/value.rs`), render `Error::Completion` as kind `model_failure` and `Error::ModelRequired` as `model_required`; `internal` stays for Engine faults. `from_raised` treats both new kinds as it treats `internal` today, keeping the message as a Lua failure.
  - Update the guide's Lua kind tables: `runvil-docs/src/language/16-limits-and-errors.md:959, 1047`, and the matching rows in `05-lua-environment.md` and `17-quick-reference.md`.
  - Add the R-16 regression test from the Testing Plan.
  - Check: the suite and the docs-claims scan.

Regression test. Write it first; it must fail before W28's change and pass after:

- R-16 (W28): a section that catches a failed model round asserts `err.kind == "model_failure"`. Today the kind is `internal`, so the assertion fails.

Decisions and constraints:

- Open Question 2 answered (a): S4-7, S4-8, and S1-3 change their messages here, each together with its guide quotes.
- Open Question 3 answered (a): S5-4's rulebook and doc wording and S5-5's `AGENTS.md:9` edit land here.
- Open Question 5 answered (a): `Error::Completion` renders as `model_failure`, `Error::ModelRequired` as `model_required`, and `internal` stays for Engine faults. This breaks prompts that test `err.kind == "internal"` to catch model failures, which the owner accepted.
- S1-3 keeps the schema refusal as a plain `cause` field behind `Error::originating_error` (step 6 renamed `Error::cause`), never a `#[source]`, as the error module's rule says.
- R-4 keeps `merge_tool_sets` in `execute::scope`, which does merge the offers; only the error vocabulary named after that operation changes.
- R-8 and R-9 keep every message and parse location byte for byte; only W21's three messages change.
- Leave runvil-lua's own "first model call" wording and its instruction-quota docs alone; in runvil-lua, only W28's `internal` kind checks may change.
- The R items come from an outline review that read no Rust source. Verify each at its site first; where the code contradicts a finding, narrow or drop it and say why in the commit message.
- Guide edits (W21's quotes and W28's kind tables) stay uncommitted in the runvil-docs worktree, outside this step's runvil commit. Once the runvil commit is finalized, they are committed in runvil-docs with a subject that names that runvil commit's short hash.

Pinned tests that must stay green, beside each item's Check:

- `error/tests.rs` and `error/tests-chain.rs`, updated only for W21's messages, and every `RunError::location` assertion unchanged (R-8, R-9).
- `error/tests.rs:244-264` and `:290`.

Refreshed outline, once the step's code is complete. Nothing from it is committed:

- Build the before outline from the scope commit in a temporary worktree: `git worktree add "$env:TEMP\runvil-base" 9f6f8080e`, run the outline command below from that root with `<dir>` set to `$env:TEMP\runvil-outline-before`, then `git worktree remove "$env:TEMP\runvil-base"`.
- Build the after outline from the runvil root with `<dir>` set to `$env:TEMP\runvil-outline-after`.
- Outline command (vibelight is not on PATH; the release build is at `c:\Users\Vinnie\cursor\vibelight\target\release\vibelight.exe`): `vibelight crates/runvil-internal/engine --follow "**" --profile analysis --facts --order modules --kill-list counts --md-source-links on --members impl --md-out <dir>/outlines --html-out <dir>/html`.
- Diff the two `runvil_engine.md` files, ignoring `Source:` lines: `git diff --no-index -I '^Source:' <before file> <after file>`.
- Summarize the diff's changed declarations and doc comments in the step's return, and name any change that matches no item of the Technical Design or of this plan's work items, so the run can confirm each planned change appears and nothing else.

</step-7>

</execution-plan>
