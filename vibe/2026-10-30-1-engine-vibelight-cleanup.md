---
name: Vibelight runvil-engine
overview: "Vibelight slop scan of the runvil-engine crate: outline it, raise and verify findings against the code and docs, and leave a self-contained Cleanup Plan."
todos: []
isProject: false
---

# Vibelight: runvil-engine

<product-contract>

## Product Requirements

### Scope

- **Crate:** `runvil-engine` at `crates/runvil-internal/engine`, outline 708 KB
  - outline: `.vibelight/runvil_engine.md`
- **Outlines:** current, reused as written (index 2026-10-30 08:09 UTC)
- **Context:** 48 other outlines listed in `.vibelight/index.md`, not analyzed
- **Excluded:** 7 crates, all build tooling, cargo aliases, or a test fixture
- **Doc roots:**
  - `../runvil-docs`, the sibling guide clone, last commit 2026-10-29
  - the crate's `README.md` and `AGENTS.md`
  - the repository's `README.md` and `AGENTS.md`
  - `crates/README.md` and `crates/runvil-internal/README.md`
  - the design records in `vibe/`, including the 2026-10-26 engine plans
- **Limits:**
  - test, build, example, and doc test source adds qualified-path and lexical token hits
  - that source has no catalog records
  - method calls never fill `crates`, `tests`, or `direct_callers`
  - macro-generated code is not counted
  - dynamic trait dispatch and external consumers are settled by search
  - plan mode: nothing in the repository is built, tested, or run
  - a type's methods are cataloged under the type's module, wherever their file sits

- **Stats:** 136 candidates, 112 accepted, 19 rejected, 5 merged; 103 cheap, 9 substantial; 3 decisions, all answered
- **Work:** 28 items, 8 of them substantial; 2 findings deferred; none wait on an answer
- **Goal:** every doc and comment states the current contract once, where its owner is
- **Goal:** the type or the compiler holds each rule a doc alone holds today, where that is cheap
- **Goal:** the engine's docs and the guide agree with the code on every cited passage
- **Non-goal:** new behavior, new public API, or any change to a persisted or wire format
- **Non-goal:** other crates, except the named runvil-lua sites and the `runvil-docs` guide
- **Success:** every repository gate in the Testing Plan passes
- **Success:** a fresh plain `vibelight` run shows the edited declarations and comments

## Functional Specification

### Debt Inventory

State, types, and code

- State: internal failures from runvil-lua report a line of `src/error/convert.rs`, not their own (S1-4)
  - consequence: `RunError::location` breaks its public promise for every Lua-crate internal fault
- State: `Effect::Chat` allows `after: None` with `keep > 0`, which the docs forbid (S3-4)
  - consequence: such an effect would log a request missing its first `keep` messages
- State: `Chain::deliver` silently replaces an answer it already holds (S4-3)
  - `ChainOps::answered` counts answers that can never exceed one per view
- State: `TaskArena::take_delivery` writes `Delivered` first and restores three states by hand (S5-9)
- Duplication: `SectionContext::new` and `new_h1` copy VM construction, limits, setup, and literal (S5-4)
- Reuse: `accept_infer` takes `emitter` and `turns` apart though `ReportingHandles` bundles them (S2-12)
- Duplication: the discovery texts spell `GetDynamicTools` and `CallDynamicTool` as literals (S5-11)
- Duplication: the `pattern` schema says 256 while `PATTERN_LIMIT` lives elsewhere (S5-12)
- Duplication: `H1_GATE_FAILED` hard-codes the notice header `Requirements::notice` builds (S5-17)
- Vestiges: `advance_turn` keeps an `unwrap_or(u32::MAX)` that cannot run (S5-2)
- Vestiges: `SourceLocation` keeps four builder methods for one function in its module (S2-4)
- Vestiges: `From<ModelSetError>` for the engine error is reached only by a test (S1-24)
- Vestiges: `src/cancel.rs` is a one-item re-export no other `runvil_types` import uses (V1-2)
- Vestiges: `#[non_exhaustive]` on `ModelBindings`, `RunContext`, and `RunError` restricts nothing (S1-13, S2-5)
- Vestiges: `test_support/scripted-chat.rs` keeps a `#[path]` whose reason is gone (S8-4)
- Placement: `MAX_CALL_DEPTH` sits in the scheduler core, away from `CallDepth` (S3-14)
- Visibility: `RunContext::limits` is `pub(crate)` while its siblings are `pub(super)` (S1-14)
- Visibility: `Run::from_state` is `pub(crate)`, so in-crate code could skip the start checks (S2-6)
- Visibility: six admission items are `pub(super)` with every user in their own module (S4-6, V4-1)
- Visibility: the `cfg(test)` `recording` module is `pub`, against its module's own rule (S6-16)
- Naming: `Admission::take_back` gives slots up, where the module uses "take back" for regaining (S4-1)
- Naming: `bound_tool_set` binds nothing, by its own doc (S2-7)
- Naming: two bodies bind `chain_id: ChainId` beside `id: ChainIndex` (S3-12, S4-5)
- Naming: `prepare_effective_scope` keeps a term from the August design (S5-16)
- Naming: `SectionOffers::read` names the run's `ToolSet` `set`, a word the module uses for one set (S5-15)
- Test gap: the store error round trip checks only the tag, never the fields (S1-19)

Doc text, by file

- `src/error.rs`: wrong or restated docs at 10 sites (S1-3, S1-6, S1-7, S1-8, S1-9, S1-11, S1-22, V1-3, V5-1)
- `src/error/convert.rs`: `from_raised`, both bridge docs, and the store inverse (S1-1, S1-5, S1-19)
- `src/error/sets.rs`: four docs restate their parent (S1-10)
- `src/execute/config.rs`: the filesystem and bindings contracts stated four and three times (S1-23, V1-4)
- `src/execute/config-limits.rs`: struct and `default` docs repeat fields (S1-15, S1-16)
- `src/execute/bindings.rs`: "the fill function's only writer" reads false (S1-12)
- `src/execute/context.rs` and `section_vm.rs`: argv and ui stated on both, one copy wrong (S2-8, S5-6)
- `src/execute/context.rs`: `section_sys` documents `SectionSys` fields; a comment misnames a counter (S2-9, S2-10)
- `src/execute/run.rs`: `Step::Pending` restates its fields (S2-11)
- `src/execute/run/effect.rs`: `ToolCall` and `Vfs` docs repeat and blur their rules (S3-1, S3-2)
- `src/execute/scheduler.rs` and `chain.rs`: four docs restate or misstate (S3-6, S3-9, S3-10, S5-8)
- `src/execute/scheduler/chain_ops.rs`: `answer` says it resumes; `number_round` copies its callee (V4-3, S4-10)
- `src/execute/scheduler/admission.rs`: rules restated three times; `var` "never copied" is false (S4-7, S4-8)
- `chain_record.rs`, `kind.rs`, `anchor.rs`: id scheme and module docs restated (S4-9, S4-12)
- `dispatch.rs` and `tasks-spawn.rs`: call sites restate the child-counter rule (V4-2, S3-11)
- Root lineage: one rule restated at about ten scheduler sites (S3-8, S5-7)
- `src/execute/scheduler/chat.rs`: `ArrivedRound` says the turn counter is the chain's own (S4-11)
- `src/execute/scheduler/h1.rs`: the counters link targets the type, not the field (S5-19)
- `src/execute/section_context.rs`: four docs incomplete or restated (S5-1, S5-3, S5-21, S5-23)
- `src/execute/section_context-construct.rs`: roles and bare propagation in six docs (S5-5, S5-22)
- `src/execute/scheduler/tool_call.rs`: `dispatch_tool_calls` restates the batch rule (S5-10)
- `src/heading_address.rs`: two lookups restate rules their callees own (S6-1, S6-2)
- `src/subst.rs`: argv nil rule wrong at four sites; lookup errors incomplete (S6-9, S6-11, S6-13, S6-14)
- `src/model.rs`: a false reason, a restated Lua rule, and a Harness crate named (S6-7, S6-8, S8-3)
- `src/test_support/scripted-chat.rs`: three docs claim more than the code (S6-17, S6-18, V6-1)
- `src/lua/tests/quota.rs`: describes the deleted instruction budget (V7-1)

Documentation drift

- `RunErrorKind::RequirementsUnmet`, `Error::RequirementsUnmet`, guide ch. 4 and 16 miss reserved Pack ids (S2-1, V2-2, S7-5); docs change
- `RunErrorKind::Internal` omits caller contract breaches the code reports there (S2-2); doc changes
- `RunErrorKind::Quota` and `Error::LuaQuota` promise a deleted instruction budget (S7-4); docs change
- `RunErrorKind::Parse` and `Lua` docs misplace a block compile failure (S7-7); docs change
- `RunResult::Cancelled` omits a late re-raised `cancelled` value (V1-1); doc changes
- `Error::TaskNotOwned` gives ownership to the spawner; the root walk owns H1 tasks (V5-1); doc changes
- runvil-lua `SpawnVia` says the depth refusal names the spawn; it names the cap (S1-21); doc changes
- `infer.rs` credits the no-failure rule to the caller's abort and names Harness internals (V2-1); doc changes
- `lib.rs` and the crate README say "catalog tool call" and "every store operation" (S8-1, V8-2); docs change
- The crate README claims determinism without answer order (S8-2); doc changes
- `src/model.rs`, test comments name a Harness crate against `AGENTS.md:26` (S8-3, V8-1); comments change
- The crate `AGENTS.md` permits provider re-exports that `build-xtask` forbids (S8-5); rulebook changes
- `crates/runvil-internal/README.md` names one allowed dependent; `build-xtask` allows two (S8-6); doc changes
- Guide ch. 12 quotes the out-of-scope message with its old label (S7-1); guide changes
- Guide ch. 1 leaves `Dropped` out of the answer forms (S7-2); guide changes
- Guide ch. 4 and 16 promise `run_failed` and a parse failure for build-time refusals (S7-3, V7-4); guide changes
- Guide ch. 10 and 15 promise a per-task round cap that no code has (S7-6, V7-2); guide changes
- Guide ch. 5 omits discovery calls from when `sys.model` appears (S7-8); guide changes
- The nil-argv message names one of three causes, and the guide quotes it (S6-10); message and guide change
- `NotATable` names a case it does not cover; the guide calls the kind stable (S6-12); deferred, name kept

Report defects in the vibelight output

- A type's methods sit under the type's module, so a module-range split hands a child file's methods to its parent's call

Rejected candidates

- 19 rejected: 13 weak, 5 intentional by a design record or the guide, 1 false

</product-contract>
<implementation-contract>

## Technical Design

- Lua internal location: `runvil_lua::Error::Internal` carries a `&'static Location<'static>`
  - captured by a `#[track_caller]` `pub(crate)` constructor at each build site, read by the engine bridge
  - rejected: a doc saying bridged faults report the bridge, which only records the papercut
  - cost: every Lua-crate internal build site moves to the constructor
- Chat prefix: only the live `Effect::Chat` changes, to one `Option` of the round and the count
  - `Effect::record` flattens it, so `EffectRecord::Chat` and the run log stay as they are
  - rejected: changing the record too, a persisted format recorders store
- Double delivery: `Chain::deliver` refuses a second answer with an internal error
  - one check covers its three entry points; `answered` becomes a `bool`
- Root lineage: the rule lives once, on `Chain::lineage`
  - the `h1` module doc keeps why the pass shares the lineage and hands its tasks over
  - each other site keeps only its local consequence; the ones that still name the lineage link to `Chain::lineage`
- Admission naming: `take_back` returns to `release`, reversing part of commit `b219b85db`
  - `park` stays the reversible give-back that `wake` undoes
- Frame constructors: two private helpers in `construct` hold the VM build and the literal
  - `new` keeps reporting `SECTION_STARTED` before it calls the helper

</implementation-contract>
<verification-contract>

## Testing Plan

Run from the `runvil` root. The pinned nightly is `nightly-2026-09-05` (`crates/build-xtask/src/api/toolchain.rs:19`), and the Engine's floor is Rust 1.95 (`rust-version` in the root `Cargo.toml`).

- `cargo fmt --all --check`
- `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` with `CARGO_BUILD_WARNINGS=deny` (PowerShell: `$env:CARGO_BUILD_WARNINGS = "deny"`)
- `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
- the facade docs build: `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`
- the Harness facade docs build: `RUSTDOCFLAGS="-D warnings" cargo doc -p harness --no-deps`
- the private docs build: `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
- `cargo +nightly-2026-09-05 xtask api --check`, with `CARGO_BUILD_WARNINGS` unset (PowerShell: `Remove-Item Env:CARGO_BUILD_WARNINGS`)
- the floor build: `cargo +1.95 build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`
- `cargo test -p build-xtask`
- `node --test crates/workshop/ui/test/docs-claims.mjs`
- the guide's site build: `RUNVIL_DOCS=../runvil-docs cargo xtask site --books-only`, with mdBook 0.4.44 on `PATH` or named by `MDBOOK`
- a fresh plain `vibelight` at the repository root; the outlines in `.vibelight/` are never committed
- New test `a_second_delivery_before_resume_is_an_internal_error`, in `src/execute/tests/scheduler/failures.rs`
  - asserts `Chain::deliver` refuses a second answer to a suspended block and keeps the first
  - fails today: both deliveries return `Ok`, and the block holds the second answer
- New test `a_refused_delivery_leaves_the_entry_unchanged`, in `src/execute/scheduler/tasks-tests.rs`
  - asserts a `Cancelled`, `Abandoned`, or `Running` entry is unchanged after `take_delivery` fails
  - passes today; it pins the behavior the rewrite of `take_delivery` must keep
- Extended test `an_owned_lua_internal_message_reaches_the_engine_error_whole`, in `src/error/tests.rs`
  - asserts the location's path and line are those of the site that built the Lua error
  - fails today: the location names `src/error/convert.rs` line 177
- Extended test `nil_argv_is_an_error`, in `src/subst-tests.rs`
  - asserts the bare and dotted messages read "is nil (argv holds no value)"
  - fails today: both read "is nil (the args string is not JSON)"
- Tightened test `every_store_failure_rebuilds_from_its_raised_table_under_its_own_tag`, in `src/error/tests.rs`
  - asserts the rebuilt error equals the original for every variant whose fields round-trip
  - passes today; it catches the next field one side of the round trip drops

</verification-contract>
<decision-record>

## Decision Record

- Dropping `#[non_exhaustive]` from all-private structs is reversible: adding it back breaks no caller
- `Effect::Chat`'s public fields change; the change reverses inside the repository, because `runvil` is `publish = false` and every binding site outside the Engine names the variant with `..`
- `RunLimits::max_concurrency` keeps the clamp rule, because callers set it and cannot read private docs
- The admission module doc's copy of the clamp rule defers to `Chain::concurrency`; two more copies wait in Deferred
- The `id: ChainIndex` convention across about 45 parameters stays; only the two `chain_id` locals change
- The argv and ui value lives on `RunState`; how a VM installs it lives on `SectionVmSetup`
- Guide edits land in the `runvil-docs` clone, its own repository, under their own commit
- Assumption: `AtomicU32::update` exists at the Engine's 1.95 floor, where `try_update` already builds; the floor build settles it
- Risk: the Lua internal location touches every Lua-crate build site; the compiler finds each one
- The nil-argv message becomes `{{ path }} is nil (argv holds no value)`, with its three guide quotes (S6-10)
  - operator's answer, 2026-10-30; the old parenthetical was false for a JSON `null` and for an H1 that left `argv` nil
- `NotATable` keeps its name, so the guide's promise of stable kind names holds (S6-12)
  - operator's answer, 2026-10-30; the message beside the kind already says what happened
- The Engine Definition says "a Pack tool's result", and the root `README.md` follows (S8-1)
  - operator's answer, 2026-10-30; model-only tools also leave as `Effect::ToolCall`

### Open Questions

- None remain; the three this scan raised are answered in the Decision Record above

### Deferred and Out of Scope

- Renaming the `NotATable` substitution kind after its rule, such as `WholeOnly` (S6-12)
  - revisit when the substitution kinds next change; the guide promises the ten names are stable

- Routing a parser Lua failure to `Error::Parse` in the test-only bridge (V7-3)
  - revisit when engine production code first converts a parse error
- Capturing the parser's internal location the way the Lua crate's internal failures capture theirs (S1-4)
  - runvil-parser builds `Error::Internal` at six sites that no Engine production path converts
  - revisit with V7-3
- Moving the store error inverse into runvil-lua beside its forward table (S1-19)
  - revisit when a `VfsError` variant is next added
- runvil-types `ids.rs:4-5` and `:52-54` leave the wait shims' timers out of the child counter
  - revisit in a runvil-types scan
- `ChainStart::concurrency` (`src/execute/scheduler/chain.rs:56-59`) and `dispatch_concurrency` (`src/execute/scheduler/waits.rs:171-177`) restate the start and clamp rule `Chain::concurrency` owns
  - revisit in the next runvil-engine scan
- `dispatch_infer`'s doc (`src/execute/scheduler/dispatch.rs:193-196`) still calls the round "the single tool-free gateway round"
  - revisit in the next runvil-engine scan

</decision-record>
<project-survey>

## Project Survey

None

</project-survey>
<execution-plan>

## Execution Instructions

The letters and numbers in parentheses are tags from the analysis; nothing here needs them to act.

Paths are below `crates/runvil-internal/engine/` unless they name another crate or `runvil-docs`. Line numbers are from commit `157bc77e9`; find each site by its symbol if a line has moved. Every gate named in a Prove line is spelled out in the Testing Plan.

#### 1. Internal error docs in `src/error.rs`, and `SpawnVia` in runvil-lua

- Problem: eleven docs and one body comment on the internal error type are stale, restated, or wrong
- Module doc, `:8-9`, the sentence that begins "See the module wrappers", new text (S1-7):
  - "The `From` bridges that let internal `?` reach this type live in `convert`."
- `out_of_scope_message` body comment, `:69-73`: keep only its first sentence, which ends "a tool it cannot call"; delete the sentence that begins "The caller holds that fact", which the `has_dynamic_set` field doc (`:291-293`) already states (S1-9)
- `DepthVia` doc, `:89-95`, new text, one paragraph (V1-3):
  - "The call name [`Error::CallDepthExceeded`] reports for a nesting that passed the call-depth cap. The name is the cap's, not the author's construct: `tasks.fanout` for a fanout arm, and `call` for a `call` or a `tasks.spawn`, which shares its cap."
  - keep the variant docs at `:98` and `:100`
- `Error` doc, `:113-119`, new text (S1-8):
  - first paragraph: "The crate's internal error type, spanning parsing, model, and execution failures."
  - second paragraph: "The model variant holds the broker's closed error whole, so no HTTP client's error type leaks through the wrappers' `source()`."
- `Interrupted`, `:142-143`, new text (S1-6):
  - "The run was cancelled, the caller dropped an effect the run was waiting on, or a block raised a caught `cancelled` error value again after another suspending call."
- `TasksLive`, `:232-233`: replace the last sentence, "The message names the ids in spawn order; the Lua table lists them as `tasks`.", with (S1-22):
  - "The message names the ids in spawn order, and the Lua table holds them as one `tasks` string joined by `, `."
- `TaskNotOwned`, `:245-247`: replace the first sentence, through "the task it runs inside.", with (V5-1):
  - "Only the task's owner - its spawner, or the root walk for a task the H1 pass spawned - may wait on, inspect, or cancel it; a chain may additionally read the status of, and annotate, the task it runs inside."
  - keep the sentence that begins "An id that names no task at all"
- `RequirementsUnmet`, `:332-335`, new first paragraph; keep the second paragraph at `:337-338` (S2-1, S1-17):
  - "The environment cannot satisfy the prompt, for any cause [`RunErrorKind::RequirementsUnmet`](crate::RunErrorKind::RequirementsUnmet) lists."
- `Internal::message`, `:371-372`: "The violated invariant, as a noun phrase." (S1-11)
- `Internal::file`, `:374`, and `Internal::line`, `:376` (S1-3):
  - "The Rust source file of the construction site." and "The 1-based line of the construction site."
- `LuaQuota`, `:382-385`, new text (S7-4):
  - "A Lua quota ran out: a `log()` event or byte budget. This stable typed variant lets the caller tell quota exhaustion apart from an authoring error."
  - `QuotaResource` has only `LogEvent` and `LogByte` (`crates/runvil-internal/lua/src/error.rs:47-54`)
- runvil-lua `crates/runvil-internal/lua/src/protocol/request.rs:23-25`, the `SpawnVia` doc, new text (S1-21):
  - "Which author-facing call started a task chain. The Engine reports a depth-cap refusal under the cap's name, not the call's: a `tasks.spawn` shares the `call` cap and reports `call`, and a fanout arm reports `tasks.fanout`."
  - the Engine maps `SpawnVia::Spawn` to `DepthVia::Call` at `src/execute/scheduler/tasks-spawn.rs:79-82`
- Prove: the private docs build and the workspace docs build

#### 2. Public run error docs, and `SourceLocation` literals

- Problem: the public kind docs misstate four causes, and `SourceLocation` keeps builders only one function uses
- `src/execute/error.rs:18-19`, `RunErrorKind::Parse`, new text (S7-7):
  - "Parsing the prompt failed: invalid frontmatter YAML, a failure the parser classified as structural, or a Lua block that did not compile."
- `:40-45`, `RunErrorKind::Lua`: replace "Lua source that did not compile" with "a Pack prelude that did not load" (S7-7)
  - a prelude load failure fails the run as `Lua` (`src/execute/config.rs:272-275`, pinned by `src/execute/tests/preludes.rs:74`)
- `:47-50`, `RunErrorKind::Quota`, new text (S7-4):
  - "A Lua quota ran out: the `log()` event budget or the `log()` byte budget. Also the `call`, `tasks.spawn`, or `tasks.fanout` arm that would have nested past the call-depth cap."
- `:56`, `RunErrorKind::Internal`, new text (S2-2):
  - "An internal invariant failed, or the caller broke the `step` and `resume` contract: a `step` after `Done`, an answer of the wrong kind, an answer for an id the run never issued, or a second answer for one effect."
  - the breaches are built at `src/execute/scheduler/drive.rs:78-81` and `:108-111`, `src/execute/run.rs:209-211`, and `src/execute/scheduler/apply.rs:155-192`; `src/execute/run/tests.rs:340` and `:489` pin them as kind `Internal`
- `:58-62`, `RunErrorKind::RequirementsUnmet`, new text (S2-1):
  - "The environment falls short of what the prompt needs: a missing or unavailable required Pack, a service the caller left out, a model requirement a bound model fails to meet (a context minimum or a hard keyword), a reserved Pack id that the run's tool catalog or Pack roster names, or an H1 block that failed the prompt's hard gate."
- `:195-207`, `RunError::originating_error`, new text; keep `:209-211` (S2-3):
  - "Returns the error this failure originated from, when it keeps one: a Lua runtime failure keeps the `mlua` error, a Lua compile failure the compiler diagnostic, a schema binding failure the validation error, a tool failure the tool's own error, a store failure the store's structured error, an unmet requirement the failure it stands in for, an internal fault the failure that broke the invariant, and a parse failure the parser's cause. A model round's failure and a failed `{{ }}` substitution answer `None`: whatever cause each holds belongs to the error it wraps."
- `:100-132`: delete `SourceLocation::new`, `with_line`, `with_column`, and `with_span` (S2-4)
  - their only calls are the two arms of `RunError::location` (`:262-270`); a workspace search finds no other
  - parse arm: `SourceLocation { path: error.name().unwrap_or("<prompt>").to_owned(), line: error.line().map(NonZeroU32::get), column: error.column().map(NonZeroU32::get), span: error.span() }`
  - internal arm: `SourceLocation { path: (*file).to_owned(), line: Some(*line), column: None, span: None }`
  - the struct's `#[non_exhaustive]` (`:74`) binds only other crates, so the literals compile here
- `src/execute.rs:119-120`, `RunResult::Cancelled`, new text (V1-1):
  - "The caller cancelled the run or dropped an effect the run was waiting on and the cancelled error went uncaught, or a block raised a caught `cancelled` error value again after another suspending call."
- Prove: the facade docs build and `cargo nextest run -p runvil-engine -E 'test(location) or test(internal_faults)'`

#### 3. Bridge docs and the test-only `ModelSetError` bridge in `src/error/convert.rs`, and `src/error/sets.rs`

- Problem: the bridge docs tell history and misstate `from_raised`, four set docs restate their parent, and one `From` impl serves only a test
- `convert.rs:290-295`, `from_raised`: replace the last sentence, the one that begins "A kind whose variant needs structure", with (S1-1):
  - "A `task_not_owned` or `task_consumed` table rebuilds its variant from its `task` field, and every `cancelled` table maps to [`Error::Interrupted`]. A kind whose variant needs structure the table does not hold (the tool-scope errors, `tasks_live`, `model_failure`, `model_required`, `internal`), or a `context_exhausted` or task table missing its field, keeps its message as a Lua failure."
  - the arms are at `:299-304`, `:321`, `:326-333`, and `:338-343`
- `convert.rs:127-132`, `From<ParseError>`, new text (S1-5):
  - "Maps a parse failure onto this internal type: a frontmatter or structured failure stays a parse failure whole, and a Lua compile or internal failure maps onto its own variant. The parser crate's error type is not `#[non_exhaustive]` (the two crates version together), so this match is total."
- `convert.rs:146-150`, `From<LuaError>`, new text (S1-5):
  - "Maps the Lua crate's internal type onto this one variant for variant, and rebuilds a raised error table through [`Error::from_raised`]. The Lua crate's error type is not `#[non_exhaustive]` (the two crates version together), so this match is total."
- `sets.rs`: delete four field docs that restate their parent (S1-10)
  - `RenderedPacks::asked` (`:39`) and `answered` (`:41`); the variant doc at `:36-37` keeps "in order"
  - `SetTool::id` (`:58`) and `SetTool::set` (`:60`); the struct doc at `:54-55` says both
  - the fields are `pub(crate)`, so `missing_docs` does not fire
- Delete `From<ModelSetError>` for the engine error, which only a test reaches (S1-24)
  - production reaches the model view only through runvil-lua's `resolve_model_binding`, whose own `From<ModelSetError>` (`crates/runvil-internal/lua/src/error.rs:264-271`) converts it
  - delete the impl at `convert.rs:111-119` and its import at `:14`
  - drop `ModelSetError` from the `crate::model` re-export at `src/model.rs:43-46`; nothing else in the crate names it
  - retarget `a_poisoned_model_set_maps_to_an_internal_error_and_never_a_completion_error` (`src/error/tests.rs:308-333`): `let error = Error::from(runvil_lua::Error::from(failure));`
  - its message assertion holds: `ModelSetError`'s `Display` is "model set mutex was poisoned" (`crates/runvil-internal/model-client/src/model/options.rs:422`)
  - runvil-lua `crates/runvil-internal/lua/src/error.rs:264-266`: replace "the mapping `runvil-engine` applies too" with "the mapping the Engine receives through its `From<runvil_lua::Error>`"
- Prove: the private docs build, the clippy gate, and `cargo nextest run -p runvil-engine -E 'test(poisoned) or test(raised_table)'`

#### 4. Delete the one-item `cancel` module

- Problem: `src/cancel.rs` holds only `pub(crate) use runvil_types::cancel::CancelHandle;`, while every other `runvil_types` item is imported by its own path (V1-2)
- Delete `src/cancel.rs` and `mod cancel;` at `src/lib.rs:18`
- Replace `use crate::cancel::CancelHandle;` with `use runvil_types::cancel::CancelHandle;` at the six sites
  - `src/execute/config.rs:19`, `src/execute/context.rs:20`, `src/execute/run.rs:25`
  - tests: `src/lua/tests/coroutine.rs:9`, `src/execute/tests.rs:54`, `src/execute/tests/live_infer.rs:8`
- Prove: the clippy gate

#### 5. Check every store error's fields in its round-trip test, and name the renderer the inverse reads

- Problem: `every_store_failure_rebuilds_from_its_raised_table_under_its_own_tag` (`src/error/tests.rs:335-433`) asserts the tag and `valid_up_to` only, so a lost `anchor`, `count`, or `rule` field goes unnoticed (S1-19)
- Assert `assert_eq!(source, failure, "{failure:?}")` for the variants whose fields all round-trip; `VfsError` derives `PartialEq` (`crates/runvil-internal/vfs/src/error.rs:21`)
  - the path-only `NotFound`, `AlreadyExists`, `NotADirectory`, `IsADirectory`, `DirectoryNotEmpty`, `ScopeEnded`, `ReadOnly`, and `RootProtected`
  - and `NotUtf8`, `InvalidPath`, `InvalidRange`, `Anchor`, and `NoStore`
- Keep the tag-only check for the variants whose text fields degrade
  - `PermissionDenied`, `ApprovalRequired`, `OutsideRoot`, `CrossMount`, `Io`, `Unsupported`, `Conflict`, `Backend`
  - keep the `Io` assertion that the rebuilt kind is `Other` with no OS code
- It passes today: `store_error_fields` (`crates/runvil-internal/lua/src/error-value-store.rs:8-55`) writes every field `raised_store_error` (`src/error/convert.rs:206-287`) reads back; it catches the next field one side drops
- `convert.rs:200-205`, the `raised_store_error` doc: replace "this crate's own [`ErrorValue`](runvil_lua::ErrorValue) rendering" with "[`store_error_value_fields`](runvil_lua::store_error_value_fields)" (S1-19)
  - both the Engine's rendering (`src/error/value.rs:75`) and the Lua crate's direct store closures build their tables through it
- Prove: `cargo nextest run -p runvil-engine -E 'test(every_store_failure)'` and the private docs build

#### 6. Lua-crate internal failures keep their construction site

- Problem: `RunError::location` (`src/execute/error.rs:256-257`) promises the broken invariant's Rust file and line for kind `Internal` (S1-4)
  - `runvil_lua::Error::Internal` holds only its message (`crates/runvil-internal/lua/src/error.rs:170`), and the bridge rebuilds it with `Error::internal(message)` at `src/error/convert.rs:177`, so every Lua-crate internal fault reports that line
- Test first: extend `an_owned_lua_internal_message_reaches_the_engine_error_whole` (`src/error/tests.rs:288-306`)
  - assert that `crate::RunError::from(error).location()` has `path == file!()` and `line` equal to the line of the test's own `std::panic::Location::caller()` call, captured the way `internal_faults_report_the_rust_file_and_line` (`:199-211`) captures its line
  - it fails today: the location names `src/error/convert.rs` line 177
  - today the test builds the error as `LuaError::Internal(Cow::Owned(..))`; after the change it builds `LuaError::Internal { message, location: std::panic::Location::caller() }`
- runvil-lua `crates/runvil-internal/lua/src/error.rs:164-170`: the variant becomes `Internal { message: Cow<'static, str>, location: &'static std::panic::Location<'static> }`, its `#[error]` reads `{message}`, and its doc adds that `location` is the construction site
- Add `#[track_caller] pub(crate) fn internal(message: impl Into<Cow<'static, str>>) -> Error` beside the other constructors (`:201-237`), capturing `std::panic::Location::caller()`
  - route every production build site through it, 25 in all: `vm/state.rs` (4), `var.rs` (4), `tools.rs` (2), `prose.rs` (2), `require.rs` (2), `vm/run.rs` (2), and one each in `packs.rs`, `vm/install.rs`, `models_loop.rs`, `globals.rs`, `vm/adhoc.rs`, `scope.rs`, `sys.rs`, `error-value.rs:377`, and `From<ModelSetError>` at `error.rs:267-270`
  - and the test support site `test_support.rs:234`
  - the patterns at `error.rs:252` and `error-value.rs:253`, `:275` become `Error::Internal { .. }`; the tests at `models-tests.rs:423` and `tests/section_vm.rs:115`, `:120` match the `message` field
- Engine: add a `pub(crate)` constructor that takes a given location, such as `Error::internal_at(message, location)`, in `src/error/convert.rs` beside `internal` (`:35-47`)
  - call it from the `LuaError::Internal { message, location }` arm at `convert.rs:177`
  - leave the parser arm at `:141`, which no production path reaches
  - `Error::Internal`'s doc (`src/error.rs:365-368`) adds that a Lua-crate internal failure brings its own captured site
- Prove: `cargo nextest run -p runvil-lua -p runvil-engine`, the clippy gate, and the private docs build

#### 7. Drop `#[non_exhaustive]` from three public structs whose fields are all private

- Problem: outside crates already cannot build or destructure these structs, so the attribute restricts nothing (S1-13, S2-5)
- Delete it at `src/execute/bindings.rs:16` (`ModelBindings`), `src/execute/config.rs:46` (`RunContext`), and `src/execute/error.rs:145` (`RunError`)
  - `RunLimits` (`src/execute/config-limits.rs:16`) and `SourceLocation` (`src/execute/error.rs:74`) keep theirs, as their fields are public
- Regenerate `crates/runvil/public-api.txt` with `cargo +nightly-2026-09-05 xtask api --bless`, `CARGO_BUILD_WARNINGS` unset; lines 42, 43, and 67 lose the attribute
- Adding the attribute back to an all-private struct breaks no caller, so the change reverses cleanly
- Prove: `cargo +nightly-2026-09-05 xtask api --check` and the facade docs build

#### 8. Run configuration docs and the limits field

- `src/execute/config.rs:65`: `pub(crate) limits: RunLimits` becomes `pub(super) limits: RunLimits` (S1-14)
  - doc, the struct's only undocumented field: "The run's resource limits; set by `with_limits`."
  - its readers are the builder (`:145`), the `Debug` impl (`:401`), and the destructure in `RunState::new` (`src/execute/context.rs:130-145`), all inside `execute`
- Filesystem contract: keep it on `with_vfs` (`:190-201`), dropping its sentence "[`prepare`](RunContext::prepare) keeps a handle set here as given." at `:196-197` (S1-23)
  - `vfs()` doc, `:325-330`: "Returns the run's VFS handle, set with [`with_vfs`](RunContext::with_vfs)."
  - `prepare` doc, `:287-290`: replace the sentence "`prepare` uses the context's filesystem as given, because ... the declared store." with "`prepare` leaves the filesystem as given."
- Model bindings (V1-4):
  - field doc, `:70-73`: "The run's model bindings; filled by [`prepare`](RunContext::prepare)."
  - getter doc, `:336-340`: "Returns the run's [`ModelBindings`], which [`prepare`](RunContext::prepare) fills."
- `src/execute/bindings.rs:25-26`, the `bind` doc: delete its second sentence, "The fill function's only writer." (S1-12)
- `src/execute/config-limits.rs:5-14`, the `RunLimits` struct doc, new text (S1-15):
  - "Resource limits for one run."
  - "The defaults are built-in constants and are safe to use as they are. A caller changes one by setting its field: `let mut limits = RunLimits::default(); limits.max_concurrency = n;`."
- `default` doc, `:43-45`, new text (S1-15):
  - "Returns limits with the default values: 24 tool iterations, 8 concurrent tasks, 64 MiB of Lua memory, and 1024 Lua log events."
  - `run_limits_pins_all_four_defaults` (`:60-67`) pins the four numbers
- `max_concurrency` doc, `:25-30`, new text (S1-16):
  - "The run's concurrency ceiling: the most tasks the scheduler admits at once across the whole run."
  - "A chain's `tasks.concurrency` call sets its own limit, clamped to its enclosing chain's limit, and this ceiling is the root's. Each spawned task counts against its owner's limit and every ancestor's, so limits nest: a fanout started inside one arm of another fanout runs within that arm's remaining share."
  - the clamp is `dispatch_concurrency` at `src/execute/scheduler/waits.rs:171-200`
- Prove: the facade docs build, the private docs build, and `cargo nextest run -p runvil-engine -E 'test(run_limits)'`

#### 9. `RunState` and section VM docs, and `run_tool_set`

- Problem: `bound_tool_set` binds nothing, by its own doc (S2-7)
  - rename it `run_tool_set` at `src/execute/context-bound.rs:23` and its one call at `src/execute/context.rs:146`; keep `bound_model_set`
  - module doc, `context-bound.rs:1-2`, new first sentence: "The run-scoped tool and model sets and the `argv` derivation: the pieces `RunState::new` assembles once per run."
  - keep the "Nothing is bound here" sentence in the function's doc
- Problem: argv and ui are described on both `RunState` and `SectionVmSetup`; one copy is wrong (S2-8, S5-6)
  - rule: what the value is lives on `RunState`; how a VM installs it lives on `SectionVmSetup`
  - `bound::derive_argv` (`context-bound.rs:82-96`) owns the nil rule: a default-declared prompt always gets a value, and a structured prompt gets `None` when its args fail to parse or parse to JSON `null`
- `src/execute/context.rs:70-73`, the `argv` field doc, new text:
  - "The run's `argv`, as `bound::derive_argv` derives it from `args`. At construction this is the value the H1 pass starts from; the walk's fork holds the value H1 left behind at the freeze, so an H1 repair reaches every downstream section."
- `:236-238`, the `argv` method doc: "The run's `argv`, as `bound::derive_argv` derives it; on the walk, the value H1 left behind at the freeze."
- `:104-106`, the `ui` field doc: "The run's application-state snapshot, when the caller supplied one."
- `:319-320`, the `with_args` doc: replace "so the chain sees the parsed form of what it was passed" with "so the chain sees the `argv` of what it was passed"
- `src/execute/section_vm.rs:54`: replace "(the parsed form of `args`; `None` installs nil)" with "(`RunState::argv`; `None` installs nil)"; keep the mode sentences at `:55-58` and `SectionVmSetup::ui` as written
- The tool-loop cap opens alike on both types
  - keep `RunState::max_tool_iterations` (`context.rs:289-291`), which says what the value is
  - `section_vm.rs:76-77`, `SectionVmSetup::max_tool_iterations`, new text: "The run's tool-loop cap, as `RunState::max_tool_iterations` resolves it, captured by the `models.loop` shim as its round cap."
- The H1-only writable `argv` is stated on `SectionVmSetup::argv` (`section_vm.rs:55-58`), which keeps it
  - `context.rs:350-353`, the second paragraph of `RunState::vm_setup`, new text: "`argv_writable` picks the [`Argv`] mode that [`SectionVmSetup::argv`] describes; the H1 pass is the only production caller that passes `true`."
  - `src/execute/section_context-construct.rs:190-192`, the comment above `new_h1`'s `ctx.vm_setup(.., true)` call, new text: "// `argv` installs writable, the H1 delta `SectionVmSetup::argv` describes; the executor reads the value back at the freeze (see the scheduler's H1-to-walk handoff)."
  - keep the `new_h1` doc's delta list as written; it is the one list of H1's deltas
- Problem: `section_sys` documents `SectionSys`'s fields on the constructor (S2-9)
  - delete the second paragraph of its doc, `context.rs:388-395`, with the blank doc line above it
  - `src/execute/section_context.rs:56`, `SectionSys::id`: "The section entry's hierarchical id: the entering chain's id extended by its local entry counter, rendered as a dot-separated path."
  - `:58-59`, `SectionSys::taskid`: "The nearest enclosing task: the main walk is task `0`, and a `call` child reports its caller's task. A chain passes it to `tasks.*` to speak about itself."
- `src/execute/context.rs:149-151`, the comment above `EventSink::seeded`, new text (S2-10):
  - "// The root task's event sequence starts at the context's `provenance_start`: past any parse events the caller put in the stream ahead of the run, or at zero."
- Prove: the private docs build and the clippy gate

#### 10. `Run::from_state` and `Step::Pending`

- `src/execute/run.rs:180-190`: drop `pub(crate)` from `from_state` (S2-6)
  - its only callers are `Run::new` (`:152`) and the `cfg(test)` `new_for_test` (`:172`), both after `prepare_state`; the suites call `new_for_test`
  - doc: "Builds the live run over a state that passed `prepare_state`."
- `:42-61`, `Step::Pending`: the variant doc restates its fields (S2-11)
  - variant doc: "The run continues."
  - `effects`: "The effects this step issued, in issue order, each with the provenance of the task that built it, for the caller to perform and answer through [`Run::resume`]. An empty list means every chain is waiting on an effect that an earlier step issued."
  - `events`: "The events the step reported, in order."
  - `withdrawn`: keep the text, and add "It may name a call that `effects` lists, because one step can issue a call and then orphan it."
- Prove: the facade docs build and the private docs build

#### 11. `accept_infer` takes the chain's reporting handles

- Problem: `accept_infer` and `accept_infer_completion` take `emitter` and `turns` as two parameters, and nothing ties a task's emitter to its own counter (S2-12)
- Replace both parameters with `reporting: &ReportingHandles` in `accept_infer_completion` (`src/execute/infer.rs:31-37`) and `accept_infer` (`:85-91`), reading `&reporting.emitter` and `&reporting.turns`
  - `accept_infer` passes it on at `:99`
  - import `super::context::ReportingHandles`, whose fields are `pub(super)` in `execute::context`; drop the now-unused `AtomicU32` and `Emitter` imports at `:15` and `:17`
  - the caller `ChainOps::accept_infer` (`src/execute/scheduler/apply.rs:250-256`) passes `chain.ctx.reporting()`
  - `src/execute/infer-tests.rs:17-23` passes `&ReportingHandles { emitter: Arc::new(null_emitter()), turns: Arc::new(AtomicU32::new(0)) }`, importing `std::sync::Arc` and `std::sync::atomic::AtomicU32`
- Problem: the docs credit the no-failure rule to the caller's abort and name Harness internals (V2-1)
  - `infer.rs:3`: replace "gateway round" with "model round"
  - `infer.rs:72-79`, first paragraph: "Applies one infer round's answer, the completion the caller obtained or its failure: it reports the round exactly like a prose round, through `turn::report_model_turn` on the chain's reporting handles under `section`, and renders its text."
  - second paragraph: "A failed completion is a failed turn and the call's error. A round the caller drops never reaches here: the drop resumes the chain with a cancellation, so no `MODEL_TURN_FAILED` fires for it."
  - the drop path: `apply.rs:131` maps `EffectAnswer::Dropped` to `Error::Interrupted` before the infer arm, pinned by `src/execute/tests/live_infer.rs:202-237`
- Prove: `cargo nextest run -p runvil-engine -E 'test(infer)'` and the clippy gate

#### 12. Effect docs in `src/execute/run/effect.rs`

- `:151-158`, `Effect::ToolCall`, new text (S3-1):
  - "One call to a Pack's tool: a run catalog tool, or a tool only the model sees, from the section entry's render. The caller resolves `tool` to an implementation. The effect's record keeps every field but `access`, a live handle, and `survives_stop`, which the run catalog and the render's answer already give."
  - `Effect::record` (`:276-287`) keeps `tool`, `alias`, `args`, and `origin`
- `:182-195`, `Effect::Vfs` (S3-2):
  - delete "Only the `store.*` calls produce this effect." at `:187-188`; the first sentence already names the eight `store.*` calls
  - replace "Correctness holds whenever the caller drops the access." at `:192-193` with "When the caller drops the access does not affect the run's correctness."
- Prove: the facade docs build

#### 13. `Effect::Chat` holds the shared prefix as one `Option`

- Problem: `after: Option<RoundId>` and `keep: u64` allow `(None, k)` with `k > 0`, which the docs forbid (S3-4)
  - in `src/execute/run/effect.rs:117-123`; `Effect::record` skips the first `keep` messages (`:251`, `:257`), so such an effect would log a request missing them
- Change the live effect only: `after: Option<(RoundId, u64)>`, the earlier round and its kept count; delete the `keep` field
  - new `after` doc: "The earlier round whose request this one extends, the previous send of the same section's `messages` list, with how many leading messages this request shares with that round's request. `None` on a section's first send and on a nested `models.infer` round."
- runvil-lua `crates/runvil-internal/lua/src/messages.rs:193-208`: `MessageList::commit` returns `Option<(RoundId, u64)>`, `None` on the first send and `Some((after, keep))` after it
  - its doc's "`(None, 0)` on the first" becomes "`None` on the first"
  - its tests: every `commit` assertion in `messages-tests-list-commit.rs`, and `compactors-tests.rs:177-181`
- `Effect::record` (`effect.rs:241-266`) flattens it into the unchanged `EffectRecord::Chat { after, keep }`; `None` records `after: None, keep: 0`, so the run log format stays the same
- Callers: `src/execute/scheduler/chat.rs:178-185` (`let after = list.commit(..)`) and `src/execute/scheduler/dispatch.rs:221-228` (`after: None`, no `keep`)
- Tests that build the effect: `chat_effect` in `src/execute/run/effect-tests.rs:29-37` takes the `Option` in place of `after` and `keep`, with its callers passing such as `Some((RoundId::new(2), 2))`; and `src/execute/run/tests.rs:47-52`
  - the record assertions at `effect-tests.rs:52-67`, `:95-106`, and `:182-193` stay as written
- Every binding site outside the Engine names the variant with `..`: harness-runner `effect_loop.rs:339-345`, harness-gateway-client `wire/request-tests.rs:81-85`, the facade's tests and example, and the bench (`benches/models_loop.rs:96-100`)
- Regenerate `crates/runvil/public-api.txt` with `cargo +nightly-2026-09-05 xtask api --bless`; line 875 changes type and line 877 goes
- Prove: `cargo nextest run -p runvil-engine -p runvil-lua -p harness-runner -p harness-gateway-client`, `cargo +nightly-2026-09-05 xtask api --check`, and the facade docs build

#### 14. Scheduler core docs, and `MAX_CALL_DEPTH` beside `CallDepth`

- `src/execute/scheduler.rs:232-233`, `decided`, new text (S3-10):
  - "Whether the run's outcome is decided: its end boundary is reported, so at most the orphans' answers stand between the run and `Done`."
  - the body also matches `Phase::Done`, and `src/execute/run/tests.rs:450` asserts a finished run stays decided
- `:238-239`, the first paragraph of `answer_inline`, new text; keep `# Errors` (S3-9):
  - "Answers `id` at once with `answer` and queues it to resume: the inline-answer path of every request the core keeps, and of a chain end answering another chain."
- `:168-174`, the `tasks` field doc, new text (S5-8, S3-7):
  - "The task arena; [`TaskArena`] and the `tasks` module state its lifecycle."
- Move `MAX_CALL_DEPTH` and its doc (`:109-111`) into `scheduler/chain_record.rs`, just above `CallDepth` (`:63-67`) (S3-14)
  - keep the name and the private visibility; drop `use super::MAX_CALL_DEPTH` at `chain_record.rs:24`
  - its only code use is `CallDepth::deeper` (`chain_record.rs:78-87`); test files name it only in comments
- `src/execute/scheduler/chain.rs:287-296`, `abort_effect`, new text (S3-6):
  - "Orphans one in-flight leaf effect whose chain is going away, as [`EffectTable::orphan`](super::effects::EffectTable::orphan) describes, and ends a tool call's identity through [`Scheduler::end_tool_call`] as its answer would, so what the tool already did is joined into the parked chain and anything it tries later is refused."
  - `chain.rs` imports `Scheduler` but not `EffectTable`, so that link names its path
- Prove: the private docs build and the clippy gate

#### 15. A chain refuses a second answer before it resumes

- Problem: `Chain::deliver` (`src/execute/scheduler/chain_record.rs:426-438`) overwrites an answer it holds (S4-3)
  - its three entry points are `ChainOps::answer` (`chain_ops.rs:79-83`), `Scheduler::answer_inline` (`scheduler.rs:244-248`), and `Scheduler::wake_from_wait` (`admission.rs:410`)
- Test first: `a_second_delivery_before_resume_is_an_internal_error` in `src/execute/tests/scheduler/failures.rs`, beside `an_answer_for_a_chain_with_no_suspended_block_fails_loudly` (`:427-448`)
  - `Rig::start` a `scheduler_scenario` over a prompt with a `# Title` and one section whose block runs `return models.infer('ask')`, holding the round with `.hold(|effect| matches!(effect, Effect::Chat { .. }))`, then `settle()`, so the root walk's block is suspended on the held round; the cancellation test at `:244-265` holds its rounds the same way
  - add a hook in `src/execute/scheduler/test_hooks.rs` beside `answer_unsuspended_for_test`, such as `deliver_twice_for_test`: it delivers `Ok(Answer::Note)` and then `Ok(Answer::Concurrency(1))` to the chain at arena index 0, and returns both results with whether the block still holds `Answer::Note`
  - assert the second result is `Err(Error::Internal { .. })` and that the block holds the first answer
  - it fails today: both deliveries return `Ok`, and the block holds the second answer
  - `Chain::for_test` enters no section, so the test needs a real run
- `Chain::deliver`: when `running.answer` is already `Some`, return `Err(Error::internal("a suspended block holds at most one delivered answer"))` and leave the first in place
- `src/execute/scheduler/chain_ops.rs:35-36`: `answered: usize` becomes `answered: bool`
  - `answer` (`:79-83`) sets it; `with_chain` (`:56-58`) pushes the chain once when it is set
  - field doc, `:35`: "Whether the view delivered an answer to the chain, which [`Scheduler::with_chain`] requeues once."
  - `with_chain` doc, `:40-43`: "Runs `f` in the view of chain `id`, then pushes the chain onto the ready queue if the view delivered an answer. The view pushes nothing else, so no other chain enters the queue between an answer and its requeue."
- `chain_ops.rs:72-73`, the first sentence of `answer`, new text; keep the rest (V4-3):
  - "Delivers `answer` to the chain's suspended block: the inline-answer path of every request in the view."
- `chain_ops.rs:93-94`, `number_round`, new text (S4-10):
  - "Numbers the model round about to be issued, through [`EffectTable::number_round`]."
- Prove: `cargo nextest run -p runvil-engine -E 'test(delivery) or test(scheduler)'` and the private docs build

#### 16. Admission: one word per slot change, narrower items, and true docs

- Problem: `Admission::take_back` gives slots up, but the module says "take back" for regaining them (S4-1)
  - rename it `release`: `src/execute/scheduler/admission.rs:265`, `src/execute/scheduler/chain.rs:214`
  - tests: `admission-tests.rs:157`, `:181`, `:215`
  - `park` stays the reversible give-back that `wake` undoes; no scheduler item is named `release` today
  - doc, `admission.rs:262-264`: "Releases `id`'s slots at its chain end, so a queued sibling or a resumption can have them. A chain that holds none, or whose slots are given back while it is parked, changes nothing."
  - `drop_aborted` doc, `:269-272`: replace "and the slots it still holds are given back" with "and the slots it still holds are released"
  - caller comment, `chain.rs:211-213`: "A task chain that was holding admission slots releases them at its end, before its owner is woken: the owner's re-acquire and any queued sibling need the freed capacity."
  - abort comment, `chain.rs:276-277`: "a running task's held slots are given back" becomes "a running task's held slots are released"
  - the commit message says it reverses part of `b219b85db`, whose own rationale described giving back
- Drop `pub(super)` from six items whose users all sit in `admission.rs` or its test child (S4-6, V4-1)
  - `ChainTree::kind` (`:75`), `ChainTree::call_parent` (`:85`), `ChainTree::ancestors` (`:103`)
  - `Admission::plan` (`:166`), `Admission::wake` (`:250`), and `enum Admit` (`:51`)
  - `ChainTree::enclosing` and `ChainTree::limit` stay `pub(super)`; `waits.rs:188-189` uses them
- `SpawnRecord` doc, `:34-38`, new text (S4-7):
  - "A queued task chain's spawn record, held from spawn until its admission: the input override and the spawning section the start event reports. The `var` snapshot is not in the record: it moves out of the yield into the chain's `var` slot at spawn, the start event reports a copy of it at admission, and the chain's first section entry seeds from the slot."
  - the copy is `let var = chain.var.clone();` at `:369`; the first entry seeds at `src/execute/scheduler/walk.rs:123-131`
- The module doc, `Admission`, and `wake_from_wait` restate each rule (S4-8)
  - module doc, `:8-11`: replace the sentences from "The root's limit is the run's" through "clamped to the parent's." with "Each chain's limit is its [`Chain::concurrency`](super::Chain::concurrency)."
  - `Admission` doc, second paragraph (`:111-115`): delete its first three sentences and keep "Slot counts are computed from the holders, never stored, so no count can drift from them."
  - `wake_from_wait` doc, first paragraph (`:396-400`), new text; keep `# Errors`: "Delivers its answer to a chain parked on a task wait: the chain is parked on [`Parked::Queued`] when [`Admission::wake`] requeues it for its slots, else pushed onto the ready queue at once."
- Prove: `cargo nextest run -p runvil-engine -E 'test(admission)'` and the private docs build

#### 17. Chain ids, lineage, and the root-lineage rule

- `src/execute/scheduler/chain_record.rs:253-258`, `Chain::lineage`, new text, the rule's one home (S4-9, S3-8, S5-7):
  - "The chain's hierarchical [`ChainId`], allocated under the scheme [`runvil_types::ids`] states: every id the chain hands out extends it. The H1 pass and the walk after it are two arena records under the root lineage `0`."
- The `chain_record` module doc, `:1-5`: end it at "indexed by [`ChainIndex`]." and delete the root-lineage clause that follows
- At each other site, keep only the local consequence (S3-8, S5-7)
  - `scheduler/h1.rs:9-15`: keep as written; it is the one doc that says why the pass shares the lineage and hands its tasks to the walk
  - `h1.rs:47-50`, comment: "// The pass's one frame takes the root lineage's entry 0, and the walk that follows continues its counters (see `Chain::lineage`). Its admission limit is the run's ceiling."
  - `h1.rs:93-98`, comment: "// The walk continues the pass's counters (see `Chain::counters` for the normative statement): the pass took entry 0, the first walked section takes entry 1, and a child the pass started keeps its index. Its admission limit continues too: the pass may have lowered it with `tasks.concurrency`."
  - `scheduler/walk.rs:8-10`: replace "sharing the root lineage `0` as a second arena record" with "under the same root lineage ([`Chain::lineage`](super::Chain::lineage))"
  - `scheduler/waits.rs:16-18`: delete ", since the pass and the walk share the root lineage `0` as two arena records"; the owner rule before it stays
  - `scheduler/task_end.rs:11-13`: replace the sentence "The H1 pass and the walk after it share ... to the walk." with "The H1 hand-off reassigns the pass's tasks to the walk (see the `h1` module)."
  - `task_end.rs:207-211`, `reassign_tasks`: delete ", where the pass and the walk share the root lineage `0` and are two arena records"
  - `scheduler/tasks.rs:114-116`, `TaskEntry::owner`: delete ", since the pass and the walk share the root lineage `0` as two arena records"
  - `chain.rs:100-102` stays; it says what makes a chain a root chain
- `next_child_id` (`chain_record.rs:369-372`), new first paragraph: "Hands out the chain's next child id, under [`Counters::next_child`]."
- `next_entry_id` (`:385-387`), new first paragraph: "Hands out the chain's next section-entry id, under [`Counters::next_entry`]."
  - keep both `# Errors` sections
- `scheduler/dispatch.rs:173-176`, comment: "// The child's id is the caller's next child id." (V4-2)
- `scheduler/tasks-spawn.rs:127-130`, comment: "// The task's id is the spawner's next child id; the task is its chain, named from the other side." (V4-2)
- `tasks-spawn.rs:87-91`, comment: cut its last sentence and end the one before it as "otherwise the chain inherits the spawner's context whole, but for the turn counter, which `with_task` replaces below." (S3-11)
  - keep the comment at `:133-135`, above the `with_task` call
- Rename the `chain_id: ChainId` locals to `lineage`, with the `ChainStart { lineage, .. }` shorthand (S3-12, S4-5)
  - in `prepare_call` (`dispatch.rs:177-179`) and `prepare_spawn` (`tasks-spawn.rs:131-138`, where `TaskId::from(chain_id.clone())` becomes `TaskId::from(lineage.clone())`)
  - the `id: ChainIndex` parameters across the scheduler stay as they are
- `scheduler/h1.rs:17`: the link target becomes `super::Chain::counters`, the field that holds the normative statement (`chain_record.rs:260-264`) (S5-19)
- `chain_record/kind.rs` and `chain_record/anchor.rs`: the type's doc carries the description, and the module doc names the type (S4-12)
  - `kind.rs:7`: "What kind of chain a record is: the main walk, the H1 pass, a `call` child, or a task's backing chain, each with the links only that kind has."
  - `kind.rs:1-3`: "The chain record's [`ChainKind`]."
  - `anchor.rs:1-2`: "The chain record's [`ChatAnchor`]."
- `scheduler/chat.rs:325-328`, `ArrivedRound`, new text (S4-11):
  - "One arrived round's reporting context: the round's id, the chain's section label, and its task's reporting handles."
  - a `call` child shares its caller's counter (`dispatch.rs:159-162`), so the old "a task chain's own" is false for root and `call` chains
- Prove: the private docs build and the clippy gate

#### 18. A refused task delivery never writes the entry

- Problem: `TaskArena::take_delivery` (`src/execute/scheduler/tasks.rs:277-302`) writes `Delivered` first (S5-9)
  - then three arms write the old state back by hand (`:286`, `:290`, `:296`); a missed restore reports a delivery that never happened
- Test first: `a_refused_delivery_leaves_the_entry_unchanged` in `src/execute/scheduler/tasks-tests.rs`
  - for each of `Cancelled`, `Abandoned`, and `Running`: `take_delivery` fails and `state_of` is unchanged
  - build the states with the `spawn` helper (`:37`) and the stop calls `stopping_a_running_task_ends_it_and_returns_its_backing_and_target` (`:52-66`) makes
  - it passes today and pins the behavior the rewrite must keep; `:86-107` and `:121-141` already pin the `Done` and `Delivered` arms
- Match on `&mut entry.state` instead
  - the `Done(outcome)` arm takes it with `std::mem::replace(outcome, Ok(String::new()))`
  - it then sets `entry.state = EntryState::Delivered { ok: outcome.is_ok() }` and returns the outcome
  - every other arm returns its error without writing; delete the comment at `:282`
- Prove: `cargo nextest run -p runvil-engine -E 'test(delivery)'`

#### 19. The two section frame constructors share one VM setup

- Problem: `new` (`src/execute/section_context-construct.rs:82-96`, `:124-135`) and `new_h1` (`:167-180`, `:195-206`) copy each other (S5-4)
  - VM construction, cancel, limits, setup, and a seven-field literal that differs only in `item`; the comments already differ, since only `new` explains `set_cancel` (`:89`)
- Add a private `fn limited_vm(ctx: &RunState, name: &str) -> Result<SectionVm<Uninjected>>`
  - it holds `SectionVm::new_for_section`, `set_cancel`, and `apply_lua_limits` under one pair of comments: the cancel comment from `:89` and the bare-propagation comment from `:91-92`
  - add `Uninjected` to the module's `crate::lua` import (`:20`); `setup_section_vm` takes that type (`src/execute/section_vm.rs:120`)
- Add a private constructor for the literal, such as `fn assemble(vm: SectionVm, sys: SectionSys, item: Option<serde_json::Value>, ctx: &RunState) -> Self`
- Both constructors call the two helpers; `new` still reports `SECTION_STARTED` (`:80-81`) before `limited_vm`
- Bare propagation is stated once, in `section_vm.rs:15-20`, which says why (S5-5)
  - `new`'s `# Errors` (`:57-62`): "Returns [`Error::Internal`] when `pos` is past its slice, and otherwise the [`Error`] of whichever step failed (see [`section_vm`] for which failures tear the VM down)."
  - `new_h1`'s `# Errors` (`:154-158`): "Returns the [`Error`] of whichever step failed (see [`section_vm`] for which failures tear the VM down)."
  - the [`section_vm`] link resolves through `use crate::execute::section_vm::{self, VmSeed}` (`:17`)
  - `section_vm.rs:107-109`: delete the sentence "The caller applies the Lua limits itself before calling, so a limits failure propagates without touching the VM's teardown observation path."
  - `section_vm.rs:4-6`: replace "Both frame constructors - `SectionContext::new` for a walked section (a spawned task's first entry included) and `SectionContext::new_h1` for the H1 pass - run" with "Both frame constructors run"
  - the `construct` module doc (`:1-8`) keeps the arrival list
- `new_h1` doc, `:146-147`, new opening (S5-22):
  - "H1's only deltas from a walked section: no `SECTION_STARTED` observation, and no `SECTION_FINISHED` since the scheduler never arms the pass's completion (the pass is not a walked section), an empty `var` seed ..." with the rest unchanged
  - the scheduler never calls `mark_completed` for the pass (`src/execute/scheduler/h1.rs:63-66`)
- Prove: `cargo nextest run -p runvil-engine -E 'test(h1) or test(observations)'` and the private docs build

#### 20. Section frame docs in `src/execute/section_context.rs`

- `:156-159`, `read_var`'s `# Errors`, new text (S5-1):
  - "Returns [`Error::Lua`] when the section reassigned the `var` global, which the walk relies on to fail the run, or when `var` cannot convert back to JSON (the write guard keeps that second case from happening in practice)."
  - runvil-lua raises the reassignment as `Error::Lua` (`crates/runvil-internal/lua/src/var.rs:208-219`); `src/execute/tests/scheduler/h1/pass.rs:150-173` pins it
- `live_vm` (`:273-276`) says its invariant is stated once; four other docs state it (S5-3)
  - `vm` field, `:124-126`: "The frame's VM; [`live_vm`] states when the slot holds it."
  - `vm()`, `:184-185`: replace the "Infallible: ..." paragraph with "Infallible; [`live_vm`] states why."
  - `completed`, `:128-130`: "Whether [`SectionContext::mark_completed`] armed the frame's `SECTION_FINISHED`."
  - `Drop` comment, `:341-346`: keep the first sentence, through "tears down exactly once here.", then "// It uses `let-else` rather than `live_vm`, so the destructor never panics."
  - keep the `live_vm` doc, its lint reason, its `expect` message, and the `mark_completed` doc
- Module doc, `:4-5`, new text (S5-21):
  - "It owns the section VM plus the state the block walk reads and writes - the section's [`SectionSys`], a spawned chain's `item`, the tool-call counts, the stack of open [`AdhocCall`]s, and the completion flag its `Drop` reads -"
- Module doc, `:10-16`, new text; the `construct` module doc keeps the arrival list (S5-5):
  - "Each section entry is one construct-run-teardown cycle: a constructor in the `construct` child builds the frame, the scheduler's chain steps run the blocks, and the frame's [`Drop`] impl is the single teardown boundary."
- `install_lazy_prose`, `:197-201`, new first paragraph; keep `# Errors` (S5-23):
  - "Installs the scheduler's pending Markdown buffer, empty when no Markdown accumulated, as the VM's lazy `prose` template before one Lua coroutine starts, rendered through this frame's [`prose_renderer`]; [`SectionVm::install_lazy_prose`] states when it renders."
  - runvil-lua states the read-once rule in `crates/runvil-internal/lua/src/prose.rs:3-9` and on `SectionVm::install_lazy_prose` (`lua/src/vm/install.rs:285-292`)
- Prove: the private docs build

#### 21. Discovery and scope: one spelling per name and limit, and clear names

- `src/execute/scheduler/tool_call.rs:114-121`, `dispatch_tool_calls`, new text; the module doc at `:23-33` keeps the batch rule (S5-10):
  - "Dispatches a `tool_calls` request, the unanswered calls of one model round, under the batch rule in the module doc: each catalog call is issued as a `CallSite::Batch` under the round's `turn` and its own slot, and the slots park on the chain."
- Format the discovery tool names from `DiscoveryTool::wire_name` (`crates/runvil-internal/lua/src/scope.rs:219-226`) (S5-11)
  - `scheduler/tool_call-discovery.rs`: `:99` names `CallDynamicTool`, and `:222`, `:270`, `:283`, and `:292` name `GetDynamicTools`; format each from `DiscoveryTool::Call.wire_name()` or `DiscoveryTool::Search.wire_name()`, as `:173` and `:218` already do
  - `src/execute/scope.rs`: `wire_name` is not a `const fn`, so build `SEARCH_USAGE` (`:30-32`) and `CALL_USAGE` (`:35-36`) with `format!` inside `discovery_tools`, which formats `SEARCH_USAGE` at `:379` and copies `CALL_USAGE` at `:381`
  - also the listed-names suffix (`scope.rs:370`) and the `call_parameters` description (`:419`)
  - the output stays byte for byte the same: `src/execute/tests/discovery.rs:40` and `:179-180`, `src/execute/tests/discovery-refusals.rs:45`, and `src/execute/scope-tests.rs:278` pin it
- Move `PATTERN_LIMIT` (`tool_call-discovery.rs:38-39`) into `scope.rs` as `pub(super) const`, visible to the private `discovery` module under `execute` (S5-12)
  - import it in `tool_call-discovery.rs`; `compiled` (`:281`, `:283-284`) and its doc link (`:277`) read the import
  - `scope.rs:405`: the `pattern` description becomes `format!("A regex of at most {PATTERN_LIMIT} characters ...")` inside the `json!`, keeping the rest of the string
- `SectionOffers::read` (`scope.rs:70-78`): rename its parameter `set` to `tool_set`, the doc's "expanded against `set`" included (S5-15)
  - the caller locals `let set = chain.ctx.tool_set();` at `scheduler/chat.rs:123` (read at `:124` and `:137`) and `:220` (read at `:222` and `:224`) become `tool_set`
  - `drop_shadowed` and the `set: OfferMode` names keep `set`
- Rename `prepare_effective_scope` to `freeze_tool_sets`, the guide's and the `scope` module's word (S5-16)
  - `scope.rs:142`; `scheduler/chat.rs:35`, `:135`, `:221`; no other code names it
  - test comments: `src/execute/tests/debug_and_counts.rs:270-271` "seeded from the effective scope" becomes "seeded from the frozen sets", and `src/execute/tests/scheduler/failures-script-tools.rs:48-49` "not the section's effective scope" becomes "not the section's frozen sets"
- Prove: `cargo nextest run -p runvil-engine -E 'test(discovery) or test(scope)'` and the clippy gate

#### 22. Two small code tidies: the turn counter and the notice header

- `src/execute/turn.rs:19-27`: `advance_turn` keeps a fallback the closure can never reach (S5-2)
  - new body: `turns.update(Ordering::Relaxed, Ordering::Relaxed, |current| current.saturating_add(1)).saturating_add(1)`
  - delete the comment at `:24`; keep the doc at `:15-18`
  - the Engine builds at its 1.95 floor, where `try_update` already builds (`crates/runvil-internal/lua/src/engine_globals.rs:38`); if `update` is missing there, write `let (Ok(previous) | Err(previous)) = turns.try_update(...)` instead
  - `src/execute/tests/exec_flow.rs:211-230` pins 1, 2, and saturation
- `src/execute/scheduler/step.rs:19-21`: `H1_GATE_FAILED` copies the notice header (S5-17)
  - add `pub(super) const NOTICE_HEADER: &str = "the environment cannot satisfy this prompt:";` in `src/execute/requirements.rs`, and use it in `notice` at `:123`
  - at `step.rs:67`, build `format!("{NOTICE_HEADER}\n- the H1 gate failed")`, importing `crate::execute::requirements::NOTICE_HEADER`; delete `step.rs:19-21`
  - no test compares the H1 notice's full text: `a_failed_h1_assertion_ends_the_run_as_requirements_unmet` (`src/execute/tests/scheduler/h1/pass.rs:87-123`) reads it back without comparing it, so check by reading that the built string equals the old literal
- Prove: `cargo nextest run -p runvil-engine -E 'test(advance_turn) or test(notice) or test(failed_h1_assertion)'` and the floor build

#### 23. Heading address docs in `src/heading_address.rs`

- `find_visible`, `:314-328`, new doc; it restates the exact-address rule and states ambiguity twice (S6-1):
  - "Looks up a heading string like `"### Name"` in a caller's visible set, returning the single section at its exact address or `None`. A heading outside the visible set and a heading that names no section anywhere are the same `None`, because only the visible set is ever searched."
  - "# Errors" / "Returns [`Error::Lua`] when the heading is malformed (see [`parse_heading_address`]) or when more than one visible section matches the exact address, so an ambiguous heading never resolves to an arbitrary first hit."
- `resolve_visible` (`:391-394`) and `resolve_reachable` (`:402-405`) narrate where a rule lives (S6-2)
  - in `resolve_visible`, replace "and when no visible section matches the exact address. The not-found rule is stated on [`not_found`], which owns it." with "and, through [`not_found`], when no visible section matches the exact address."
  - the same in `resolve_reachable`, with `reachable` in place of `visible`
- Prove: the private docs build

#### 24. Substitution docs in `src/subst.rs`

- The argv nil rule is wrong at four sites; `derive_argv` (`src/execute/context-bound.rs:82-96`) owns it (S6-9)
  - module doc, `:6-7`: replace "`argv` (its parsed JSON form, nil when the args did not parse)" with "`argv` (the chain's `argv`: the derived value in the H1 pass, the value the H1 left behind on the walk; nil when it holds no value)"
  - `Sources::argv`, `:141`: "The chain's `argv`, which the H1 pass may repair: `{{ argv }}` and dotted paths."
  - `NilArgv`, `:60-61`: "An `argv` placeholder, bare or dotted, was used while `argv` was nil."
- The `Sources` doc, `:127-137`, restates the module doc, and its argv copy has drifted (S6-13)
  - new doc: "The value sources `{{ }}` placeholders resolve against." / "`var` and `sys` are JSON objects."
  - `Sources::item`, `:143`: "The item seeded into this section on a fanout arm's or a `tasks.spawn` task's first entry, rendered per [`render_item`](crate::lua::render_item): `{{ item }}`. `None` on every other entry."
- Lookup failures other than non-JSON data are undocumented (S6-11)
  - `SubstErrorKind::Serialize`, `:63`: "The bare-global lookup failed: the global cannot become JSON data, the name is `prose` itself, or the lookup raised."
  - the `globals` field, `:149-150`: "The bare-global lookup for `{{ name }}` resolution: `Ok(None)` when unset, `Ok(Some(_))` with its JSON form when set, and `Err` when the lookup fails, which a placeholder reports as `Serialize`."
  - the production lookup is `crates/runvil-internal/lua/src/prose.rs:130-146`
- Delete the comment at `:295-296`, which is `bare_global_root`'s doc (`:221-222`) word for word (S6-14)
- `nil_argv`, `:208-219`: the message's parenthetical names one of the three causes of a nil `argv` (S6-10)
  - a JSON `null` under a structured declaration, or an H1 that leaves `argv` nil, gets "the args string is not JSON" too
  - test first: in `nil_argv_is_an_error` (`src/subst-tests.rs:113-121`), assert `e.to_string().contains("is nil (argv holds no value)")` for both paths
  - it fails today, because the message reads "is nil (the args string is not JSON)"
  - `:214`, the format string becomes `"{{{{ {} }}}} is nil (argv holds no value)"`
  - no other source writes the old text; the guide's three quotes change in the guide item
- Prove: `cargo nextest run -p runvil-engine -E 'test(subst) or test(argv)'` and the private docs build

#### 25. The model import surface docs in `src/model.rs`

- `:8-12`: delete the paragraph about `models.default` and `models.use`; runvil-lua owns both globals, and `Error::ModelRequired` (`src/error.rs:315-316`) states the failure (S6-8)
- `:20`: "Three groups still name the dependency crates themselves:" becomes "Two groups ..."
- `:22-26`: the two bullets give a false reason; replace them with one (S6-7):
  - "This crate's test support and its test suites, which also need names the production surface does not re-export, such as `CompletionOptions`, `RawExchange`, `Role`, and `ToolSchemaError`."
  - keep the bench bullet at `:27-28`
- `:30-34`: names a Harness crate and its internals, against `AGENTS.md:26` (S8-3)
  - new text: "Performing a round, its transport codec included, is the caller's job. The Engine never performs a completion."
- Prove: the private docs build

#### 26. Test support and test text

- `src/test_support.rs:38-39` and `:60-61`: `recording` and its `forward` re-export become `pub(crate)`, as their `cfg(test)` siblings are (S6-16)
  - narrow the `pub` items in `src/test_support/recording.rs`, `recording/forward.rs`, and `recording/observation.rs` to `pub(crate)` too, or `unreachable_pub` (`Cargo.toml:280`) fires on each
- Rename `src/test_support/scripted-chat.rs` to `scripted_chat.rs` with `git mv`, and delete the `#[path]` at `test_support.rs:51` (S8-4)
  - `test_support/` holds six other files, so the standard layout applies (`AGENTS.md:71`); the module path, the bench's import, and the `cfg_attr` at `:44-50` stay as they are
- In that file (S6-17, S6-18, V6-1, S8-3):
  - `MOCK_MODEL`, `:19`: "The server model the reply builders name."
  - delete `ScriptedReply::answer`'s `# Errors` section (`:128-132`); `ScriptedChat::complete_now` keeps the same list
  - module doc, `:7-10`: replace "because only the in-crate suites write one" with "because only the in-crate suites use them; the bench builds its one reply from the variant directly", and keep the sentence after it
  - `fail`, `:73-76`: "For every kind it builds the kind's phrase with the status appended and the body as the detail, so each test names its kind directly."
- `src/execute/tests/context.rs:91`, the `parse` doc: "Parses `md`, adding a `# Test prompt` title when the source has none." (S8-3)
- `src/execute/tests/exit_rules.rs:376-378`, comment (V8-1):
  - "// The scripted model treats a whitespace-only reasoning channel as absent, so the message keeps the plain phrase."
- `src/lua/tests/quota.rs` describes the deleted instruction budget (V7-1)
  - module doc, `:1-4`: keep "The loop trampoline's instruction cost.", and replace the sentence that begins "`models.loop` runs on the author's Lua instruction budget" with "`models.loop`'s rules run in Rust behind the loop shim's trampoline, so the every-Nth-instruction cancel hook counts only the trampoline's own instructions: a handful per yield."
  - `:33`, the `ROUND_INSTRUCTION_FLOOR` doc: replace "about the quota" with "about the trampoline's cost"
- Prove: the clippy gate, which builds the bench, and `cargo nextest run -p runvil-engine`

#### 27. Crate docs and rulebooks

- `src/lib.rs:4-7`, new first clause (S8-1, V8-2):
  - "It issues each model round, Pack render, Pack tool call, `store` call outside a prelude's install, and timer as an [`Effect`] the Harness performs and answers"
  - the start probe (`src/execute/run.rs:319-327`) is no `store` call, so the clause leaves it out
- The crate `README.md:3`: replace "catalog tool call" with "Pack tool call" (S8-1)
- The same sentence's determinism claim ends (S8-2):
  - "and the same answers delivered in the same order at the same steps, a run produces the same effects and events."
  - `src/execute/tests/serial_driver.rs:161-189` and `src/execute/tests/happens_before.rs:440-457` show the order matters
- The crate `AGENTS.md:3`, new bullet (S8-5):
  - "The Engine depends on no tool, search, or model implementation; a run reaches one only through an effect the caller performs. `cargo test -p build-xtask` enforces the dependency rules."
- `crates/runvil-internal/README.md:3`, new text (S8-6):
  - "Code outside the Engine reaches them only through the Engine's two public crates, `runvil` and `runvil-plugin`, the only crates outside this directory allowed to depend on it."
  - `crates/build-xtask/src/product.rs:322-328` allows both
- The repository root's `AGENTS.md:9`, the Engine Definition: replace "a catalog tool's result" with "a Pack tool's result" (S8-1)
  - model-only tools leave as `Effect::ToolCall` too, and the caller answers them like catalog tools
  - `docs-claims.mjs` skips the Definitions section itself (`:39`, `:88`), so the edit cannot trip the scan
- The repository root's `README.md:59`: the same replacement, "a catalog tool's result" to "a Pack tool's result" (S8-1)
- Prove: `node --test crates/workshop/ui/test/docs-claims.mjs`, `cargo test -p build-xtask`, and the workspace docs build

#### 28. Guide edits in the `runvil-docs` clone

- Its own repository: one commit there, after the Rust items, and nothing in `runvil` changes; paths are below `src/language/`
- `12-tools.md`: the out-of-scope message changed its label and gained an ending (S7-1)
  - replace `in-scope aliases:` with `aliases this section offers:` in the quotes at `:2314`, `:2518`, and `:2576`; neither the `search-loop` nor the `scope-check` example offers a Dynamic Set tool, so only the label changes
  - `:2582`, new text: "- The message names the requested tool and lists the aliases the section offers by their wire names, not their tool ids, each in double quotes, as a bracketed list of quoted names, such as `["echo"]`. The list ends the message, unless the not-offered hint follows it or the section's Dynamic Set holds a tool, which adds `; a Dynamic Set tool is reached through GetDynamicTools and CallDynamicTool` last."
  - `:2596`: replace "so their message ends with the in-scope list:" with "so their message ends with the offered list, or with the Dynamic Set ending when the section's Dynamic Set holds a tool:"
  - `:2608`, the table cell: "`tool {name:?} is not in this section's scope; aliases this section offers: {static_aliases:?}`, followed by the not-offered hint when it applies, then `; a Dynamic Set tool is reached through GetDynamicTools and CallDynamicTool` when the section's Dynamic Set holds a tool"
  - the code is `src/error.rs:58-87` in the engine; `src/execute/tests/tool_loop-scope-gate.rs:59-65` and `:215-220` pin it
- `01-what-a-prompt-is.md:608`, new text (S7-2):
  - "The Harness answers every effect exactly once: with its result, with its failure, such as the reason a model round, a tool call, or a store operation failed, or as dropped when it gives the work up. A round the Host stops is answered as dropped, which the waiting call raises as the `cancelled` error, as [Stopping a model round](#stopping-a-model-round) shows."
- `04-how-a-prompt-runs.md`: a run refused when it is built reports no run events (S7-3)
  - `:1664`: "A run that does not succeed" becomes "A run that started and does not succeed"; the rest stays
  - the table at `:210-214` gains the row "| the run is refused when it is built: an unsupported or missing version, no declared store, or a store that fails its probe | `parse_started` | `parse_succeeded` |"
  - `:286`: append "Such a run reports no run events, so its record ends with `parse_succeeded`, the last parse event."
  - the engine side is `src/execute/run.rs:141-157` and `:203-216`, pinned by `src/execute/run/tests.rs:466-490`
- `16-limits-and-errors.md:836`: replace the clause ", including a file with no `runvil:` key, whose parse error reads `not a runvil prompt: no runvil version`." with "." and add after that sentence (V7-4):
  - "A file with no `runvil:` key also fails with kind `Parse` and the message `not a runvil prompt: no runvil version`, but its prompt parses: the check runs when the run is built, after input staging, the wait for Packs, and the requirements check, so an input or requirements refusal comes first when one applies, and its record ends with `parse_succeeded`."
- `16-limits-and-errors.md:877`: replace "No instruction-count quota runs out in practice." with "No instruction-count quota exists." (S7-4)
- `04-how-a-prompt-runs.md:343`: replace "The Engine's own part checks model roles only, so" with "The Engine's own part checks model roles and the reserved Pack ids `adhoc` and `dynamic` in the run's catalog and roster, so" (S7-5)
- `16-limits-and-errors.md:894` (V2-2):
  - replace "a requirement the prompt states" with "what the prompt needs"
  - after "such as a context minimum or a hard keyword," insert " a tool in the run's catalog or a Pack in its roster has a Pack id the Engine reserves, `adhoc` or `dynamic`,"
- `10-models.md:1455`: replace "so a task's infer rounds count against the task's own cap rather than the run's." with "so a task's `turn` numbers, and its `tasks.status` `turns`, count only its own rounds. No cap counts infer rounds." (S7-6)
- `15-tasks.md:2009`: replace "so its model rounds count against its own cap and never add to its spawner's count" with "so its rounds are numbered on its own counter and never add to its spawner's count"; the rest stays (V7-2)
- `05-lua-environment.md:1936`: replace "`sys.model` appears at the section entry's first call to a catalog tool or an adhoc tool," with "`sys.model` appears at the section entry's first tool call: a call to a catalog tool or an adhoc tool, or the model's call to a discovery tool, refused calls included," and keep the rest (S7-8)
  - the engine installs it at every first tool call, discovery calls included (`src/execute/scheduler/tool_call-discovery.rs:116-118`)
- `07-substitution.md`: the nil-argv message's three quotes follow the engine's new text (S6-10)
  - `:519`: replace "`{{ {path} }} is nil (the args string is not JSON)`" with "`{{ {path} }} is nil (argv holds no value)`"
  - `:553`: replace "`{{ argv }} is nil (the args string is not JSON)`" with "`{{ argv }} is nil (argv holds no value)`"
  - `:1292`, the `NilArgv` table row: its message cell becomes "`{{ {path} }} is nil (argv holds no value)`"
- Prove: the guide's site build, with `RUNVIL_DOCS` naming the clone

</execution-plan>
