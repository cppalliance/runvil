---
name: Vibelight Engine Cleanup
overview: Outline the runvil-internal/engine crate with vibelight, raise slop candidates from the outlines, verify every finding against the repository and docs, and deliver one self-contained Cleanup Plan.
todos:
  - id: step-1
    content: "Step 1: One map for the Pack roster"
    status: pending
  - id: step-2
    content: "Step 2: Delete Effect::Chat's options field"
    status: pending
  - id: step-3
    content: "Step 3: Infallible VM access and an Argv-carrying setup"
    status: pending
  - id: step-4
    content: "Step 4: One producer for the Pack order and self-evidencing tool-set errors"
    status: pending
  - id: step-5
    content: "Step 5: Root-chain constructor, Admission names, ChainOps state, scheduler doc text"
    status: pending
  - id: step-6
    content: "Step 6: One owner for the empty-reply message, and a narrowed test-support"
    status: pending
  - id: step-7
    content: "Step 7: Engine doc sentences, one file at a time"
    status: pending
  - id: step-8
    content: "Step 8: Section docs and the three import surfaces"
    status: pending
  - id: step-9
    content: "Step 9: Engine repository drift, two files"
    status: pending
  - id: step-10
    content: "Step 10: runvil-docs checkout drift, three files"
    status: pending
isProject: false
---

# Vibelight: runvil/crates/runvil-internal/engine

<product-contract>

## Product Requirements

### Scope

- **Crate:** `runvil-engine` at `crates/runvil-internal/engine`, outline 438 KB
- **Outlines:** current, reused as written, written 2026-10-27T12:09:24Z
- **Context:** 48 other outlines, not analyzed
- **Excluded:** 7 crates, all build tooling or test fixtures
- **Doc roots:**
  - `runvil-docs` checkout, latest edit 2026-10-27, 18 language chapters
  - the crate's `README.md` and `AGENTS.md`
  - the repository's `README.md`, `AGENTS.md`, and 264 `vibe/` records
- **Limits:**
  - the outline reports no test or bench declarations, so reachability is settled by search
  - the outline carries no `Unconnected types` section, so vestige leads come from `Used by` alone
  - 208 source files at 1769 KB, so analysis splits along module sections
  - `runvil-docs` is its own git checkout at `c:\Users\Vinnie\cursor\runvil-docs`, so a drift fix there is a separate commit

- **Stats:** 116 findings, 86 accepted, 30 rejected, 78 cheap, 8 substantial, 0 operator decisions outstanding.
- **Goals:**
  - Remove the state and duplication findings, which are the only items that change what the code can express.
  - Collapse the doc findings into one edit per file, with the exact replacement text.
- **Non-goals:**
  - No behavior change unrelated to a finding; no new abstraction to satisfy a doc.
  - No published-interface change, since every crate here is `publish = false`.
- **Success criteria:**
  - Every work item is done and the crate's own gate commands pass.
  - A fresh `vibelight` run shows each declaration and comment change and no new finding of the same kind.

## Functional Specification

### Debt Inventory

Verified so far: files 1, 2, 3, 5, and 6. File 4 and the doc drift pass are still being verified.

- **State:** `RunContext` keeps `roster` and `vendors` as two maps keyed by one `PackId`, where `vendors` is defined as the usable entries of `roster` and nothing holds them in step (`src/execute/config.rs:73`, `:248`). Consequence: a second writer or an edited filter stamps a vendor onto a tool-call event for a Pack the roster calls unavailable; both maps cross the crate seam into `runvil-lua::ToolSet` unchecked. (S2-10, S1-1)
- **State:** `Effect::Chat` stores the round's request settings twice, as `binding` and as `options`, where `options` is `binding.completion_options()` (`src/execute/run/effect.rs:127`). Consequence: `Effect::record` logs from `binding.invocation()`, so a caller-built effect logs a request nobody sent, in the structure a replay compares against. (S2-9)
- **State:** three root-chain constructions fill one nine-field `ChainStart` with four constants repeated, and neither the type nor the function states whether a root start enqueues itself (`src/execute/scheduler/chain.rs:35`). Consequence: `start_root_walk` and `end_h1` push their own chain while `start_h1` returns an index for `drive::start` to push, and the two multi-line sites differ in six fields with nothing naming which differ by rule. (S3-10)
- **State:** `SectionContext::vm` is an `Option` whose `None` no live frame can reach, so five error branches promise a failure that cannot happen (`src/execute/section_context.rs:126`, `:190`). Consequence: four crate-visible signatures carry a `Result` whose only error is a scheduler bug, and `tool_call_counts` repeats the check by hand with the same message. (S5-1)
- **State:** `SectionVmSetup` carries `argv` and `argv_writable` as separate fields, rebuilding at the VM boundary the `Argv` enum that already means that pair (`src/execute/section_vm.rs:55`, `:58`). Consequence: H1's writability rests on a mutation applied after the constructor returns, and nothing stops a writable value reaching a non-H1 section. (S5-2)
- **State:** `Error::ToolSets` pairs a `SetRule` with a `Vec<SetTool>` whose cardinality is a rule-dependent fact, so the type admits `RenderedPacks` carrying tools and a render rule carrying none (`src/error.rs:157`). Consequence: a shape the renderer never builds still renders a message with its subject missing. (S1-2)
- **Duplication:** one Pack order is kept twice, `SectionOffers::packs` and the local list inside `offered_packs`, and the second is discarded for a `sort_by_key` on a `position()` lookup whose `None` sorts first (`src/execute/scope.rs:63`, `:111`). Consequence: a Pack missing from the stored list moves to the front of what a render is asked, the mismatch the merge's first rule exists to catch. (S5-3)
- **Duplication:** the empty-reply message is maintained in two crates under a "change together" note (`src/test_support/scripted-chat.rs:23`, `harness-gateway-client/src/wire/parse.rs:35`), and the two copies have already diverged on whitespace-only reasoning. Consequence: the Engine copy treats `Some("  ")` as present and the gateway copy as absent, so the same scripted reply produces a different message than the reader it mirrors. (S6-2, V6-1)
- **Duplication:** `RunResult`'s `# Outcomes` list restates all three variant docs and has already drifted from the one it restates (`src/execute.rs:101`). Consequence: the variant doc a caller reads while matching is the weaker copy. (S1-17)
- **Duplication:** `ChainOps` holds its own `RunState` reference beside the chain's fork of the same state, and both reach the prompt through a wrapper (`src/execute/scheduler/chain_ops.rs:35`). Consequence: a future view method reaching for the view's copy silently reads the scheduler's `args`, `argv`, and reporting handles instead of the chain's, and the compiler cannot tell the two apart. (S3-25, S3-8)
- **Vestige:** `ScriptedReply::Failure` carries a `finish_reason` no constructor in the workspace ever sets, so the three lines that read it cannot run (`src/test_support/scripted-chat.rs:130`). (S6-6)
- **Naming:** `OutOfScopeToolCall::in_scope` holds the Static Set only, and its message labels it "in-scope aliases" (`src/error.rs:235`, `:239`). Consequence: a model reading the refusal to correct itself is shown a list labelled complete that omits the Dynamic Set, while also containing the two discovery tools. Seven test sites read the field by name. (S1-16)
- **Naming:** `Admission::release`, `forget`, and `park` name one state change three ways, and `released` names the opposite relation (`src/execute/scheduler/admission.rs:205`). (S3-11)
- **Visibility:** twelve items under `test-support` are public that only `#[cfg(test)]` suites use, where the bench names five (`src/test_support/scripted-chat.rs:26`). (S6-7)
- **Visibility:** `RunState::limits` is `pub(crate)` among `pub(super)` siblings (`src/execute/context.rs:243`). (S2-6)
- **Doc text**, by file:
  - `src/execute.rs`: the `runvil_engine` layout line says "journaled" for bindings no event carries (`:12`); `RunResult`'s doc leaves a declining prompt's mechanism unstated (`:96`) and gives the `#[non_exhaustive]` attribute no reason beside three closed peers (`:111`). (S1-5, S1-11, S1-12, S1-17)
  - `src/execute/bindings.rs`: "journaled" twice (`:1`, `:18`); `model`'s doc says `None` means the run may not use the model, when a UI snapshot reaches catalog models by raw id (`:46`). (S1-5, S1-3)
  - `src/execute/config.rs`: the `name` field doc describes the job of the field it is moved into (`:47`); `uid`'s purpose names three consumers that do not exist (`:362`); `model_bindings` overstates its set (`:334`). (S1-4, S1-14, S1-3)
  - `src/error.rs`: the module doc lists `CompletionError` as a wrapper of this type and describes the cause rule over a shape three transparent variants do not have (`:4`, `:10`); `Store`'s concrete cause departs from the `BoxedSource` rule with no reason (`:352`); `UnsupportedVersion` never names the supported major (`:267`); `LuaQuota` names two of three resources (`:322`); `DepthVia` splits the reported-name rule across two sentences (`:44`). (S1-6, S1-8, S1-7, S1-9, S1-10, S1-18)
  - `src/error/convert.rs`: the module doc names the gateway client, a crate the boundary forbids, and `model.rs` sends the transport codec to a crate that has none (`:1`). (S1-13, S6-3)
  - `src/execute/config-limits.rs`: `lua_log_events` does not say the byte ceiling is derived from it (`:34`). (S1-15)
  - `src/execute/requirements.rs`: `merge` promises skipping that `unmet_models` does not do (`:72`); the `UsablePack` doc promises a vendor every tool event records, which fails for a Pack listed in both roster lists (`:174`); the notice doc says every line carries required versus actual, which only model lines do (`:110`). (S2-1, S2-3, S2-13)
  - `src/execute/context.rs`: the fork list and `with_args` name `call` alone, though a `tasks.spawn` or fanout arm with an input takes the same fork (`:50`, `:316`). (S2-11)
  - `src/execute/context-bound.rs`: the module doc says `RunState::new` assembles its pieces once per run, when `derive_argv` re-runs on every args fork (`:1`); the role-binding list omits the declared description and context window (`:40`). (S2-5, S2-12)
  - `src/execute/error.rs`: the module doc names two of its three exports (`:1`); `RunErrorKind::Quota` and `Lua` name too few members (`:39`); `originating_error` lists four cause kinds of eight (`:187`); `Parse` still claims to cover a compiled Lua region, which now classifies as `Lua` (`:16`). (S2-7, S2-2, S2-8, V2-1)
  - `src/execute/infer.rs`: the report path names a `round` item that does not exist, where `turn::report_model_turn` is meant (`:70`). (S2-4)
  - `src/execute/run/effect.rs`: `Timer`'s field doc does not say the record stores fractional seconds (`:205`). (S2-14)
  - `src/execute/section_context.rs`: the no-derives note claims neither `Clone` nor `Debug` is available, when only `Clone` is blocked (`:120`); the `sys` field doc restates the teardown reporting rule `SectionSys::section_name` owns (`:130`). (S5-4, V5-2)
  - `src/execute/section_vm.rs`: the fixed-sequence list omits the raw-model-id opt-in, the one step that must land before injection (`:94`). (S5-6)
  - `src/execute/section_context-construct.rs`: the module doc's delta list is short by two against `new_h1`'s (`:6`). (S5-7)
  - `src/heading_address.rs`: `Visible::of` promises a fallback its own `debug_assert!` forbids, so the promise is true in release and panics in debug (`:152`); the not-found rule is stated on `not_found` and restated in two `# Errors` paragraphs (`:392`, `:404`). (S5-5, S5-8, V5-1)
  - `src/subst.rs`: the `NullValue` variant states a null rule that `{{ item }}` does not follow, and the `# Errors` prose duplicates the ten `SubstErrorKind` variants one for one (`:54`, `:156`). (S6-4, S6-5)
  - `src/model.rs`, `src/lua.rs`, `src/parser.rs`: each calls itself the import surface for its dependency while production code bypasses it, and the exception set has at least five groups rather than the two the docs name (`model.rs:1`, `lua.rs:1`, `parser.rs:1`). (S6-1)
  - `src/execute/scheduler/chain.rs`: `finish` gives delivery as a closed three-way split though a `Determinism` outcome ends the run first, and states the teardown flatly though an H1 frame drops unarmed (`:100`); `start_chain` claims the `0.1` entry rule it only copies (`:64`). (S3-1, S3-6)
  - `src/execute/scheduler/h1.rs`: `end_h1`'s `# Errors` omits the gone capability and the chain-count cause its body raises (`:68`). (S3-2)
  - `src/execute/scheduler.rs`: `prompt_origin` defines `label` as a section or pass where one caller passes a tool id (`:130`); the `scope` field's single-writer rule is unstated where both writers assign unconditionally (`:181`). (S3-3, S3-8)
  - `src/execute/scheduler/task_end.rs`: `end_backing` omits the queued-chain silence its body enforces (`:155`). (S3-4)
  - `src/execute/scheduler/admission.rs`: the ordering rule is stated at four sites and the VM invariant at one, far from `plan`, which makes both true (`:150`, `:323`, `:349`). (S3-5)
  - `src/execute/scheduler/walk.rs` and `chain_record.rs`: the `0.1` entry rule is stated four times and enforced by two literals, and `Chain::counters` is its normative home (`walk.rs:31`, `chain_record.rs:255`). (S3-6)
  - `src/execute/scheduler/chain_ops.rs`: the module doc names five of eight files holding `impl` blocks, omitting its own (`:7`). (S3-7)
  - `src/execute/scheduler/drive.rs`: the stall test names three conditions where the guard tests two (`:7`). (S3-11)
  - `src/execute/scheduler/step.rs`: the module doc and `step_chain`'s doc both restate the four advance cases `walk` owns (`:1`, `:121`). (S3-9)
  - `src/execute/scheduler/chain_record`: `ChainArena`'s doc says it holds at most `u32::MAX` chains, which a `#[cfg(test)]` field makes false in test builds (`arena.rs:91`); `ChainIndex` says only `push` creates one, which a `#[cfg(test)]` constructor also falsifies (`arena.rs:14`); `Entered`'s teardown clause reads as belonging to the abort alone (`:5065` outline); `Chain::access`' `# Errors` names only chain-end paths though two teardown probes skip on it (`:4897`); `entered_name`'s three-way storage and standing are unstated (`:4864`). (S4-5, S4-6, S4-4, S4-9, S4-28)
  - `src/execute/scheduler/chat.rs`: `overflow_result` says no round ran, false on the provider path (`:5460`). (S4-11)
  - `src/execute/scheduler/dispatch.rs`: the index sentence accounts for eleven of sixteen requests and omits the three this file implements (`:5555`); the module doc says no arm emits for an answer, which two refusing arms break (`:1`); `dispatch_vfs`' doc restates the module's leaf-yield rule (`:238`). (S4-13, V4-2, V4-1)
  - `src/execute/scheduler/effects.rs`: `orphan` and `clear_withdrawn` leave their contracts to a caller's comment (`:5701`, `:5731`). (S4-14, S4-15)
  - `src/execute/scheduler/tasks.rs`: `Delivered`'s rationale is written twice with its subject changed (`:99`). (S4-2)
  - `src/execute/scheduler/tool_call`: `Listed::input_schema`'s presence rule lives in `search` and not on the field (`discovery.rs:6603`); the adhoc handshake is written in the parent and stubbed in the child that implements it (`tool_call.rs:6424`); the discovery child restates the parent's routing clause and buries its own guarantee (`discovery.rs:6488`). (S4-20, S4-23, S4-24)
  - `src/execute/scheduler/waits.rs`: `live_of` is the narrower query under the shorter name (`:6094`). (S4-3)

**Report defects in the vibelight output:** `ChatOutcome` is absent from the outline's External items although `lua.rs` re-exports it; the outline prints no test or bench declarations, so `ChainArena`'s `#[cfg(test)] max_chains` field is missing from its `Counts:` line and `ChainIndex::for_test` is invisible.

**Rejected candidates:** 32. Fourteen are structural leads that died on a specific rule the code states (`Continuation::Infer`/`Chat` route to different accept paths; `ChainStep`'s named type documents three `RunEnded` producers; `Admission::plan`'s clones are its rollback; `Settled`'s split ends a borrow early; `ChainOps`' spread blocks are the module's stated rule; the three admission queues carry different records; settle-then-join ordering is documented; `settle_all_tasks`' skip is the one-terminal guard; `end_tool_call`'s `&self` is what lets an abort caller hold a chain borrow; `end_section`/`apply_jump` diverge immediately after the shared lines; `GENERIC_COMPLETION`'s three sites share one const; `h1`'s delta list must restate what it negates; `Chain::seed` and `var` differ in kind and lifetime; `Parked`'s empty states are load-bearing). Ten were extinguished as weak on the "has a consequence" test. Four were extinguished because the source refuted them: `Running::answer` and `Entered::running` are not exclusive, since `answer: None` is the ordinary suspended state; `ArrivedRound` is built and consumed inside one function, so no section entry can intervene; `TaskState`'s narrowing remedy does not compile, since a `use` cannot widen `pub(super)`; and `Chain::entered_name`'s module doc is a summary that already omits six other fields. The rest are duplicates of a kept finding.
**Documentation drift:** five passages, all doc-side fixes, each checked against the source they describe:

- `crates/runvil-internal/engine/AGENTS.md:4` says every store view is derived "at dispatch", but a section's `store` table gets one view at VM install (`crates/runvil-internal/lua/src/engine_globals-store.rs:258`), which is what lets a prelude's install-time store call issue no effect. The rule needs both derivations named. (S7-1)
- `runvil-docs/src/language/01-what-a-prompt-is.md:604` says every store call is Harness work, contradicting `12-tools.md:2767`, which describes a prelude's install-time calls running directly. The shim install order at `src/execute/section_vm.rs:196` settles it in favor of `12-tools.md`, so the absolute in chapter 1 is the wrong side. (S7-2)
- `README.md:59` lists four effect kinds and omits the Pack render; `Effect` has five, and `AGENTS.md:9` and `runvil-docs/src/introduction.md:14` both already name all five. It also says "a tool result", where only a catalog tool call is answered by the caller. (S7-3)
- `runvil-docs/src/language/16-limits-and-errors.md:1078`'s table lists twelve of thirteen `CompletionErrorKind`s, missing `InvalidParts`, whose tag reaches a run record as `invalid_parts`. (S7-4)
- `runvil-docs/src/language/12-tools.md:2389` says the validation events fire once per section entry, but every freeze attempt reports its own pair (`src/execute/scope.rs:148`), and a failed freeze attaches nothing, so a later round renders and validates again. The same claim appears at `12-tools.md:915` and `01-what-a-prompt-is.md:600`. (S7-5)

</product-contract>
<implementation-contract>

## Technical Design

- **One map for the Pack roster.** `roster` and `vendors` collapse into `BTreeMap<PackId, RosterEntry>` with `Usable { vendor }` and `Unavailable { reason }`, which makes the subset rule unrepresentable. The cost is a `runvil-lua` interface change, since `ToolSet` accepts the pair: the same one-map shape there, and `bound_tool_set` and `vendor_for` read one map. [2026-10-26-1-engine-slop-cleanup.md](runvil/vibe/2026-10-26-1-engine-slop-cleanup.md) deferred exactly this and fixed only the disagreement, so the plan finishes that record's own target. (S2-10, S1-1)
- **The Pack order's single producer.** `offered_packs` takes each Pack's slot up front from `offers.packs` and fills the sets as it walks the tools, so the throwaway local order and the `sort_by_key` both go. The alternative, keeping the body and adding an invariant sentence, leaves two producers of one order and was rejected. The existing order test at `src/execute/scope-tests-render.rs:33` pins the behavior a rewrite must keep. (S5-3)
- **Rule-dependent cardinality.** `NonEmpty<SetTool>` already exists in `runvil-lua`, is exported, and is imported by this crate, so the `ToolSets` split costs no new collection type. `RenderedPacks` moves to a sibling variant of a new private enum carrying only `asked` and `answered`, leaving the five rule variants that name tools. (S1-2)
- **Deleting `Effect::Chat::options`.** The field was added for a per-round UI model substitution, but the one consumer that performs it works on its own copy of the binding, which travels beside the field anyway. The performer calls `binding.completion_options()` where it read the field, the record keeps its shape because it already records from `binding`, and the divergence becomes unrepresentable. `Effect` is deliberately closed, so a hand-built effect can disagree today. (S2-9)
- **One root-chain constructor.** A `start_root_chain` helper fills the four constants `ChainStart` repeats across the three root sites. Its parameters must carry `start` and `concurrency`, because `end_h1` inherits the pass's concurrency when `tasks.concurrency` lowered it. The enqueue question stays open: two sites push their own chain and one returns an index for `drive::start` to push, and the helper's doc states whichever convention is chosen. (S3-10)
- **Narrowing `test-support`.** Every item narrowed to `pub(crate)` is used only under `#[cfg(test)]`, which `pub(crate)` reaches, and the bench is a separate crate, so the compiler proves the list: a bench-time item narrowed wrongly fails the bench build. This is a surface change counted cheap only because the crate is `publish = false` with every user in-repo. (S6-7)

### Step sizing

The operator requires between five and ten steps total. The work divides naturally into one substantial cluster and a large body of cheap doc sentences, so size the steps by cause rather than by file count:

- Group the cheap doc items into a few steps by crate area, not one step per file and never one step per finding. A step may carry thirty replaced sentences.
- Give each substantial item its own step, or pair two that touch disjoint files and land cleanly together.
- Keep the five drift fixes in one step for the engine repository and one step for the `runvil-docs` checkout, since that is a separate git repository with its own commit.
- Never produce a step whose only content is a single sentence replacement.

</implementation-contract>
<verification-contract>

## Testing Plan

- `cargo nextest run --locked -p runvil-engine -p runvil-lua -p harness-gateway-client -p harness-runner`
- `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
- `cargo fmt --all --check`
- `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api` with `RUSTDOCFLAGS=-D warnings`
- `cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
- `cargo test -p build-xtask` and `node --test crates/workshop/ui/test/docs-claims.mjs`
- `cargo bench -p runvil-engine --features test-support --no-run`, which compiles the bench against the narrowed surface
- `cargo +<pinned nightly> xtask api --check` if any public surface moves
- A fresh `vibelight` run at the repository root, whose outlines show each declaration and comment change; nothing under `.vibelight/` is committed
- No new test is written for a doc-only item. Behavior items rely on tests that already exist:
  - `scope-tests-render.rs:33` pins the render order the `offered_packs` rewrite must preserve
  - `h1/pass.rs:68` and `exit_rules.rs:45` pin the `0.1` entry ids the `start_root_chain` helper must keep
  - `context-tests.rs:150` pins that a Pack in both roster lists records no vendor, from the side a one-map change must preserve
  - `effect-tests.rs:164` pins `{"Timer":{"seconds":0.5}}`, which the `Timer` doc change describes rather than alters
  - `happens_before.rs:433` pins the single root identity a double scope acquisition would break

</verification-contract>
<decision-record>

## Decision Record

- Design records settle four questions the analysis raised. `roster`/`vendors`: the disagreement is fixed and the one-map shape is deliberately deferred. `Effect::Chat::options`: added for a substitution its lone consumer performs on its own copy. `RunResult`'s `#[non_exhaustive]`: deliberate, with its reason unrecorded at the site. A decline is a `Completed` value, never a thrown error. Each site gets the reason its record already holds.
- The empty-reply copy is the one finding that changes a crate this plan does not own. `harness-gateway-client` reaches `CompletionError` through the `runvil` facade rather than depending on `runvil-model-client`, so a single owner is callable from both with no new dependency; but the remedy adds a method to a type the facade publishes, which a prior pass declined for that reason.
- `SectionContext::vm` keeps its `Option`. The `Drop` implementation needs `take()` to place the VM's destruction between the teardown pair, and removing it was rejected before. Only the accessors change.
- Reversal costs: every item is inside a `publish = false` crate. The one cross-crate addition is additive and non-breaking, and deleting it later would cost more than reverting it.
- The `runvil-docs` guide is treated as fact for author-facing behavior. All five drift passages it disagrees on were doc-side, so the guide changes and the code stays, except where the guide's own chapters contradict each other, which `12-tools.md` settles.
- **Operator answers, all three (a):** add `CompletionError::empty_reply` to `runvil-model-client`; narrow the twelve `test-support` items; have `start_root_chain` enqueue and stop `drive::start` pushing.
- Risk: the `offered_packs` and `start_root_chain` items rewrite working code whose current behavior is pinned by tests, so both must land against a green suite. Neither is a doc edit despite riding beside many.

### Open Questions

None. Each was answered by the operator, and the answers are folded into the Decision Record and the work items they affect.

### Deferred and Out of Scope

- The drift verifier was stopped short of a written report. Each of the five findings was checked against source directly, which is what the entries above cite, so nothing rests on an unverified claim.
- `Chain::entered_name`'s three-way storage is deferred as a type change. The copies serve different lifetimes, and removing one changes signatures without removing a state; the doc items in this plan state its standing instead. Revisit when a change touches section teardown.
- `SectionPos`' position-past-slice case and `Visible::of`'s profile-dependent promise: only the doc is fixed. A test for the release-path fallback is out of scope because the fallback is untested today and the `debug_assert!` is the intended contract.
- `ChatOutcome::Overflow`'s doc in `runvil-lua` repeats the "no round ran" clause this plan fixes in `chat.rs`. That crate is outside the target, so the sentence stays until a run that owns it takes the same edit. Revisit with any change to the overflow vocabulary.
- `MAX_CALL_DEPTH` bounds `call_children`'s cost at eight passes per abort, so no complexity note is written on a private method for an observation that cannot change a decision.
- Slop the verifiers noticed but no candidate owned: none reached the standard, so nothing is parked here.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p gateway` for the gateway and `cargo build -p workshop` for the desktop app; a bare `cargo build` builds only `default-members`, which is `crates/gateway/app`, and CI builds `cargo build --locked -p gateway --no-default-features` as the headless shape.
- Focused test command pattern: `cargo nextest run --locked -p <package> -E 'test(<substring>)'`, narrowed the way CI narrows its Windows job, e.g. `cargo nextest run --locked --no-default-features --features gateway/test-fixtures -p gateway -E 'package(gateway) & binary(it) & test(<name>)'`.
- Component test command pattern: Engine and plugins `cargo nextest run --locked -p runvil -p runvil-engine -p runvil-parser -p runvil-lua -p runvil-vfs -p runvil-types -p runvil-model-client`; Harness `cargo nextest run --locked -p harness -p harness-runner -p harness-plugin-loader`; Gateway `cargo nextest run --locked -p gateway`; Workshop partition `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api -p workshop-workspace`; Workshop UI `npm test --workspaces --if-present` in `crates/workshop`; config UI `npm test` in `crates/gateway/config-ui/ui`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, plus `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api` for the Workshop crates. Structural checks run separately as `cargo test -p build-xtask` (product and container boundaries, Workshop tier graph, `## Invariants` marker, lint inheritance) and `node --test crates/workshop/ui/test/docs-claims.mjs` (Engine wording, rulebook Definitions, Pack lifecycle wording). nextest settings live in `.config/nextest.toml`, which caps the whisper suites in a `heavy` group and extends timeouts for the ignored native realtime speech tests.
- Linter command: `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` with `CARGO_BUILD_WARNINGS=deny`, and `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets` for the Workshop crates. Do not pair either with a standalone `cargo check --workspace`; the sole exception is `cargo check -p gateway --no-default-features`. UI type checks are `npm run typecheck` in `crates/workshop` and `crates/gateway/config-ui/ui`.
- Formatter check command: `cargo fmt --all --check` (rustfmt `style_edition = "2024"`). For the TypeScript and CSS packages there is no formatter or lint config in the tree: the only frontend config files are `tsconfig.json` in `crates/workshop/ui`, `crates/workshop/look`, `crates/workshop/platform`, and `crates/gateway/config-ui/ui`, so the frontend gates are `typecheck` and `test` only.
- Docs command: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, then the facades with default features (`cargo doc -p runvil --no-deps`, `cargo doc -p harness --no-deps`) and `cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`. The mdBook sources under `guide/books` build through `MDBOOK=<path> cargo xtask site [--books-only]`, and the facade surface check is `cargo +<pinned nightly> xtask api --check` with `CARGO_BUILD_WARNINGS` unset.
- Test placement and naming conventions: unit tests sit inline behind `#[cfg(test)]`, declared as `mod tests;` pointing at a sibling `tests.rs` or a `tests/` subdirectory, or as an inline `mod tests { }` block near the code they cover. Integration tests live in the crate's `tests/` tree as one entry-point binary per suite: `crates/runvil/tests/suite/main.rs` and `crates/harness/tests/suite/main.rs` declare `mod` files beside them, and `crates/gateway/app/tests/it/main.rs` does the same under `it/`, with subdirectories such as `boot/`, `chat/`, `realtime_stt/`, `speech/`, and `support/` grouping the modules. Test functions are long snake_case sentences stating the behavior and result, for example `answering_a_batch_in_reverse_gives_the_same_result_as_issue_order` and `each_effect_and_its_answer_log_as_one_json_line_that_parses_back`, and most return a `TestResult` alias rather than `()`. Prompt fixtures live in `crates/runvil/tests/prompts/{valid,invalid,execution}/*.md` and `crates/runvil-internal/engine/tests/prompts/`, and tests embed them with `include_str!`. Shared scaffolding sits in a `support.rs` module per suite. `crates/workshop/ui/test/*.mjs` holds the Node test files, each named for its subject.
- Directory map: `crates/` holds the whole Rust workspace plus the `shared-ui` TypeScript package and the `build-*` tooling crates (`build-ceiling`, `build-ui`, `build-user-guide`, `build-workshop`, `build-xtask`, `build-llama-cuda`). `crates/runvil` is the public facade; `crates/runvil-internal/` holds the private Engine crates (`engine`, `parser`, `lua`, `types`, `vfs`, `model-client`). `crates/harness` and `crates/harness-gateway-client` are the Harness facades, with `crates/harness-internal/` holding `runner`, `plugin-loader`, and the `test-plugin` fixture. `crates/plugin-*` are the four Plugins, and `crates/runvil-plugin` is the Plugin contract. `crates/gateway/` holds the gateway family (`app`, `config`, `config-ui`, `local`, `logging`, `progress`, `protocol`, `routing`, `web-search`, and the nested `stt/{api,engine,backend-whisper,whisper-ffi}`), with `crates/gateway-api-types` and `crates/gateway-api-discovery` beside it. `crates/workshop/` holds the desktop app (`desktop`, package `workshop`), `server`, `server-api`, `ui`, `look`, `platform`, `agents`, `gateway`, `menu`, `protocol`, `registry`, `run-log`, `status`, `support`, `user-state`, and `workspace`. `crates/shared-error-source` and `crates/shared-loopback` are shared leaves. Outside `crates/`: `guide/` holds the mdBook `books` (`gateway`, `language`, `workshop`), `chrome`, and `landing`; `prompts/` holds shipped prompt files; `tools/` holds the Node scripts `stage-gateway-sidecar.mjs` and `gateway-tts-live.mjs` with their tests; `vibe/` holds dated change notes by month plus a `scratch` area; `.vibelight/` holds generated crate outlines (one markdown file per crate plus `index.md`); `local/` holds operator-local config, prompts, profiles, and STT fixtures; `images/` holds artwork; `.config/`, `.cargo/`, `.githooks/`, and `.github/` hold nextest config, cargo aliases and target flags, git hooks, and CI workflows.
- Component boundaries: seven terms each have exactly one meaning and are always capitalized, Engine (the `runvil` and `runvil-*` crates), Harness (the `harness` and `harness-*` crates), Host (an application such as Workshop that makes every policy decision), Pack, Plugin, Static Set, and Dynamic Set. Dependency direction runs one way. Engine crates depend only among themselves, with `runvil-engine` on `runvil-lua` and `runvil-parser`, and `runvil-parser` on `runvil-lua`; the `runvil` facade re-exports all six internal crates. Plugins depend only on the `runvil-plugin` contract. The Harness facade depends on `runvil`, `runvil-plugin`, `harness-runner`, and `harness-plugin-loader`. Gateway crates stay internal to their family, with `gateway` on `gateway-local`, `gateway-routing`, `gateway-stt`, `gateway-web-search`, and the optional `gateway-config-ui`, and `gateway-stt-backend-whisper` on `gateway-stt-engine`. Workshop tiers upward through `workshop-support` and `workshop-server` into `workshop-server-api`, and the desktop app depends on the facade only, never on `workshop-server` directly. Engine crates never depend on `tokio`, `tokio-util`, `async-trait`, `reqwest`, `getrandom`, or `fastrand`, and Engine code never names the Host; `cargo test -p build-xtask` enforces these boundaries and the tier graph, while `crates/workshop/ui/test/docs-claims.mjs` enforces the wording rules.
- Conventions summary: edition 2024 everywhere, rustfmt `style_edition = "2024"`, a pinned `1.99` toolchain with a `1.95` MSRV for the Engine crates. Lints are inherited from the workspace root and are strict: `all` and `pedantic` denied, `unsafe_code` and `unsafe_op_in_unsafe_fn` denied, `unwrap_used` and `expect_used` denied (with `allow-unwrap-in-tests` and `allow-expect-in-tests` set in `clippy.toml`), `todo`, `exit`, `dbg_macro`, `allow_attributes`, and `allow_attributes_without_reason` all denied, and process-global installers such as `set_hook`, logger installation, and `CryptoProvider::install_default` banned outside a binary entry point. `missing_docs`, `missing_debug_implementations`, and `unreachable_pub` warn. Every crate doc carries a `## Invariants` section. Source directories stay flat by default: a subdirectory needs at least three files, and a two-file group lives as `foo-bar.rs` siblings wired with an explicit `#[path]`, converting in either direction at the three-file line. Comments explain non-obvious constraints and cite an upstream issue URL for every workaround. Error and status text is written for a model to read: concise, factual, naming required versus actual. Behavior changes ship with tests in the same change. Builds and tests must not write into the repository, and CI fails on a dirty tree. Workshop UI CSS uses only the custom properties from `@workshop/look` and `tokens/component.css`, and every persisted value goes through the `ui-storage` adapter into one of two homes, `ui-state.json` for account state or the workspace file for workspace state. Cargo aliases are `cargo xtask`, `cargo workshop`, and `cargo vibelight`.

</project-survey>
<execution-plan>

## Execution Instructions

Ten steps, one commit each. Order is by cause, not by file count. The Pack roster moves a `runvil-lua` interface that every later behavioral test links against, so it goes first; the doc-only steps come last so a doc edit never hides inside a code commit. Substantial items that touch one shared file share a step, because a step is one commit. Doc-only sentences are grouped by crate area, never one step per finding.

**Line numbers are locators, not guarantees.** Steps 1 through 6 edit source that Steps 7 and 8 also describe, so every line number in those doc steps was captured before this plan was written and may have moved. Confirm each sentence by its text before replacing it, and if a sentence is not there, report it rather than guessing.

<step-1>

### Step 1: One map for the Pack roster [completed]

- Component: Pack roster
- Tests: `cargo nextest run --locked -p runvil-engine -p runvil-lua`

**Artifacts.** In `crates/runvil-internal/engine/src/execute/config.rs`, add a private enum `RosterEntry` with two variants, `Usable { vendor: String }` and `Unavailable { reason: String }`, each documented with one sentence. Replace the `roster` and `vendors` fields with one `packs: BTreeMap<PackId, RosterEntry>`. Rewrite `with_packs` to fold both argument lists into that map with the unavailable entry winning, keeping its public signature and its three doc paragraphs minus the vendor/roster split. In `src/execute/context-bound.rs`, `bound_tool_set` takes the one map. In `crates/runvil-internal/lua/src/handles.rs`, `ToolSet` holds the one map and `vendor_for` answers from the entry.

**Fixtures to update.** In the `runvil-lua` crate: `handles-tests.rs:76`, `test_vm.rs:117` and `:135`, `packs-tests.rs:25`, `tools/tests-parameters.rs:44`, `tools/offer.rs:153`. In the engine crate: `lua/tests.rs:63`, `lua/tests/globals.rs:55`, `execute/tests/tool_scoping.rs:26`.

**Test case.** Write the new test before the change and keep it green after. A Pack listed in both roster lists records no vendor on the tool-call event; `context-tests.rs:150` already asserts that from the roster side, so extend it to assert the one-map shape answers with the `Unavailable` reason rather than a vendor. Expected fact, not just no-panic: the event's vendor field is absent and the reason string is the one `with_packs` was given.

**Acceptance.** The subset rule becomes unrepresentable. The cost is one `runvil-lua` interface change, since `ToolSet` accepted the pair; that change is deliberate and settles the deferral recorded in `vibe/2026-10-26-1-engine-slop-cleanup.md`, which fixed only the disagreement. No new dependency is added on either side of the seam.

**Flags.** This is the plan's one cross-crate interface change. It is inside `publish = false` crates and additive in information, so nothing is published from it. Findings: S2-10, S1-1.

</step-1>

<step-2>

### Step 2: Delete `Effect::Chat`'s `options` field [completed]

- Component: Engine state
- Tests: `cargo nextest run --locked -p runvil-engine -p harness-runner -p harness-gateway-client`

**Artifacts.** In `src/execute/run/effect.rs`, remove the `options` field from the `Chat` variant and its doc line "The per-request completion options, built from `binding`". Reword the variant doc's "using the frozen `options` built from `binding`" to "using the options `binding` freezes". Drop the `options: binding.completion_options(),` line at `src/execute/scheduler/chat.rs:180` and `src/execute/scheduler/dispatch.rs:222`.

**Performers.** Each performer reads `binding.completion_options()` where it read the field, so the request sent is unchanged: `src/execute/test_support/scenario-driving.rs`, `src/test_support/scripted-chat.rs`, the two test helpers at `src/execute/run/tests.rs:48` and `effect-tests.rs:33`, plus `benches/models_loop.rs`, `crates/harness-internal/runner/src/effect_loop.rs`, and `crates/harness-gateway-client/src/wire/request-tests.rs`.

**Acceptance.** The record keeps its shape because `Effect::record` already logs from `binding.invocation()`, so a caller-built effect can no longer log a request nobody sent. `Effect` is deliberately closed, so the divergence is unrepresentable after this, not merely unused. The record tests at `effect-tests.rs:53`, `:96`, and `:164` must pass unchanged; that is the proof the logged shape did not move, including `{"Timer":{"seconds":0.5}}` at `:164`, which this step does not alter. Findings: S2-9.

</step-2>

<step-3>

### Step 3: Infallible VM access and an `Argv`-carrying setup

- Component: Engine state
- Tests: `cargo nextest run --locked -p runvil-engine -p runvil-lua`

**Part one, `src/execute/section_context.rs`.** Change `pub(super) fn vm(&self) -> Result<&SectionVm>` to `pub(super) fn vm(&self) -> &SectionVm`, returning `self.vm.as_ref().expect("a live frame holds its VM until drop")`. Add a doc sentence stating that only the frame's own `Drop` clears the slot. Delete the `Error::Internal` clause from the `# Errors` paragraphs of `vm`, `read_var`, `read_argv`, and `install_lazy_prose`, and delete the by-hand `let Some(vm) = vm.as_ref() else { ... }` in `tool_call_counts`, calling `self.vm(..)` instead. The internal uses at `:162`, `:173`, and `:214` become `self.vm.` with no error mapping.

**Call sites that drop `?`.** `scheduler/step.rs:78`, `:161`, `:177`; `scheduler/tool_call.rs:178`, `:217`, `:367`; `scheduler/tool_call-discovery.rs:87`, `:153`; `scheduler/chat.rs:105`, `:220`, `:282`; `scheduler/dispatch.rs:212`. Leave `read_var`'s callers on `Result`, because it still reports the Lua error: `scheduler/chain.rs:137`, `walk.rs:166`, `walk.rs:193`, `step.rs:108`, `h1.rs:79`.

**Part two, `src/execute/section_vm.rs`.** Replace the `argv: Option<&'a Value>` and `argv_writable: bool` fields with one `argv: crate::lua::Argv<'a>` and delete the `:135-139` rebuild, passing the field straight to the VM. `RunState::vm_setup` at `src/execute/context.rs:357` takes the writable flag and builds the `Argv` once, so the decision is made at construction. `SectionContext::new_h1` at `src/execute/section_context-construct.rs:193` stops mutating the returned struct and passes the writable flag to `vm_setup` instead. Update the two test helpers at `lua/tests.rs:88` and `:126`, and the five loops at `lua/tests/globals.rs:105`.

**Acceptance.** `SectionContext::vm` keeps its `Option`; the `Drop` implementation needs `take()` to place the VM's destruction between the teardown pair, and removing it was already rejected. Only the accessors change. No test asserts the deleted `Error::Internal` arm, which is why it can go. A writable `Argv` can no longer reach a non-H1 section, and H1-only writability stays pinned end to end by `execute/tests/suite/args_surface.rs:154` and `suite/global_metatable.rs:23`, both unchanged. Findings: S5-1, S5-2.

</step-3>

<step-4>

### Step 4: One producer for the Pack order and self-evidencing tool-set errors

- Component: Offers and errors
- Tests: `cargo nextest run --locked -p runvil-engine`

**Pack order, `src/execute/scope.rs`.** Rewrite `offered_packs` to insert one slot per Pack up front in `offers.packs` order and fill each slot's sets while walking the tools. Delete the local order list it builds today and the `sort_by_key` at `:131`. Document on `SectionOffers::packs` that it is the only copy of that order. The alternative, keeping the body and adding an invariant sentence, leaves two producers of one order and was rejected; `scope-tests-render.rs:33` pins the asked order the rewrite must preserve, and `scope-tests.rs:57` with `scope-tests-render.rs:40`, `:82`, `:168` call the function directly.

**Tool-set evidence, `src/error/sets.rs`.** Move `RenderedPacks { asked, answered }` out of `SetRule` into a sibling variant of a new private enum `SetEvidence` whose other variant holds a `runvil_lua::NonEmpty<SetTool>` beside the `SetRule` that names it. Change `Error::ToolSets` at `src/error.rs:157` to hold that enum and make `sets_message` match without an empty case. Update the construction sites at `src/execute/scope.rs:244`, `:282`, `:292`, `:446`, `src/error/value.rs:41` and `:81`, `src/error/tests.rs`, and `src/execute/scope-tests.rs:289`. `NonEmpty` already exists, is exported by `runvil-lua`, and is imported through `src/lua.rs:7`, so no collection type is invented.

**Test case for the evidence.** No test constructs `RenderedPacks` with tools today; that shape is the defect. Add one that renders a rule-shaped message and asserts the subject is present, for example that a `RenderedPacks` message names both asked and answered counts and never renders with a missing subject.

**Rename, `src/error.rs` and `src/execute/scheduler/chat.rs`.** Rename `OutOfScopeToolCall::in_scope` to `static_aliases` and relabel the message tail to "aliases this section offers", followed by a clause naming the two discovery tools. Update the field declaration, the `Display` at `:235`, the construction at `chat.rs:443`, and seven readers: `tool_loop-scope-gate.rs:45`, `:79`, `:113`, `:189`, `offering-sets.rs:158`, `chat_arm.rs:225`, `error/tests.rs:99`. Rewrite the two `ends_with` assertions at `tool_loop-scope-gate.rs:57` and `:90` to the new tail.

**Acceptance.** The field name and the message agree: it holds the Static Set only, and the text no longer calls the list complete. The two discovery tools stay named in the message. Findings: S5-3, S1-2, S1-16.

</step-4>

<step-5>

### Step 5: Root-chain constructor, `Admission` names, `ChainOps` state, scheduler doc text

- Component: Scheduler
- Tests: `cargo nextest run --locked -p runvil-engine`

**Artifacts, `src/execute/scheduler/chain.rs`.** Add `start_root_chain(&mut self, kind: ChainKind, counters: Counters, ctx: RunState, access: Arc<Access>, start: SectionPos, var: Value, concurrency: NonZeroUsize) -> Result<ChainIndex>`. It fills `lineage: ChainId::root()` and `call_depth: CallDepth::ROOT` itself, pushes the chain onto the ready queue itself, returns its index, and states the enqueue convention in its doc. Have `start_h1` (`h1.rs:48`), `end_h1` (`h1.rs:108`), and `start_root_walk` (`walk.rs:56`) call it and drop their own `push`, including `drive::start`'s push at `drive.rs:137`. Keep `prepare_call` and `prepare_spawn` on `ChainStart`, so this is an addition and not a type split.

**Parameter rules.** `start` and `concurrency` must be parameters, because `end_h1` inherits the pass's concurrency when `tasks.concurrency` lowered it. The operator chose enqueue-in-helper, so the doc says a root start enqueues itself and no caller pushes.

**Artifacts, `src/execute/scheduler/admission.rs`, the naming.** `release`, `forget`, and `park` name one state change three ways, and `released` names the opposite relation. Rename to state the effect, not the cause: `release` becomes `take_back` (it gives the chain's slots back at its end), `forget` becomes `drop_aborted` (it drops an aborted chain from both queues and from the holders), and `park` stays `park`, which already names the state a chain enters. Rename the `released` field to `gave_back`, since it is the map of chains that gave slots back, not the slots. Update the readers: `chain.rs:169` and `:233`, `waits.rs:82`, `drive.rs:218` (`clear` is unchanged), and the calls in `admission-tests.rs:117`, `:157`, `:170`, `:181`, `:193`, `:208`, `:215`, `:231`, `:243`, `:245`, `:294`, `:311`, plus the field reads at `:308`. Each renamed method keeps its existing doc paragraph with the term substituted; the three-way split becomes three names for three distinct operations, which is what the finding asks.

**Artifacts, `src/execute/scheduler/chain_ops.rs`.** Remove the `ctx` field at `:35` and its initializer at `:54`, and have `prompt` return `Arc::clone(self.chain.ctx.prompt())`. The signature is unchanged, so no call site moves. After this, one `RunState` reference reaches the prompt: the chain's own fork, not the scheduler's sibling copy.

**Acceptance.** `h1/pass.rs:68` and `exit_rules.rs:45` pin the `0.1` entry ids, and `happens_before.rs:433` pins the single root identity that a double scope acquisition would break. The scheduler suite pins that a started chain runs, which is the behavior the moved push must preserve.

**Doc text, first group, one sentence each.** Each bullet below is one replaced or appended sentence, no code change:

- `chain.rs`: `finish` delivers three ways, but a `Determinism` outcome ends the run first, and the frame is armed only when not H1.
- `h1.rs`: `end_h1`'s `# Errors` gains the gone capability and the chain-count cause its body raises.
- `scheduler.rs`: `prompt_origin` enumerates four `label` shapes, including the tool id a caller passes; the `scope` field states its single writer.
- `task_end.rs`: `end_backing` adds the queued-chain silence its body enforces.
- `admission.rs`: ordering and counting live in `plan`, which makes both true; `admit` and `admit_chain` shrink to one sentence each, keeping both `# Errors`.
- `chain_ops.rs`: name all eight files holding `impl ChainOps` blocks, including this one, and include `prompt` in the shared four.
- `walk.rs` and `chain_record.rs`: `Chain::counters` becomes the normative `0.1` statement, and the other three sites defer to it.
- `drive.rs`: the stall guard tests two conditions, and the admission queue needs no third.
- `step.rs`: cut the module doc's four-case list and `step_chain`'s copy, keeping the block-outcome half.

**Doc text, second group, one sentence each.** Also doc-only, except the `waits.rs` renames:

- `chain_record/arena.rs`: `ChainArena` holds at most `MAX_CHAINS`, and a test build lowers that through `set_max_for_test` so the overflow path runs without allocating the real bound. `ChainIndex` is created only by `push` outside test builds, and a `#[cfg(test)]` constructor mints one for tables that only compare indices.
- `chain_record.rs`: `Entered`'s drop is the teardown boundary in all three taking paths, reporting `SECTION_FINISHED` only where the section completed, so an abort's teardown is deliberately silent. `Chain::access`' `# Errors` also says the two teardown probes skip on it. `entered_name` states that the frame's `sys.section_name` and the walk position name the same section while entered, and that this field is the only record between sections and after teardown, written only after the frame is constructed.
- `chat.rs`: `overflow_result`'s doc names both the precheck, where no round ran, and a provider's overflow refusal, where the round ran and reported no finish reason.
- `dispatch.rs`: the routing sentence names the three arms this file keeps and the module and file each remaining group lives in. The opening qualifies that a refusal decided before dispatch may report its failure and answer inline. `dispatch_vfs` drops the repeated leaf-yield rule and keeps only its own contract.
- `effects.rs`: `orphan` says any continuation other than a tool call is orphaned and returns `None`. `clear_withdrawn` says `drive::end` clears the withdrawals after teardown orphaned the rest silently, so a decided run lists none rather than some.
- `tasks.rs`: `TaskState::Delivered` reads "Whether the task ended well, carried so `status` can report it after the outcome is gone", leaving the origin sentence on `EntryState::Delivered`.
- `tool_call.rs` and `tool_call-discovery.rs`: the adhoc paragraph moves into `tool_call-adhoc.rs`, leaving one pointer in the parent; the discovery child's opening reduces to its freeze-before-preparer guarantee; `Listed::input_schema` gains a doc naming the one branch that sends it.
- `waits.rs`: rename `live_of` to `live_author_tasks` and `live_with_timers` to `live_all`, updating `waits.rs:160`, `:169`, `task_end.rs:133`, the module doc at `tasks.rs:17`, and the test at `tasks-tests.rs:181`.

**Findings.** S3-10, S3-25, S3-11; docs S3-1, S3-2, S3-3, S3-4, S3-5, S3-6, S3-7, S3-8, S3-9; second group S4-5, S4-6, S4-4, S4-9, S4-28, S4-11, S4-13, V4-2, V4-1, S4-14, S4-15, S4-2, S4-20, S4-23, S4-24, S4-3.

</step-5>
<step-6>

### Step 6: One owner for the empty-reply message, and a narrowed `test-support`

- Component: Test support
- Tests: `cargo nextest run --locked -p runvil-engine -p runvil-model-client -p harness-gateway-client -p runvil`, then `cargo bench -p runvil-engine --features test-support --no-run`, then `cargo +<pinned nightly> xtask api --check`

**Empty-reply owner.** Add `pub fn empty_reply(reasoning_present: bool) -> CompletionError` beside `CompletionError::new` in `crates/runvil-internal/model-client/src/error.rs:173`. Build it from the existing private `specific` formatter and `with_finish_reason`. Its doc says it reports `EmptyReply` and extends the phrase with the reasoning-ignored specific when reasoning was present. Call it from `normalize::empty_reply_error`. Then delete `REASONING_IGNORED` and the local `empty_reply` body in both `src/test_support/scripted-chat.rs:23` and `crates/harness-gateway-client/src/wire/parse.rs:35`, and delete the "two copies change together" sentence at `scripted-chat.rs:6`. The Engine copy's whitespace predicate at `scripted-chat.rs:151` becomes `!text.trim().is_empty()` as part of the move, since the gateway already treats whitespace-only reasoning as absent and that divergence is the reason the copies exist.

**`harness-gateway-client` reachability.** It reaches `CompletionError` through the `runvil` facade rather than depending on `runvil-model-client`, so the single owner is callable from both with no new dependency. Note in the commit message that this adds a method to a type the facade publishes, which a prior pass declined only for that reason, and the operator answered yes.

**Text pinned by tests.** The exact strings are asserted at `exit_rules.rs:352`, `model-client/src/model/error-tests.rs:230`, `wire/parse-tests.rs:248`, and `transport/tests/streaming.rs:68`. All four must pass without edits; a change to any string is a failure of this step, not a rebless.

**Narrowing, `src/test_support/scripted-chat.rs`.** Change these to `pub(crate)`: `MOCK_MODEL` (`:26`), `text` (`:31`), `text_finish` (`:38`), `calls` (`:62`), `fail` (`:84`), `ScriptedReply::answer` (`:142`), `ScriptedCall` with its three fields (`:212`), `messages_json` (`:228`), `tool_names` (`:235`), `ScriptedChat::requests` (`:279`), and `last_request` (`:288`).

**Keep `pub`.** `drive`, `perform_locally`, `ScriptedChat`, `new`, `complete_now`, `call_count`, `ScriptedReply`, and its variant fields. That list is exactly what `benches/models_loop.rs` names, and the bench is a separate crate, so `pub(crate)` cannot reach it.

**Vestige in the same file.** Delete `ScriptedReply::Failure`'s `finish_reason` field and the `if let Some(reason)` block that reads it. No constructor in the workspace sets the field, so the three reading lines cannot run, and the bench never names `Failure`. Grep the workspace for `Failure` with a `finish_reason` before deleting; if a setter exists that this plan missed, stop and report rather than removing a reachable field.

**Acceptance.** `cargo bench -p runvil-engine --features test-support --no-run` fails if the bench needed one of the narrowed items, which makes the compiler prove the list. If the facade publishes `empty_reply`, rebless `crates/runvil/public-api.txt`. Findings: S6-2, V6-1, S6-7, S6-6.

</step-6>

<step-7>

### Step 7: Engine doc sentences, one file at a time

- Component: Doc text
- Tests: `cargo nextest run --locked -p runvil-engine` and `RUSTDOCFLAGS=-D warnings cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`

Every bullet is replaced or appended prose plus one visibility token. No behavior changes, so no new test. Where a bullet cites a line number, treat it as a locator, not a guarantee: earlier steps in this plan edited several of these files, so confirm the sentence by its text before replacing it.

**`src/error.rs` and `src/error/convert.rs`, seven edits.**

- The module doc's wrapper list names `RunError` as the boundary, with `ParseError`, `CompletionError`, and `ToolError` reaching it through `RunError`'s classification.
- Append that `Parse`, `Completion`, and `Substitution` are transparent, so `originating_error` answers `None` for two of them and the parser's cause for `Parse`.
- On `Store`, append why its cause is the concrete `VfsError`: the Lua error table carries the tag and fields this crate rebuilds it from.
- On `UnsupportedVersion`, state that major 0 is the only supported major and that `Display` names it beside the declared one.
- On `LuaQuota`, write "A Lua quota ran out: a `log()` event or byte budget, or the instruction-hook budget that keeps a block cancellable."
- On `DepthVia`, add that the reported name is the cap's, not the author's construct, and reduce `Call`'s doc to "A `call`, or a `tasks.spawn`."
- In `convert.rs`, replace "the gateway client, parser, and Lua crates'" with "the model-client, parser, and Lua crates'". The old wording names a crate the boundary forbids this module to reference.

**`src/execute.rs`, `src/execute/bindings.rs`, `src/execute/config.rs`, five edits.**

- Delete "journaled" at `execute.rs:12`, `bindings.rs:1`, and `bindings.rs:18`; write "the run's model bindings", and on `roles` "The decision: role label to the bound model's identity".
- `model`'s doc: `None` means no role is bound, and a catalog model reachable by raw id is not in the map either.
- `models` and `model_bindings`: the map holds the info of every model a role is bound to.
- `name`: the caller's label, moved into `RunState::execution` at start and read nowhere else.
- `uid`: shown in a debug rendering only; events and reports carry `name`.

**`RunResult`'s doc, `src/execute.rs`.** Delete the `# Outcomes` list first by itself, then rewrite the surviving paragraph. The rewrite names `Step`, `Effect`, and `EffectAnswer` as closed peers, says why the outcome is the exception to the closed set, and states that a declining prompt still completes with its reason as the `Completed` text. Deleting before rewriting is deliberate: the list restates all three variant docs and has already drifted from the one it restates.

**Six files, one sentence each.**

- `src/execute/config-limits.rs`: `lua_log_events` says the byte ceiling is derived from this count, so raising it raises both budgets.
- `src/execute/context.rs`: name `tasks.spawn` and `tasks.fanout` beside `call` in the fork list and on `with_args`, since an arm with an input takes the same fork; `limits` becomes `pub(super)` alongside its siblings.
- `src/execute/context-bound.rs`: state that `derive_argv` re-runs on every args fork, and add the declared description and context window to the role-binding list.
- `src/execute/infer.rs`: replace "one prose round of `round`" with `turn::report_model_turn`, which is the item that exists.
- `src/execute/requirements.rs`: `merge` appends `unmet_models` whole rather than skipping; the `UsablePack` vendor promise excludes a Pack listed in both roster lists; required versus actual is model lines only.
- `src/execute/run/effect.rs`: `Timer`'s record stores fractional seconds, which `effect-tests.rs:164` pins as `{"Timer":{"seconds":0.5}}`. Describe the shape; do not alter it.

**`src/execute/error.rs`, five edits.** Add `SourceLocation` to the module doc so all three exports are named; name all three quota resources on `RunErrorKind::Quota` and all six members on `Lua`; extend `originating_error` to all eight cause kinds; drop `Parse`'s compiled-Lua claim, since a compiled Lua region now classifies as `Lua`.

**Findings.** S1-6, S1-8, S1-7, S1-9, S1-10, S1-18, S1-13, V6-2, S1-5, S1-3, S1-4, S1-14, S1-17, S1-11, S1-12, S1-15, S2-11, S2-6, S2-5, S2-12, S2-4, S2-1, S2-3, S2-13, S2-14, S2-7, S2-2, S2-8, V2-1.

</step-7>

<step-8>

### Step 8: Section docs and the three import surfaces

- Component: Doc text
- Tests: `cargo nextest run --locked -p runvil-engine`, plus `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` for the import moves

**Section docs, one edit each.**

- `src/execute/section_context.rs`: the no-derives note becomes "Not `Clone`: the frame owns the VM and its one teardown", since only `Clone` is blocked. Drop the reporting half of the `sys` field doc and say on `SectionSys::section_name` that `Drop` reports under it, fixed at construction.
- `src/execute/section_vm.rs`: add the raw-model-id opt-in to `setup_section_vm`'s fixed sequence, after injection, with the reason it must land first. Leave the module doc alone.
- `src/execute/section_context-construct.rs`: reduce the `construct` module doc's delta clause to name `new_h1` as the list, because that list is short by two.
- `src/heading_address.rs`: on `Visible::of`, state the `debug_assert!` and drop the fallback promise, which today is true in release and panics in debug; reduce two `# Errors` paragraphs to a pointer at `not_found`, which owns the not-found rule.

**`src/subst.rs`, two edits.** `NullValue` scopes its rule to `var`, `sys`, `argv`, and a bare global, and says a null `item` renders as the text `null`, which is what `{{ item }}` does. `substitute`'s `# Errors` names `SubstErrorKind` as the complete list, with the offset and bounded preview, replacing the prose that duplicates the ten variants one for one.

**The three import surfaces, `src/model.rs`, `src/lua.rs`, `src/parser.rs`.** Each file states the rule once: this module is the import surface for its dependency, and production code reaches the dependency through it. Add `ModelSetError` to `model.rs` and `ModelKeyword` to `parser.rs`, which today are missing from the surfaces that claim to be complete. Route the eight bypassing imports through the surface. Name the exception groups in the doc rather than claiming two: at least five groups exist. In `model.rs`, name `harness-gateway-client` as where the transport codec lives, replacing the sentence that sends the codec to a crate that has none.

**Acceptance.** The import moves are mechanical, so the suite proves them; a missed re-export is a compile error, not a silent change. No new abstraction is added to satisfy any of these docs. Findings: S5-4, V5-2, S5-6, S5-7, S5-5, S5-8, V5-1, S6-4, S6-5, S6-1, S6-3.

</step-8>

<step-9>

### Step 9: Engine repository drift, two files

- Component: Drift
- Tests: `cargo test -p build-xtask` and `node --test crates/workshop/ui/test/docs-claims.mjs`

Doc edits only, in the engine repository, so they commit here rather than with the docs checkout. Both are checked against the source they describe.

- **`crates/runvil-internal/engine/AGENTS.md`, line 4.** Replace "at dispatch" with "at dispatch for a `Vfs` effect and once at section-VM install for the section's `store` table", so the rule matches both derivations. The install-time derivation is what lets a prelude's install-time store call issue no effect, and it is visible at `crates/runvil-internal/lua/src/engine_globals-store.rs:258` and `src/execute/section_vm.rs:196`.
- **`README.md`, line 59.** Replace the effect list with "a model reply, a Pack's render of the tools a section offers, a catalog tool's result, a timer, or a file outside a prelude's install". `Effect` has five kinds and the current line lists four; `AGENTS.md:9` and `runvil-docs/src/introduction.md:14` already name all five, so this makes the third site agree. Also change "a tool result" to "a catalog tool's result", since only a catalog tool call is answered by the caller.

**Acceptance.** `docs-claims.mjs` checks Engine wording, rulebook Definitions, and Pack lifecycle wording, so it is the gate that fails if the new phrasing breaks a wording rule. Run it with `cargo test -p build-xtask` in the same command as the plan's structural checks. Findings: S7-1, S7-3.

</step-9>

<step-10>

### Step 10: `runvil-docs` checkout drift, three files

- Component: Drift
- Tests: `MDBOOK=<path> cargo xtask site --books-only` from the engine repository, then a chapter render check that the changed pages build

`runvil-docs` is its own git checkout at `c:\Users\Vinnie\cursor\runvil-docs`, latest edit 2026-10-27, so this is a separate commit in a separate repository. It cannot share a commit with any step above. All three edits are guide-side: the guide's own chapters settle each case, and code stays put.

- **`src/language/01-what-a-prompt-is.md`, line 604.** Qualify the absolute so it covers the calls a block makes, and add that a prelude's install-time call runs inside the Engine and reports only its event. This matches `12-tools.md:2767`, and the shim install order at `src/execute/section_vm.rs:196` settles the conflict in `12-tools.md`'s favor.
- **`src/language/16-limits-and-errors.md`, after line 1093.** Add one table row: `| InvalidParts | a caller built a tool call or completion from parts the validating constructors refuse | no |`. The table lists twelve of thirteen `CompletionErrorKind`s, and `InvalidParts` reaches a run record as `invalid_parts`.
- **`src/language/12-tools.md`, lines 2389 and 915, and `01-what-a-prompt-is.md:600`.** State the rule as one event pair per freeze attempt, with a failed freeze attaching nothing so a later round renders and validates again. Every freeze attempt reports its own pair at `src/execute/scope.rs:148`, so "once per section entry" is wrong at all three sites.

**Ordering rule.** Commit this step in the `runvil-docs` repository, not the engine one. The step number here is plan ordering, not commit order within a single repository; this commit exists as its own change because the checkout is separate.

**Flags.** Two of these are the only work in this plan that lands outside the cargo workspace, so no cargo gate covers them. The mdBook build is the only check available, plus reading the rendered page. Findings: S7-2, S7-4, S7-5.

</step-10>
Each step is one commit against a green suite. Run the step's own `Tests` line before committing; a doc-only step runs the crate suite and the rustdoc gate, because `RUSTDOCFLAGS=-D warnings` is what fails on a broken link or a stale intra-doc reference. Never merge a doc sentence into a code commit, and never split one step across two commits.

Steps 9 and 10 land in separate repositories. Step 10's commit belongs to the `runvil-docs` checkout at `c:\Users\Vinnie\cursor\runvil-docs`, so it cannot be part of the engine repository's history. The step numbers order the plan, not a single repository's log.

</execution-plan>
