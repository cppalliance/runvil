---
name: Remove tools.allow_tasks
overview: "Remove tools.allow_tasks, the four model task tools it advertises (task, task_cancel, task_status, await_tasks), and every path that exists only for model-origin tasks, including TaskOrigin and the task notices queue. The Lua tasks object keeps its functions."
todos: []
isProject: false
---

# Remove tools.allow_tasks

<product-contract>

## Product Requirements

`tools.allow_tasks` lets a section offer the model four task tools that mirror the Lua `tasks` object, but the model needs a different shape, so the feature goes away entirely. This plan removes the Lua function, the four tools, every Engine path that exists only for tasks the model starts, and their tests and comments. The Lua `tasks` object stays, minus its notion of task origin. The model-facing replacement is a later subagents Plugin and is not part of this plan.

- Problem and users:
  - Prompt authors call `tools.allow_tasks(targets?)` in a section to make the Engine advertise `task`, `task_cancel`, `task_status`, and `await_tasks` to the model (`crates/runvil-internal/engine/src/execute/scheduler/builtins.rs`).
  - Those tools copy the Lua `tasks` API, which is the wrong shape for a model. Lua tasks return futures with await functions and can outlive the section that spawned them; a model's work never outlives its section. The right model-facing shape also differs per model and needs late tool rendering. The model will instead be offered a subagents Plugin (the reserved built-in `runvil/subagents`), designed separately.
- Goals:
  - Remove `tools.allow_tasks`, the four model task tools, and every type, field, function, module, test, and comment that exists only for them.
  - Remove model-origin tasks completely: `TaskOrigin` (both variants), the `origin` field of `Event::TaskStarted`, the `origin` field of Lua task status, the `origin` filter of `tasks.pending`, the `origin` field that `tasks.spawn` and fanout send, and the task notices queue.
- Non-goals:
  - Changing any other behavior of the Lua `tasks` object: spawn, await, cancel, status, pending, timers, and tasks outliving their section all keep working as they do today.
  - Designing or starting the subagents Plugin.
  - Editing the guide in the separate `runvil-docs` repository.
- Success criteria:
  - `git grep -n -E 'allow_tasks|allowed_tasks|TaskAllowlist|TaskBuiltin|task_allowlist|TaskOrigin|AwaitTasks|await_tasks|task_cancel([^l]|$)|queue_task_notice|advertise_task_builtins|answer_task_builtin' -- ':!vibe'` prints nothing.
  - No test, doc comment, or Lua shim mentions model-started tasks, the four tool names as tools, or task origin.
  - Every exit check under Testing Plan passes.
- Constraints:
  - Engine crate text (docs, comments, tests, strings, names) never mentions the Host and names the Harness only to say a responsibility belongs to it.
  - Behavior changes ship with tests in the same change; product and behavior tests of the Lua `tasks` object are preserved.
  - A source subdirectory holds at least three files, counting nested ones; one or two files sit beside the parent module as `foo-bar.rs` with `#[path = "foo-bar.rs"] mod bar;`. When a deletion leaves a subdirectory or a `foo-*.rs` group on the wrong side of three files, convert it in the same change. Top-level `tests/` and `benches/` trees are exempt.
  - `crates/runvil/public-api.txt` is regenerated with `cargo xtask api` on the nightly toolchain named in `crates/build-xtask/src/api/toolchain.rs`, and `--check` must pass after the change.
- Open questions: None

## Functional Specification

After the change a prompt has no way to give the model task tools, and the Lua `tasks` object works as before without any origin concept. A recorded run log written before the change still loads. The only new error is for a `tasks.pending` call that passes an argument.

- Actors and workflows:
  - A section's Lua calls `tools.allow_tasks(...)`: `tools.allow_tasks` is nil, so Lua raises its standard error for calling a nil field. No compatibility alias exists.
  - A model round: the advertised tool list never includes `task`, `task_cancel`, `task_status`, or `await_tasks`. A model call naming one of them takes the existing path for a tool name the round did not advertise, with that path's existing message.
  - The four names are no longer reserved by the Engine (`crates/runvil-internal/engine/src/execute/context-bound.rs` line 60 check is removed).
- Inputs and outputs:
  - `tasks.status(id)` returns the same table as today without the `origin` key.
  - `tasks.pending()` returns every pending task of the run, as `tasks.pending()` with no filter does today. It takes no argument: a call with any non-nil argument raises `tasks.pending takes no arguments, got ` followed by the argument's Lua type name (for example `got table`).
  - `tasks.spawn` and fanout spawn requests no longer carry `origin` (`crates/runvil-internal/lua/src/__impl_tasks.lua` line 41, `crates/runvil-internal/lua/src/__impl_fanout.lua` line 111), and the Rust spawn parser no longer reads it (`crates/runvil-internal/lua/src/protocol/parse/tasks.rs` `shim_origin`).
  - `Event::TaskStarted` (`crates/runvil-internal/types/src/event.rs` lines 303-319) has no `origin` field; the facade no longer exports `TaskOrigin` (`crates/runvil/src/lib.rs` lines 64, 68).
- States and validation:
  - A chain is never parked on a model `await_tasks` call: `Chain.awaiting` and everything that sets, clears, or takes it are removed.
  - When a task or its owner ends, no notice is queued for the model; the notices module and the notice drain in `chat.rs` are removed.
- Errors and recovery:
  - Old recorded logs: a `task_started` event that carries `"origin":"author"` or `"origin":"model"` still deserializes into `Event::TaskStarted`, because `Event` does not set `deny_unknown_fields` (`crates/runvil-internal/types/src/event.rs` lines 69-70), and Workshop's run-log reads payloads as untyped JSON (`crates/workshop/run-log/src/read.rs` lines 82-91). No shim is added.
- Security and privacy behavior:
  - Removes the untested path where a hand-built Lua yield with `origin = "model"` could create a model-origin task (`parse/tasks.rs` lines 95-103).
- Acceptance criteria:
  - `tools.allow_tasks` is nil in every section.
  - No model round advertises a task tool, with or without any Lua call.
  - The `tasks.status` table has no `origin` key; `tasks.pending({})` and `tasks.pending({ origin = 'author' })` raise the error above; `tasks.pending()` still lists pending tasks.
  - A JSON `task_started` event containing an `origin` field deserializes.
  - Existing Lua `tasks` behavior tests pass unchanged except for removed `origin` assertions.

</product-contract>
<implementation-contract>

## Technical Design

This is a deletion that crosses the Lua crate, the Engine scheduler, the shared types crate, and the facade surface. The scheduler loses its built-in tool route, its await-parking state, and its notices queue; the task arena, spawn, wait, cancel, and status paths stay because the Lua `tasks` object uses them. `TaskOrigin` disappears from the types crate, the public event, and the Lua protocol.

- Architecture:
  - Kept, shared with the Lua `tasks` object: `prepare_spawn` (`scheduler/tasks.rs`), `cancel_task`, `task_status`, `visible_slot`, `live_tasks_of`, `wake_waiter`, `park_wait`, `wake_from_wait` (`scheduler/waits.rs`), `prepare_timer`, `join_task`, the chain's `waiting_on` set.
  - Removed, model-only: everything in `scheduler/builtins.rs` (`TaskBuiltin`, `task_allowlist`, `BuiltinAnswer`, `BuiltinOutcome`, `task_id_argument`, `render_status`, `answer_task_builtin`, `report_builtin_answer`, `builtin_task`, `model_task`, `builtin_task_cancel`, `builtin_task_status`), `scheduler/builtins-schemas.rs`, `scheduler/builtins-tests.rs`, `scheduler/await_tasks.rs` (`AwaitTasks`, `finish_await_tasks`), `scheduler/notices.rs` (`queue_task_notice` and its drain), `Chain.awaiting`, `DispatchTarget::Builtin`, `ToolRoute::Builtin` and its reserved-name check, and the `ToolCallDispatch::Started` and `Parked` variants with their arms in `tool_call.rs` lines 117-123 (only `builtins.rs` lines 219 and 224 produce them).
  - Lua crate: `TaskAllowlist` and `permits` (`lua/src/scope.rs` lines 83-124), `ToolRuntime.allowed_tasks`, `install_allow_tasks`, `task_allowlist_from`, and the call at `lua/src/tools.rs` line 194; the `TaskAllowlist` re-exports in `lua/src/lib.rs` line 138 and `engine/src/lua.rs` line 20.
- Modules and interfaces:
  - `TaskOrigin` (`crates/runvil-internal/types/src/ids.rs` lines 260-290) and `TaskSlot.origin` are removed, with every comparison on them: `task_end.rs` lines 74-79 and 200-201 (`complete_task`, `abandon_owned_tasks`), the notice move at `task_end.rs` lines 270-271, `waits.rs` lines 196, 304, and 361, `admission.rs` lines 201-213, `dispatch.rs` lines 136-155, `chain_record.rs` lines 198 and 216, `chain.rs` lines 111 and 281, `drive.rs` line 225, `apply.rs` line 289, `chat.rs` lines 105-155.
  - Lua protocol: `Request::Pending.origin` (`protocol/request.rs` lines 122-125) goes and `Request::Pending` carries no data; `parse_pending` (`protocol/parse/tasks.rs` lines 192-221) accepts only nil and raises the error from the Functional Specification otherwise; the Lua shim `tasks_pending` (`__impl_tasks.lua` lines 205-229) passes its argument through so the Rust parser owns the check; `TaskStatus.origin` (`protocol/answer.rs` line 226) and its render (`protocol/render.rs` line 28) go.
- File and public API changes:
  - Facade: `TaskOrigin` leaves `crates/runvil/public-api.txt` and `crates/runvil/src/lib.rs`; `Event::TaskStarted` loses `origin`. Regenerate the listing with `xtask api` on the pinned nightly.
  - Doc comments naming `tools.allow_tasks`, the four tools, model tasks, or task origin are removed or rewritten: `scheduler.rs` lines 48-53, `scheduler/chat.rs` lines 4-9, `engine/src/execute/scope.rs` lines 23-33, `lua/src/tools.rs` lines 13-14, `lua/src/vm.rs` line 377, `types/src/event.rs` line 433.
  - Layout: deleting `builtins-schemas.rs` and `builtins-tests.rs` removes the `builtins-*` group together with `builtins.rs`. Deleting `model_task_notices.rs` also deletes `model_task_notices-endings.rs`. Recount `scheduler/` and `execute/tests/` after deletion and apply the three-file rule.
- Data, persistence, failure, security, and privacy constraints:
  - Recorded `task_started` events change shape (no `origin`). Old logs stay readable through serde's default handling of unknown fields; no reader decodes them strictly.
  - No new dependencies, features, or unsafe code.

</implementation-contract>
<verification-contract>

## Testing Plan

Most of the work removes tests that exist only for the removed feature. Tests that use `tools.allow_tasks` only as setup keep their other assertions. A few new tests pin the remaining observable changes: the shape of the Lua `tasks` surface and backward-compatible reading of old logs.

- Unit:
  - Delete `lua/src/tools/tests.rs` `allow_tasks_records_the_section_allowlist_and_rejects_bad_targets` (lines 187-212); in `lua/src/tools/tests-offering.rs` line 192 drop `'allow_tasks'` from the listed names and assert `tools.allow_tasks == nil`.
  - New: `tasks.pending({})` and `tasks.pending({ origin = 'author' })` raise `tasks.pending takes no arguments, got table`; `tasks.pending()` lists a pending task.
  - New: a `tasks.status` table has no `origin` key.
  - Update `lua/src/protocol/tests/parse_tasks.rs` and `types/src/ids-tests.rs` line 81 to drop origin assertions; delete tests that exist only for `TaskOrigin`.
  - New regression: a JSON `task_started` event with `"origin":"author"` deserializes into `Event::TaskStarted`.
- Integration and end-to-end:
  - Delete the model-task Engine test files under `crates/runvil-internal/engine/src/execute/tests/`: `model_tasks.rs`, `model_task_answers.rs`, `model_task_awaits.rs`, `model_task_ids_and_scope.rs`, `model_task_notices.rs`, `model_task_notices-endings.rs`, `model_task_trust.rs`, `model_task_acceptance.rs`, and their `mod` lines.
  - Edit tests that use the feature only as setup: `batch_turn.rs`, `parallel_tool_calls.rs`, `serial_driver.rs`, `full_id_calls.rs`, `tool_call_arm.rs`, `models_loop-arguments.rs`, `models_loop_contract.rs`, `engine/src/execute/run/tests-drops.rs`. Remove the `tools.allow_tasks` lines and task-tool expectations; delete a test case only when its whole purpose was a task tool.
- Regression, security, and performance:
  - The removal grep from Success criteria prints nothing.
- Exit criteria, every command below passes:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, and `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`.
  - Both clippy runs with `CARGO_BUILD_WARNINGS=deny`, and `cargo check -p gateway --no-default-features`.
  - `cargo fmt --all --check`.
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api` and `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`.
  - `cargo xtask api --check` on the nightly toolchain named in `crates/build-xtask/src/api/toolchain.rs`, with `CARGO_BUILD_WARNINGS` unset.
  - `cargo test -p build-xtask` and `node --test crates/workshop/ui/test/docs-claims.mjs`.

</verification-contract>
<decision-record>

## Decision Record

- Decisions:
  - Remove `tools.allow_tasks` and the four model task tools completely. Rationale: the model needs a different task shape than Lua, one that differs per model, so these tools cannot stay as-is. Product owner, via the user: "you need to rip out the four tools that it offers ... remove all the tests, remove every trace of it. Leave the tasks, leave the the tasks object there with its functions".
  - Remove model-origin tasks entirely, including `TaskOrigin`, `Event::TaskStarted.origin`, the notices queue, and the `tasks.pending` origin filter, even though this changes the facade and the recorded event shape. Rationale: with the `task` tool gone nothing creates a model-origin task, so this code would be unreachable. User choice: "Remove all".
  - `tasks.pending` takes no argument and rejects any argument. Rationale: silently ignoring an old `origin` filter would change results without telling the author or a model reading the error; the message names the expected and actual shapes.
  - Release the four names from the Engine's reserved set. Rationale: no built-in owns them; the future subagents Plugin will put its tools under its own Plugin id.
  - Rely on serde's default unknown-field handling for old logs instead of a compatibility shim. Rationale: `Event` already accepts unknown fields and the run-log reader is untyped.
- Rejected alternatives:
  - Keep `TaskOrigin` on the public and recorded surfaces and reject `"model"` in the parser. Rejected by the user. Revisit when a second task origin exists again.
  - Remove only the tools and their direct wiring, leaving notices and model-origin branches unreachable. Rejected by the user and by the principle against dormant machinery. No revisit condition.
  - Keep `tasks.pending(filter)` and ignore the filter. Rejected: silent behavior change. No revisit condition.
- Assumptions, risks, and notes:
  - The subagents Plugin will define its own task model and will not need `TaskOrigin` back in its old form.
  - The `runvil-docs` guide documents `tools.allow_tasks` and `origin` until it is edited separately.
  - Prompts in other repositories that call `tools.allow_tasks` will fail at that call. This repository has no such prompt fixtures.

### Deferred and Out of Scope

- Deferred: guide edits in the separate `runvil-docs` repository that remove `tools.allow_tasks`, the four tools, and task origin. Revisit when that repository is next edited.
- Out of scope: the subagents Plugin and its model-facing task tools.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p gateway` (the workspace default member; plain `cargo build` builds only it). Desktop app: `cargo build --locked -p workshop`, or `cargo workshop` (alias for `build-workshop`, which builds and stages the gateway sidecar, then builds Workshop). Workshop UI bundle: `npm run build --workspace ui` from `crates/workshop` (after `npm ci` there and in `crates/gateway/config-ui/ui`; the UI crates' build scripts bundle with esbuild through `build-ui`).
- Focused test command pattern: `cargo nextest run --locked -p <crate> --all-features <test-name-filter>` (drop `--all-features` for `workshop`, `workshop-server`, `workshop-server-api`); a single UI test file: `node --test crates/workshop/ui/test/<name>.mjs`.
- Component test command pattern: `cargo nextest run --locked -p <crate> --all-features`; structural checks: `cargo test -p build-xtask`; a UI package: `npm test --workspace <ui|look|platform>` from `crates/workshop`, or `npm test` from `crates/gateway/config-ui/ui`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`; UI suites: `npm test --workspaces --if-present` in `crates/workshop` and `npm test` in `crates/gateway/config-ui/ui`.
- Linter command: `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `CARGO_BUILD_WARNINGS=deny cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`; headless shape: `cargo check -p gateway --no-default-features`; wording scan: `node --test crates/workshop/ui/test/docs-claims.mjs`; TypeScript: `npm run typecheck --workspaces --if-present` in `crates/workshop`, `npm run typecheck` in `crates/gateway/config-ui/ui`; supply chain: `cargo deny check`.
- Formatter check command: `cargo fmt --all --check` (also the pre-commit hook).
- Docs command: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, plus facade docs `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`; facade surface `cargo +<nightly from crates/build-xtask/src/api/toolchain.rs> xtask api --check` with `CARGO_BUILD_WARNINGS` unset.
- Test placement and naming conventions: Rust unit tests live beside their module in a sibling `<stem>-tests.rs` wired by `#[cfg(test)] #[path = "<stem>-tests.rs"] mod tests;` (or `<dir>/tests.rs` / a `tests/` subdirectory once a group reaches three files), each opening with a `//!` doc naming what it covers. Integration tests compile as one binary per crate under `tests/suite/` (`main.rs` plus one module per topic, e.g. `crates/runvil/tests/suite/`, `crates/harness/tests/suite/`). Prompt fixtures are Markdown under `tests/prompts/{valid,invalid,execution}/` loaded with `include_str!`. Shared test support sits in a `test_support` module (e.g. `crates/runvil-internal/engine/src/test_support/`: rig, scenario, seeded, temp_root, recording). Engine tests use no sleeps, wall clock, timeouts, or OS entropy; randomness comes from a printed fixed seed; temp dirs are named from test name, pid, and a counter. TypeScript tests are `node --test` `.mjs` files in each package's `test/` directory (`crates/workshop/{ui,look,platform}/test/`) or `src/**/*.test.mjs`, using jsdom. Nextest config is `.config/nextest.toml` (60s slow timeout; whisper suites in a `heavy` group).
- Directory map: `crates/` holds every crate; its top level is the public and shared layer (`runvil` Engine facade with `public-api.txt`, `runvil-plugin` Plugin API, `harness`, `harness-gateway-client`, `plugin-web`/`plugin-mcp`/`plugin-user-input`, `gateway-api-types`/`gateway-api-discovery` shared gateway vocabulary, `shared-error-source`/`shared-loopback`, `shared-ui` TypeScript+CSS package, and `build-*` tooling: `build-xtask` structural checks and `xtask api`, `build-ceiling` 500-line file check run by every crate's `build.rs`, `build-ui` esbuild helper, `build-workshop`, `build-user-guide`, `build-llama-cuda`). Manifestless private containers: `crates/runvil-internal/` (Engine: `types`, `parser`, `lua`, `vfs`, `model-client`, `engine`), `crates/harness-internal/runner`, `crates/gateway/` (`app` binary, `config`, `config-ui`, `local`, `logging`, `progress`, `protocol`, `routing`, `cloud-providers`, `web-search`, and `stt/{api,engine,backend-whisper,whisper-ffi}`), `crates/workshop/` (Rust: `desktop` Tauri app, `server`, `server-api`, `agents`, `gateway`, `menu`, `protocol`, `registry`, `run-log`, `status`, `support`, `user-state`, `workspace`; npm workspace: `ui`, `look`, `platform`). Other top level: `prompts/` sample prompts, `guide/` docs site (`books`, `chrome`, `landing`), `tools/` Node scripts (gateway sidecar staging, TTS live check), `vibe/` dated plan files, `images/`, `.github/workflows/` (ci, nightly, release, site, stt-miri), `.githooks/` (pre-commit fmt, pre-push docs-claims + headless check + clippy + deny), `.cursor/rules`, `.config/nextest.toml`, `.cargo/config.toml` (aliases `xtask`, `workshop`).
- Component boundaries: enforced by `crates/build-xtask/src/product.rs` (`cargo test -p build-xtask`). Engine (`runvil`, `runvil-*`) depends on no gateway, workshop, or Harness crate; `runvil` alone reaches `runvil-internal/*` (plus `runvil-plugin`, which uses `runvil-types`/`runvil-vfs`). Harness (`harness` -> `harness-runner`, `runvil`, `runvil-plugin`) names only `runvil` and `runvil-plugin` outside its family; only `harness` reaches `harness-internal/`. `plugin-*` depend only on `runvil-plugin`, `shared-*`, and outside libraries; only `harness-gateway-client` (-> `harness`, `plugin-web`, `runvil`) may name a Plugin crate among Engine/gateway/Harness crates. Gateway crates depend on no runvil, workshop, or Harness crate; outsiders reach it only via `gateway-api-types`/`gateway-api-discovery`; `gateway-stt` is the stt subsystem's public member. Workshop crates use only the public gateway pair; `workshop-server` is the Host, depending on `harness`, `harness-gateway-client`, the `plugin-*` crates, `runvil`, `runvil-plugin`, and its workshop siblings; the `workshop` desktop app depends on no workshop crate but `workshop-server-api`, which depends only on `workshop-server`. `shared-*` depend on no product crate. Engine crates never pull `tokio`, `async-trait`, `reqwest`, `getrandom`, or `fastrand`.
- Conventions summary: Rust 2024 edition, MSRV 1.95 for Engine crates, workspace version 0.3.0, BSL-1.0; workspace lints inherited by every crate (`unreachable_pub`, `unsafe_op_in_unsafe_fn = deny`, `rust_2018_idioms`), `#[expect(..., reason = "...")]` instead of bare allows; every Rust file at most 500 lines (`build-ceiling`); every source file opens with a `//!` doc; crate docs carry an `## Invariants` section; flat source directories with kebab `foo-bar.rs` siblings wired by `#[path]` until a group reaches three files; capitalized defined terms (Engine, Harness, Host, Plugin) checked by `docs-claims.mjs`, and Engine crates say "the caller"; model-readable error messages stating required versus actual; recorder JSON round-trips with sorted keys; Cargo features only for real build constraints; unsafe confined to allowlisted boundaries; workaround comments cite upstream issue URLs; Workshop UI CSS uses only tokens from `@workshop/look` and `ui/src/tokens/component.css`, persisted UI state goes through `ui-storage` to the server; per-directory `AGENTS.md` files add local rules; plans live in `vibe/` as `YYYY-MM-DD-N-slug.md`; PRs are drafts and plan steps ship as stacked PRs.

</project-survey>
<execution-plan>

## Execution Instructions

<step-1>

### Step 1: Remove tools.allow_tasks and the four model task tools [completed]

- Component: `none`
- Tests: `cargo nextest run --locked -p runvil-lua -p runvil-engine -p runvil-types --all-features`

Remove the Lua function, the four model task tools, and every Engine path that serves only them: the built-in tool route, await parking, and the task notices queue. `TaskOrigin` stays in this step (its `Model` variant becomes unproduced by the Engine); Step 2 removes it. The Lua and Engine removals share re-exports and land together in one commit with their tests.

- Lua crate (`crates/runvil-internal/lua/src/`):
  - `scope.rs` lines 83-124: delete `TaskAllowlist` and `permits`, and the `ToolRuntime.allowed_tasks` field.
  - `tools.rs`: delete `install_allow_tasks`, `task_allowlist_from`, and the call at line 194; rewrite the module doc at lines 13-14 and the comment near line 268 so neither mentions the allowlist or task built-ins.
  - `lib.rs` line 138: drop the `TaskAllowlist` re-export.
  - `vm.rs` line 377, `models_loop/machine.rs` line 105, `protocol/request.rs` line 171: drop the task built-in wording from the doc comments.
- Engine crate (`crates/runvil-internal/engine/src/`):
  - `lua.rs` line 20: drop the `TaskAllowlist` re-export.
  - Delete `execute/scheduler/builtins.rs` (`TaskBuiltin`, `task_allowlist`, `BuiltinAnswer`, `BuiltinOutcome`, `task_id_argument`, `render_status`, `answer_task_builtin`, `report_builtin_answer`, `builtin_task`, `model_task`, `builtin_task_cancel`, `builtin_task_status`), `builtins-schemas.rs` (including `advertise_task_builtins`), `builtins-tests.rs`, `await_tasks.rs` (`AwaitTasks`, `finish_await_tasks`), and `notices.rs` (`queue_task_notice` and its drain), with their `mod` lines in `execute/scheduler.rs` (lines 64, 65, 72).
  - Remove `Chain.awaiting` (`scheduler/chain.rs`) and everything that sets, clears, or takes it (`waits.rs`, `task_end.rs`, `apply.rs`, `chain_record.rs`, `drive.rs`).
  - Remove `DispatchTarget::Builtin` (`dispatch.rs`), `ToolRoute::Builtin`, and the `ToolCallDispatch::Started` and `Parked` variants with their arms at `tool_call.rs` lines 117-123.
  - `execute/context-bound.rs` lines 56-63: delete the `TaskBuiltin::from_name` check in `offer_refusal` and its doc clause, so `task`, `task_cancel`, `task_status`, and `await_tasks` are no longer reserved.
  - `scheduler/chat.rs`: remove the notice drain and the task-tool advertising (around lines 105-155) and rewrite the module doc at lines 4-9. A model round never advertises a task tool; a model call naming one takes the existing path for a tool name the round did not advertise, with that path's existing message.
  - `scheduler/task_end.rs`: remove the notice queueing in `complete_task`, `abandon_owned_tasks`, and the notice move at lines 270-271. Keep each existing origin branch (Step 2 removes them).
  - `scheduler/waits.rs`: remove the notice queueing at line 298 onward and the `await_tasks` wording at line 21; drop returning "the target of a live model task this call ended" (line 316) if its only consumer was a notice.
  - Doc comments: `execute/scheduler.rs` lines 48-53, `execute/scope.rs` lines 23-33, `scheduler/tool_call.rs` lines 11, 67, and 276, `scheduler/timer.rs` lines 2 and 48, and `scheduler/task_end.rs` lines 9, 32, 144, and 208. No comment mentions model tasks, the four tool names as tools, `tools.allow_tasks`, or task notices.
- Types crate doc comments: `types/src/event.rs` line 409 and `types/src/metrics.rs` line 32 drop "a task built-in".
- Workshop: `crates/workshop/ui/src/services/agent-session.ts` line 58 drops "a task built-in".
- Tests to delete:
  - `lua/src/tools/tests.rs` `allow_tasks_records_the_section_allowlist_and_rejects_bad_targets` (lines 187-212).
  - Under `engine/src/execute/tests/`: `model_tasks.rs`, `model_task_answers.rs`, `model_task_awaits.rs`, `model_task_ids_and_scope.rs`, `model_task_notices.rs`, `model_task_notices-endings.rs`, `model_task_trust.rs`, `model_task_acceptance.rs`, and their `mod` lines in `execute/tests.rs` (lines 178-184 plus the `-endings` path attribute inside `model_task_notices.rs`).
- Shared helpers to relocate before deleting their files:
  - `task_lifecycle` and `terminals_per_started_task` (`model_task_acceptance.rs` lines 30-60) move unchanged to `execute/tests/fixtures.rs`; update the imports in `fanout_acceptance.rs` line 20 and `run_termination.rs` line 14.
  - `loop_owner` (`model_task_notices.rs`), `model_task_scenario` and `CHILD_PARKED_ON_SLOW_TOOL` (`model_tasks.rs`): callers are `models_loop-arguments.rs`, `models_loop_contract.rs`, and `serial_driver.rs`. Delete each caller test whose whole purpose was a task tool (for example `serial_driver.rs` `a_model_task_whose_owner_ends_first_reports_abandoned_in_its_event_and_its_notice`, and tests that cancel `tasks.pending({ origin = 'model' })`). When a surviving test still needs a helper, move that helper to `fixtures.rs` without its `tools.allow_tasks()` line.
- Tests to edit (remove `tools.allow_tasks` lines and task-tool expectations; delete a case only when its whole purpose was a task tool): `batch_turn.rs`, `parallel_tool_calls.rs` (including the module doc at line 4), `serial_driver.rs` (module doc line 8), `full_id_calls.rs`, `tool_call_arm.rs`, `models_loop-arguments.rs`, `models_loop_contract.rs`, `engine/src/execute/run/tests-drops.rs`.
- Tests that change meaning because the names are released:
  - `offering.rs` `the_offering_leaves_out_a_name_that_is_reserved_unadvertisable_or_repeated`: no name is reserved any more. Delete `offered_tool("task/cancel")` from its catalog (keeping it would add a `task_cancel` wire name and break the removal grep), rename the test to drop "reserved", and rewrite the assertion message to name only the unadvertisable schema and the later repeat. The expected offered list is unchanged.
  - `full_id_calls.rs` `unofferable_tool` and `a_script_reaches_a_catalog_tool_left_out_of_the_offering_by_id`: a `task/cancel` tool is now offerable. Make the tool unofferable by a schema no round can advertise (a scalar `parameters` schema such as `json!("scalar")`, as `offering.rs` `shapeless_tool` does), give it an id unrelated to tasks (for example `hidden/shape`) in the tool, the Lua source, and the expected `EffectRecord::ToolCall`, keep the by-id call assertions, and rewrite the doc and assertion messages to give the schema reason.
- Tests to add or update:
  - `lua/src/tools/tests-offering.rs` line 192: drop `'allow_tasks'` from the listed names and assert `tools.allow_tasks == nil` in a section.
  - Engine: a section whose Lua calls `tools.allow_tasks()` fails with Lua's standard error for calling a nil field (the message names `allow_tasks`).
  - Engine: a model round in a section that runs `models.loop` advertises none of `task`, `task_cancel`, `task_status`, `await_tasks`.
- Layout: `scheduler/` keeps 18 files after deleting six; the `chain_record-*` group is unchanged. Recount `execute/tests/` after the deletions and apply the three-file rule to any group that crosses it. Every touched Rust file stays at or under 500 lines (`build-ceiling`).
- Acceptance: `git grep -n -E 'allow_tasks|allowed_tasks|TaskAllowlist|TaskBuiltin|task_allowlist|AwaitTasks|await_tasks|task_cancel([^l]|$)|queue_task_notice|advertise_task_builtins|answer_task_builtin' -- ':!vibe'` prints nothing. The `task_cancelled` event helpers in `execute/tests/scheduler.rs`, `scheduler/failures.rs`, `scheduler/fanout/failure_semantics.rs`, and `tests/waits.rs` stay; the pattern does not match them. Existing Lua `tasks` behavior tests pass unchanged.

</step-1>

<step-2>

### Step 2: Remove TaskOrigin and make tasks.pending take no arguments

- Component: `none`
- Tests: `cargo nextest run --locked -p runvil-types -p runvil-lua -p runvil-engine -p runvil --all-features`

With nothing producing a model-origin task, remove task origin from the types crate, the public event, the Lua protocol and shims, the scheduler, and the facade, then regenerate `public-api.txt`. One commit with code and tests.

- Types crate (`crates/runvil-internal/types/src/`):
  - `ids.rs` lines 260-290: delete `TaskOrigin`.
  - `event.rs` lines 303-319: remove the `origin` field from `Event::TaskStarted`; rewrite line 433's doc so it does not mention origin. Do not add `deny_unknown_fields` to `Event` (lines 69-70); old logs carrying `origin` must still load.
- Lua crate (`crates/runvil-internal/lua/src/`):
  - `protocol/request.rs` lines 122-125: `Request::Pending` carries no data.
  - `protocol/parse/tasks.rs`: delete `shim_origin` and the `origin` read in the spawn parser (lines 95-103, which also closes the path where a hand-built yield with `origin = "model"` made a model-origin task); `parse_pending` (lines 192-221) accepts only nil and otherwise raises exactly `tasks.pending takes no arguments, got ` followed by the argument's Lua type name (for example `tasks.pending takes no arguments, got table`).
  - `protocol/answer.rs` line 226: remove `TaskStatus.origin`; `protocol/render.rs` line 28: stop rendering it.
  - `__impl_tasks.lua`: drop `origin = "author"` at line 41 and the comment at line 26; rewrite `tasks_pending` (lines 205-229) to pass its argument through to the yield unchanged so the Rust parser owns the check, and drop the filter wording at line 213.
  - `__impl_fanout.lua` line 111: drop `origin = "author"`.
- Engine crate (`crates/runvil-internal/engine/src/`):
  - `scheduler/tasks.rs` lines 23, 91, 137, 178: remove `TaskSlot.origin` and the `origin` parameters of `prepare_spawn` and its sibling.
  - `scheduler/timer.rs` lines 27 and 59: drop `TaskOrigin::Author`.
  - Remove every remaining comparison on origin, keeping the author behavior as the only behavior: `task_end.rs` lines 74-79 and 200-201 (`complete_task`, `abandon_owned_tasks`: an ending owner's live tasks follow today's author-task path), `waits.rs` lines 196, 304, and 361, `admission.rs` lines 201-213, `dispatch.rs` lines 136-155, `chain_record.rs` lines 198 and 216, `chain.rs` lines 111 and 281, `drive.rs` line 225, `apply.rs` line 289, `chat.rs` lines 105-155 (whatever Step 1 left).
  - Test support: `test_support/recording/observation.rs` line 211 and `test_support/recording/forward-tests-variants.rs` line 159 drop the origin field.
- Task notices: Step 1 removed the scheduler's notices queue, so nothing produces `Event::TaskNotice` or calls `Emitter::task_notice` any more. Delete both from `crates/runvil-internal/types/src/` with their tests and docs, and remove them from the facade listing. Workshop's typed readers (`crates/workshop/agents/src/tee.rs` line 52, `crates/workshop/server/src/agents/socket_frames.rs` line 86, `crates/workshop/server/src/agents/status.rs` line 80) already skip an entry that fails to decode, so an old log's `task_notice` entry is skipped; keep that behavior and add no shim.
- Facade: remove `TaskOrigin` from `crates/runvil/src/lib.rs` lines 64 and 68. Regenerate `crates/runvil/public-api.txt` with `cargo xtask api` on the nightly named in `crates/build-xtask/src/api/toolchain.rs`, with `CARGO_BUILD_WARNINGS` unset; the listing loses `TaskOrigin` and `Event::TaskStarted::origin`, and `cargo xtask api --check` on that nightly passes.
- Tests to update (drop origin fields and assertions only): `types/src/ids-tests.rs` line 81 (delete tests that exist only for `TaskOrigin`), `types/src/emitter-tests.rs` lines 117 and 137, `types/src/event-tests.rs` line 161, `lua/src/protocol/tests.rs` line 17, `lua/src/protocol/tests/parse_tasks.rs` (delete origin-only cases), `engine/src/lua/tests/shims.rs` lines 74 and 124, `engine/src/execute/tests/tasks.rs` lines 153 and 223, and any remaining `tasks.pending({ origin = ... })` call in a kept test, which becomes `tasks.pending()`.
- New tests:
  - Lua: `tasks.pending({})` and `tasks.pending({ origin = 'author' })` each raise `tasks.pending takes no arguments, got table`; `tasks.pending()` lists a pending task.
  - Lua: the table `tasks.status(id)` returns has no `origin` key and keeps every other key it has today.
  - Types regression (in `types/src/event-tests.rs`): a JSON `task_started` event containing `"origin":"author"` deserializes into `Event::TaskStarted`.
- Layout: apply the three-file rule to any group a deletion moves across the line; every Rust file stays at or under 500 lines.
- Acceptance and exit, all passing:
  - `git grep -n -E 'allow_tasks|allowed_tasks|TaskAllowlist|TaskBuiltin|task_allowlist|TaskOrigin|AwaitTasks|await_tasks|task_cancel([^l]|$)|queue_task_notice|advertise_task_builtins|answer_task_builtin' -- ':!vibe'` prints nothing, and no test, doc comment, or Lua shim mentions model-started tasks, the four tool names as tools, or task origin.
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features` and `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`.
  - `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`, `CARGO_BUILD_WARNINGS=deny cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`, and `cargo check -p gateway --no-default-features`.
  - `cargo fmt --all --check`.
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api` and `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`.
  - `cargo xtask api --check` on the nightly named in `crates/build-xtask/src/api/toolchain.rs`, with `CARGO_BUILD_WARNINGS` unset.
  - `cargo test -p build-xtask` and `node --test crates/workshop/ui/test/docs-claims.mjs`.
- Pre-existing failures, not this plan's: on this machine `npm test --workspace ui` fails the same 9 files at the baseline `fc4a1c0e` (agent-keybindings, editor-commands, files-actions, gateway-config-menu, menu-spec, quick-access-wording, window-chrome, workshop-layout, zoom), and `docs-claims.mjs` fails only on the stale `AGENTS.md` copies under the untracked `.claude/worktrees/` checkouts. A failure limited to those is pre-existing.

</step-2>

</execution-plan>
