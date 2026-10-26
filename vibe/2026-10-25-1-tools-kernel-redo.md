---
name: Tools kernel redo
overview: "Make tools.offer_dynamic work through two Engine tools, GetDynamicTools and CallDynamicTool, replace tools.calls with tools.stats, and add a late-render hook so every Harness Pack shapes the tools it shows each model. All model-facing tool state is per section: the run keeps the Lua-facing catalog by id, and each section's merge names, checks, and freezes its own Static Set and Dynamic Set."
todos:
  - id: reserve-dynamic
    content: Reserve the Pack id dynamic the way adhoc is reserved (registry, parser, RunContext::prepare) from one const list in runvil-types with a PackId::is_reserved check, replacing Requirements::reserved_adhoc with the reserved ids a run names, with tests
    status: pending
  - id: render-hook
    content: Add Pack::render(cx, offered) with a RenderContext and a pass-through default, re-export RenderContext, ModelInfo, ModelId, and ThinkingMode from runvil-plugin, add ToolInfo::wire_name, which a Pack may also set in Pack::tools(), and reserve RV_PACK_RENDER as verb 8 in abi.rs, with tests. It lands before section-bindings, whose test support names tools through wire_name
    status: pending
  - id: section-bindings
    content: Add the Static Set and Dynamic Set terms to AGENTS.md in their own commit. In the behavior commit, resolve each call once in tool_route, hold the run's ToolCatalog in ToolSet with vendors per Pack, drop ToolBinding's vendor and derive it at report sites, dedupe offers by id only and move every wire-name rule into the section's merge, check every tool through tool_schema_new, build both sets as ToolBinding lists on RenderedTools in place of dispatch and Advertised, project Chat schemas from the Static Set when the Chat effect is issued, read the frozen sets through the frame in place of Chain.advertised, use catalog wire names for global_exists and the tools.offer hint, and move Scenario::offer and the other test-support offerings into the catalog under wire_name, with tests and docs
    status: pending
  - id: discovery-tools
    content: Add GetDynamicTools and CallDynamicTool as one discovery route inside tool_route's single lookup, in a tool_call-discovery child module, for model calls only, reusing prepare_adhoc_call and prepare_catalog_call, and let the scope gate pass Dynamic Set names so a direct call gets refusal text pointing to CallDynamicTool, with tests and docs
    status: pending
  - id: stats
    content: Replace tools.calls with the read-only tools.stats table (total, packs, tools), computed in Lua from the one id-keyed ToolCallCounts map with discovery and model-only ids kept out of the tools view, with tests and docs
    status: pending
  - id: caller-origin
    content: Add the section entry id, read through a new SectionContext accessor that the render effect reuses, and the round's model name to ToolCallOrigin, both read from the calling frame. Record the model name on the message list when a round is committed, not on ChatAnchor, which drops the round's model inside accept_chat before the calls are issued, with tests
    status: pending
  - id: render-effect
    content: Add the Render effect carrying the resolved ModelBinding, its answer, both records, and Continuation::Render holding only that binding, after moving effect.rs's record types into a private child that also defines PackToolSets, and re-export PackToolSets up to the facade. ModelBinding carries the catalog ModelInfo from role fill with a Never fallback. At a section's first model call park the chain before the projection, the precheck, the attach, and the commit, recover the list and offers from the parked frame on the answer, check the answer, merge, attach, and dispatch the chat again. The test rigs and the Harness pass tools through, and every test that steps a run by hand over catalog offers expects the render effect first, with tests
    status: pending
  - id: render-harness
    content: Add ToolPerformer::render and have HostRunContext render each offered Pack once per non-empty set, move survives_stop onto Effect::ToolCall in place of ToolPerformer::survives_stop and drop it from the eight test doubles, and route every Harness call by its id's Pack, rewording the registry comment and the host-run.rs module doc that say a call reaches only a listed tool, with tests and docs
    status: pending
  - id: fmt-debug
    content: Spell the Debug trait as fmt::Debug in every Rust file, leaving every other std::fmt path as written, and add a build-xtask structural check that refuses the std::fmt::Debug spelling, with tests
    status: pending
isProject: false
---

# Static Set, Dynamic Set, and late rendering

<product-contract>

## Product Requirements

Runvil sections record dynamic tool offers today, and the model is sent only the tools offered with `tools.offer`. This plan adds two Engine tools, `GetDynamicTools` and `CallDynamicTool`, so the model can find and call the tools offered with `tools.offer_dynamic`. It replaces `tools.calls` with `tools.stats`. And it adds `Pack::render`, the hook each Harness Pack answers when a section entry first calls a model, so the Pack can fit its tools to that model.

Words this plan uses:

- A section entry is one execution of a section. Re-entering a section, a fan-out arm, a task, and a child started with `call` are each their own entry.
- The freeze is that entry's first model call, the moment the Engine builds both sets and keeps them for the rest of the entry.
- The Static Set is the tools rendered into the model's tool list at the freeze. The Dynamic Set is the tools the model finds with `GetDynamicTools` and calls with `CallDynamicTool`.
- A wire name is the name the model sees for a tool. The tool id is the stable name, such as `web/fetch`, and its default wire name is that id with `/` and `.` turned into `_`, such as `web_fetch`. A catalog tool's wire name is the `wire_name` its `ToolInfo` sets, or else its default wire name.
- The run catalog is every catalog tool's `ToolInfo` by id, read once per run from each Pack's `Pack::tools()` and shared by every section entry. It is what Lua sees.
- A Harness Pack is a Pack the Harness serves, whose tools are in the run catalog. The Pack ids `adhoc` and `dynamic` hold tools the Engine defines itself, and nothing renders them. Engine crate docs and code names call a Harness Pack a catalog Pack, because they name the Harness only to assign it a responsibility.
- Model-facing means the wire name, description, parameter schema, and the flags `structured_output`, `survives_stop`, and `describes_tools` as one section entry's model sees them. `structured_output` makes a script's `tools.call` receive the output as data, `survives_stop` makes a stop leave that call running, and `describes_tools` makes a script call's untrusted output skip the nonce wrapper.
- The scope gate is the check in `ArrivedRound::tool_calls` (`crates/runvil-internal/engine/src/execute/scheduler/chat.rs`) that each name the model called is in the section entry's frozen sets. A name outside them fails the round with `OutOfScopeToolCall`, whose `global_exists` field says whether the run catalog has a tool with that catalog wire name, and whose `in_scope` list holds the Static Set's wire names.
- The nonce is the per-run token the Engine wraps around untrusted tool output (`prepare_dispatch`, `crates/runvil-internal/lua/src/dispatch.rs`).
- The facade is the public API of the `runvil` crate, listed in `crates/runvil/public-api.txt` and checked by `xtask api --check`.

- Problem and users:
  - Prompt authors write section Lua that offers tools with `tools.offer` and `tools.offer_dynamic`. Pack authors write Plugin crates that supply tools. Models call the tools.
  - Dynamic offers are recorded and then dropped: the section's first `models.loop` round stores them in `RenderedTools.dynamic` (`crates/runvil-internal/lua/src/scope.rs`), and the model is sent only the static ones.
  - Each Pack's tools are fixed for the whole run, read once from `Pack::tools()` (`crates/runvil-plugin/src/pack.rs`). Every model family gets the same edit tool, a small context gets the same long descriptions as a large one, and a small model gets as many tools as a large one.
  - Model-facing state is run-wide. Every catalog tool is named once at run start (`offered_bindings`, `crates/runvil-internal/engine/src/execute/context-bound.rs`), and a tool whose name is invalid or clashes is dropped from the whole run after one log line.
- Goals:
  - The model finds Dynamic Set tools with `GetDynamicTools` and calls them with `CallDynamicTool`.
  - Every Harness Pack renders the tools a section offered from it, for that section entry and the model of its first round. By default it passes them through unchanged.
  - A Pack can mix tools Lua can see with model-only tools. A rendered tool the run catalog holds is one Lua can see, call, and count. A tool that appears only in the render's answer is model-only.
  - Every section entry owns its model-facing tool state: wire names, name checks, both sets, and tool flags. The run keeps the Lua-facing catalog.
  - Lua judges progress from `tools.stats`, whose per-Pack totals count model-only tools too, without naming them.
  - The terms Static Set and Dynamic Set name the two sets in docs, code names, and the root `AGENTS.md`.
- Non-goals:
  - Changing the Lua offer API, beyond `tools.stats` replacing `tools.calls`.
  - Reading each Pack's `Pack::tools()` again at every section entry.
  - The Pack-level C Plugin ABI. This plan reserves the render verb's number and stops there.
  - MCP rendering its server tools, and Dynamic Set growth after the freeze.
  - A `failed` counter in `tools.stats`.
  - Rendering for Engine-owned Packs such as `runvil/subagents`.
- Success criteria:
  - A section that offers tools dynamically shows the model `GetDynamicTools` and `CallDynamicTool`. The model lists, searches, and calls a dynamic tool, and every discovery refusal comes back as text while the loop continues.
  - A test Pack whose `render` renames one tool and adds one model-only tool works end to end: the model calls both, the Pack sees the round's model and the section entry, Lua's `tools.call` refuses the model-only id, and `tools.stats.packs` counts its calls.
  - Two sections on different models that offer the same Pack get different renders, each with its own names and flags.
  - Every exit criterion in the Testing Plan passes after each work item.
- Constraints:
  - The tool list is frozen per section entry at its first model call, and later rounds send the same tool bytes even when they use another model (`every_later_round_sends_the_same_tool_bytes_across_a_model_change`, `crates/runvil-internal/engine/src/execute/tests/rendered_tools.rs`).
  - The Engine is pure: it emits effects and the Harness performs them, and Engine crates stay free of `tokio`, `tokio-util`, `async-trait`, `reqwest`, `getrandom`, and `fastrand`, which `cargo test -p build-xtask` enforces.
  - JSON written to the run log or compared in a replay round-trips exactly, with canonical key order.
  - Engine crate docs, comments, tests, and strings call the code that steps a run "the caller", name the Harness only to assign it a responsibility, and leave the Host out. `crates/workshop/ui/test/docs-claims.mjs` checks the parts a scan can see.
  - Error and status text is written for a model to read: concise, factual, and naming what is wrong and what to do next.
  - Behavior changes ship with their tests in the same commit.
- Open questions: None.

## Functional Specification

Section Lua offers tools, and at the section's first model call the Engine has every offered Harness Pack render, then merges and freezes the section's two sets. Every model-facing name and check is decided in that merge, for that section entry. The model calls Static Set tools directly and reaches Dynamic Set tools through two Engine tools. Discovery mistakes come back to the model as text and the loop goes on, while a name outside both sets is a hard error. Lua reads counts from `tools.stats`, and model-only tools stay out of Lua's reach.

- Actors and workflows:
  - Section Lua offers with `tools.offer` (into the Static Set) or `tools.offer_dynamic` (into the Dynamic Set) before the section's first `models.loop` round. Both raise after it (`refuse_after_render`, `crates/runvil-internal/lua/src/tools.rs`).
  - At the freeze, the Engine asks every Harness Pack the section offered from to render, once for each set it was offered into, for that section entry and the round's model. It merges the answers with the section's adhoc tools, the Lua functions a section defines with `tools.adhoc`, and freezes both sets.
  - The model calls Static Set tools directly. It finds Dynamic Set tools with `GetDynamicTools` and calls them with `CallDynamicTool`.
  - Section Lua reads `tools.stats` and calls catalog tools by id with `tools.call`, as today.
  - A Pack author implements `Pack::tools()` for the Lua-facing catalog and may implement `Pack::render` for the model-facing tools.
- Inputs and outputs:
  - `Pack::render(cx, offered)` receives a `RenderContext` and the `ToolInfo` list the section offered from that Pack in one set, and returns the model-facing `ToolInfo` list. The context holds the section entry id, the value Lua reads as `sys.id`, and the round's `ModelInfo` (id, description, context window, thinking mode, and optional provider, `crates/runvil-internal/types/src/models.rs`). A render may reshape, rename, or drop offered tools, and add model-only tools under its own Pack id. Every catalog tool it returns is one the section offered in that set. It receives the same inputs for either set, and a set the Pack was not offered into is not rendered.
  - `GetDynamicTools` description: usage text, then the Dynamic Set's wire names, sorted, comma-separated, in parentheses. Past 100 names the list stops with "and N more; call GetDynamicTools to search".
  - `GetDynamicTools` parameters, both optional: `toolName` (string) and `pattern` (string, a regex of at most 256 characters, matched against Dynamic Set wire names with the `regex` crate's default syntax, so `(?i)` makes it case-insensitive). A `namespace` key is accepted and ignored. When both are given, `toolName` wins.
  - `GetDynamicTools` results: a `toolName` lookup returns `{"tool", "description", "inputSchema"}`. A pattern search, or a call with empty arguments, returns an array of `{"tool", "description"}`, one for every match in Dynamic Set order, and an empty array when nothing matches. A description longer than 200 characters (counted in characters) is cut to its first 200 and followed by "... [truncated]".
  - `CallDynamicTool` parameters: `toolName` (string, required, a wire name from the list) and `arguments` (object, default `{}`). A `namespace` key is accepted and ignored.
  - `tools.stats` is a read-only table with three keys:
    - `total` counts every tool call the section made, at dispatch, so a call that later fails or is cancelled still counts. A call refused before its count is taken (an unbound name, a failed scope install, or an out-of-scope model call) stays out of every counter.
    - `packs[pack]` counts calls per Pack id, model-only tools included.
    - `tools[id]` counts calls per tool Lua can see.
    - A `CallDynamicTool` call counts under the tool it calls. A `GetDynamicTools` call and every discovery refusal, a refused direct call included, count under `packs.dynamic`. All of them count toward `total`.
  - Every call's `ToolCallOrigin` names the section entry that made it. A model call's origin also names the requesting round's model (`ModelId::name()`), and a script call's origin leaves the model empty.
  - A script's `tools.call` of a catalog tool reports the tool's id as its alias, in its `ToolCall` effect, that effect's record, and its `ToolResult` event, because its binding is built from the run catalog and a script names tools by id. Today a tool in the run-start offering reports its wire name there, such as `tools_echo` for `tools/echo`.
- States and validation:
  - Each section entry freezes once, rendered for the model of its first round.
  - Before the merge, offers are deduped by id, and a static offer of an id wins over a dynamic offer of it. Every rule that compares wire names runs in the merge, after the render.
  - Each set keeps offer order by Pack: a Pack's tools sit together where that Pack's first tool was offered into that set, in the order its render returns them, and an adhoc tool keeps its own place. A section that interleaves one Pack's offers with another's sees each Pack's tools together, where today each tool sits at its own first offer: offering `kit/a`, then `web`, then `kit` gives `kit_a, kit_b, web_fetch` in place of `kit_a, web_fetch, kit_b`.
  - The discovery tools join the Static Set when the frozen Dynamic Set is non-empty. Their ids are `dynamic/search` and `dynamic/call` under the reserved Pack id `dynamic`, their wire names are `GetDynamicTools` and `CallDynamicTool`, and their vendor is `runvil`.
  - The merge, in this order:
    1. The render's answer matches what was asked: the same Packs and sets, in offer order. For each Pack and set, every rendered id has that Pack's id as its first segment, an id repeated within the set keeps its first copy, and a rendered id in the run catalog is one the section offered in that set. A catalog id keeps the catalog's `structured_output`, `survives_stop`, and `describes_tools`, so a render changes only its wire name, description, and schema. A model-only tool's flags are whatever that section's render returns. Each tool's wire name is the render's `wire_name`, or else its default wire name, so a catalog tool passed through keeps its catalog wire name.
    2. Adhoc tools join their sets. Within a set, when an adhoc tool and a Pack tool share a wire name, the adhoc tool stays and the Pack tool is dropped, renamed and model-only tools included.
    3. A Dynamic Set tool whose id the Static Set holds is dropped.
    4. When the Dynamic Set is non-empty, the discovery tools join the end of the Static Set.
    5. A wire name used twice anywhere in the two sets, within one set or across them, the discovery tools included, is a merge failure.
    6. Every tool in both sets has a wire name that passes `[A-Za-z][A-Za-z0-9_-]{0,63}` and an object parameter schema.
  - Until the render effect lands, the merge runs over the offered tools directly, as if every Pack passed its tools through.
  - The Pack id `dynamic` is reserved the way `adhoc` is: the registry refuses a Pack with it, `uses:` refuses it, and `RunContext::prepare` reports a catalog tool or roster Pack with it as unmet.
  - `tools.adhoc` refuses the names `GetDynamicTools` and `CallDynamicTool`.
  - The "did you mean" hint of `tools.offer` for a guessed wire name (`crates/runvil-internal/lua/src/tools/offer.rs`) compares against the run catalog's wire names, then the adhoc tools' names, as it does today against the run-start offering.
  - `tools.stats` accepts exactly its three keys and raises on any other. `tools` starts with a zero entry for every id Lua can see in both sets: the run catalog's ids and the adhoc ids. `packs` starts with those tools' Packs, plus `dynamic` when the discovery tools are rendered. Each raises on a key it does not hold, naming that key and listing the keys it holds. On a call, `packs` gains an entry for a Pack it did not start with, and `tools` gains one only for an id Lua can see. `pairs` walks `packs` and `tools` in key order.
  - `tools.stats` appears when `tools.calls` does today, through the one-time install that also fills `sys.model` (`install_section_scope`, `crates/runvil-internal/engine/src/execute/section_context.rs`).
- Errors and recovery:
  - Every discovery refusal is a plain-text tool result, and the loop continues: an unknown `toolName`, a Static Set name, a `toolName` or `pattern` that is not a string, a bad or overlong `pattern`, a missing `toolName` on `CallDynamicTool`, a non-object `arguments`, and a direct model call to a Dynamic Set name. Each text says what was wrong and what to do next, such as naming `GetDynamicTools` for an unknown name, or saying to call a Dynamic Set tool through `CallDynamicTool` with that `toolName`. Each refusal reports `TOOL_CALL_FAILED` and a `ToolResult` holding its text, as a failed model call reports its failure (`prepare_model_dispatch`), except that the text is trusted. A `GetDynamicTools` answer reports `TOOL_CALL_SUCCEEDED`.
  - These refusals must be successful plain outcomes (`ToolCallOutcome::Plain`, `crates/runvil-internal/lua/src/protocol/answer.rs`), because an error on the dispatch path raises out of `models.loop` (`crates/runvil-internal/lua/src/models_loop/machine.rs`).
  - A model call to a name outside both of its section's sets fails the round, as today (`model_calling_an_offered_but_unscoped_tool_is_a_hard_error`, `crates/runvil-internal/engine/src/execute/tests/tool_loop-scope-gate.rs`).
  - A script's `tools.call` on a `dynamic/*` id fails as an unknown id, because `tools.call` resolves ids in the run catalog. Model-only tools are likewise out of Lua's reach.
  - A merge failure fails the section's model call with an error naming the tool and what was wrong. It covers a catalog wire name that is invalid or clashes: the sections that offer the tool fail with a clear error, where today the tool disappears from the whole run after one log line.
  - When the caller drops the render effect, through a cancel, a stop, or a panicking `render`, the model call ends as a cancelled chat, and Lua sees the cancellation error.
- Security and privacy behavior:
  - A `GetDynamicTools` listing is untrusted, because descriptions come from Packs and their servers. The Engine wraps it with the nonce, and its `ToolResult` event has `trusted` false.
  - A refusal is Engine text and is trusted. A tool called through `CallDynamicTool` reports through its own path, under the `CallDynamicTool` call id.
  - Lua's offer is the upper bound on the catalog tools the model sees, because a render returns only catalog tools the section offered.
  - `tools.stats.tools` starts with only the ids Lua can see, because its unknown-key error lists every key it started with.
- Acceptance criteria:
  - The success criteria hold, and every test in the Testing Plan passes.

</product-contract>
<implementation-contract>

## Technical Design

The run keeps one Lua-facing catalog by id, and everything model-facing is built per section entry: each section's merge names its tools, checks them, and freezes two `ToolBinding` lists on `RenderedTools`. The Chat effect projects its schemas from the Static Set when it is issued. The render is a new effect the Engine issues at a section's first model call, so the Engine stays pure and the run log records what each Pack rendered. The Harness answers it by calling `Pack::render` on each offered Pack. Every call resolves to its binding once, the frozen sets are the one record of what the model was shown, and each tool call holds the one flag the Harness reads, `survives_stop`.

- Architecture:
  - The Engine (`runvil` and `runvil-*` crates) steps a run and emits effects, the Harness (`harness-*` crates) performs them, and Packs supply tools. `Effect` is a closed enum (`crates/runvil-internal/engine/src/execute/run/effect.rs`).
  - Run-wide tool state is Lua-facing and the same for every model: the declared Packs, the roster of Pack availability, the run catalog, and each Pack's vendor, all in `ToolSet` (`crates/runvil-internal/lua/src/handles.rs`), plus the run's model set. The render reads the catalog `ModelInfo` carried on the round's binding, not a second copy of `ModelBindings`.
  - Section-entry tool state is everything else: the offers, the adhoc tools, the message list with its `RenderedTools`, and the call counts. Each section entry already rebuilds all of these with a new `SectionVm` (`enter_section`, `crates/runvil-internal/engine/src/execute/scheduler/walk.rs`).
  - The render runs at the freeze, because `render` takes what the section offered, which is known only then.
- Modules and interfaces:
  - The new public and protocol declarations, as sketches. The work items settle exact visibility and derives.

    ```rust
    // crates/runvil-plugin/src/context.rs, beside ToolContext

    /// What a Pack learns about the section entry it renders for. The Harness
    /// builds it with `RenderContext::new`, and later facts join as fields.
    #[non_exhaustive]
    pub struct RenderContext<'a> {
        /// The section entry, the value Lua reads as `sys.id`.
        pub entry: &'a str,
        /// The model of the section's first round.
        pub model: &'a ModelInfo,
    }

    // crates/runvil-plugin/src/pack.rs

    pub trait Pack {
        // tools(), ready(), and call() stay as they are.

        /// New. Returns the tools the model sees, given the tools a section
        /// offered from this Pack in one set. It runs in the effect loop with
        /// `call`'s rules: it must not block while polled, it must not panic,
        /// and it is cancellation-aware.
        fn render<'a>(
            &'a self,
            _cx: RenderContext<'a>,
            offered: Vec<ToolInfo>,
        ) -> PackFuture<'a, Vec<ToolInfo>> {
            Box::pin(async move { offered })
        }
    }

    // crates/runvil-internal/engine/src/execute/run/effect.rs

    /// New. One Pack's tools for each set of a section entry. The effect and
    /// its answer hold `ToolInfo`, and the effect record holds `ToolId`.
    pub struct PackToolSets<T> {
        pub pack: PackId,
        pub static_set: Vec<T>,
        pub dynamic_set: Vec<T>,
    }

    pub enum Effect {
        // Chat, Vfs, and Timer stay as they are.
        ToolCall {
            // tool, alias, args, origin, and access stay as they are.
            /// New. Whether a stop leaves this call running, from the binding
            /// the call resolved to.
            survives_stop: bool,
        },
        /// New. Render each offered Harness Pack's tools for one section entry.
        /// Carries the resolved binding, as Chat does, so the record derives
        /// the alias and the performer reads the binding's ModelInfo.
        Render {
            entry: String,
            binding: ModelBinding,
            offered: Vec<PackToolSets<ToolInfo>>,
        },
    }

    pub enum EffectAnswer {
        /// New. Each Pack's rendered tools per set, in the order asked.
        Render(Vec<PackToolSets<ToolInfo>>),
    }

    pub enum EffectRecord {
        /// New. `alias` comes from the binding, as `EffectRecord::Chat` takes it.
        Render { alias: String, entry: String, offered: Vec<PackToolSets<ToolId>> },
    }

    pub enum AnswerRecord {
        /// New. The full rendered tools, so the log shows what each entry sent.
        Render(Vec<PackToolSets<ToolInfo>>),
    }

    // crates/harness-internal/runner/src/performers.rs

    pub trait ToolPerformer {
        // call() and max_in_flight() stay. survives_stop() goes away.

        /// New. Renders each offered Pack's tools for one section entry. The
        /// default passes them through.
        fn render(
            &self,
            entry: String,
            model: ModelInfo,
            offered: Vec<PackToolSets<ToolInfo>>,
        ) -> BoxFuture<Vec<PackToolSets<ToolInfo>>> {
            Box::pin(async move { offered })
        }
    }
    ```

  - `Pack::render` and `RenderContext`: `RenderContext` is `#[non_exhaustive]`, so later facts join it and existing Packs keep compiling, and `dyn Pack` stays object-safe. `render`'s inputs and outputs are plain data, free of Rust-only handles such as `HostServices`, so a later C verb can pass them serialized.
  - The run catalog: `ToolSet` replaces `offered: Vec<ToolBinding>`, whose bindings are named at run start, with the run's `ToolCatalog` and `vendors: BTreeMap<PackId, String>`, the per-Pack vendors `RunContext` holds today. `ToolCatalog` (`crates/runvil-internal/types/src/tools/registry.rs`) is an `Arc<[ToolInfo]>`, so `ToolSet` clones the handle `RunContext.tools` already holds instead of deep-cloning every `ToolInfo` into a second map; lookups use its by-id scan and readers iterate `tools()`. These also replace `RunState.catalog_bindings` (`crates/runvil-internal/engine/src/execute/context.rs`): a script call builds its binding on demand from `catalog.get(id)` under the full id as alias. `offered_bindings` and `catalog_bindings` in `context-bound.rs` go away, because nothing is bound at run start. Every reader of the removed fields fails to compile, which lists them. A catalog tool's wire name is computed where it is needed, in the merge, `global_exists`, and the `tools.offer` hint, as its `ToolInfo.wire_name` or else its default.
  - Test support moves to the catalog. Today `Scenario::offer(alias, tool)` (`crates/runvil-internal/engine/src/test_support/scenario.rs`) binds a tool outside the catalog under a short wire name, and about 80 calls in 28 engine test files depend on it, such as `.offer("echo", echo_tool())` for `tools/echo`, whose default wire name is `tools_echo`. It puts the tool in the run catalog with `wire_name` set to `alias` instead, and `Scenario::offering` and its `with_tool_set` prefill build the catalog the same way, so those tests keep their scripted calls and assertions, including `global_exists: true` in `model_calling_an_offered_but_unscoped_tool_is_a_hard_error`. This is why render-hook, which adds `ToolInfo.wire_name`, lands before section-bindings.
  - Vendor leaves `ToolBinding`: the field and `with_vendor` go away, because the vendor is a per-Pack fact the run already holds once. `ToolSet::vendor_for(id)` derives it from the id's Pack segment through `vendors`, with the Engine's discovery tools reporting `runvil` and an adhoc tool reporting none, as a map miss does today. The three readers (the scope-gate event in `chat.rs` and the `ToolResult` reports inside `prepare_dispatch` and `prepare_model_dispatch`, `crates/runvil-internal/lua/src/dispatch.rs`) derive it there, where the chain's `ToolSet` is in hand, and the two dispatch bodies take it as a parameter. A binding no longer clones its Pack's vendor string once per tool per section.
  - The frozen sets: `RenderedTools` (`crates/runvil-internal/lua/src/scope.rs`) holds `static_set` and `dynamic_set`, both `Vec<ToolBinding>` in merge order, scanned linearly with no wire-name index: a rendered section list is tens of tools, and schema order is the vec order. The Dynamic Set holds bindings, not ids, because after a render its wire names, descriptions, schemas, and flags cannot be derived from the catalog (a renamed or model-only tool exists only there). It does not store schemas. The Chat effect projects them from `static_set` through `tool_schema_new` when it is issued, and a later round projects the same bytes because the sets are frozen. `dispatch` and `dynamic` go away, and so do `Advertised`, which only `dispatch` and the chain copy use, and `Chain.advertised` (`crates/runvil-internal/engine/src/execute/scheduler/chain_record.rs`), a per-round copy of `dispatch`. When a round's answer arrives, the chain is parked on that round, so the scope gate and the tool-call event read the section's frozen sets through the chain's current frame. A chain has one parked chat, and `accept_chat` reads the scope before `settle`, so the frame's sets are that round's sets. The comments in `tool_route` and `chain_record.rs` that describe the chain copy are updated. `rendered_target` (`crates/runvil-internal/lua/src/messages.rs`) and `rendered_ids` (`crates/runvil-internal/lua/src/vm/state.rs`), which read `dispatch` today, read the frozen sets instead: `rendered_target` resolves a wire name in them, and `rendered_ids` returns the Static Set's ids until stats widens it to both sets.
  - One lookup per call: `tool_route` (`crates/runvil-internal/engine/src/execute/scheduler/tool_call.rs`) resolves every call to its binding once, a model call by wire name in the frozen sets and a script call by id among the section's adhoc tools or in the run catalog, and the catalog route holds that binding, so `prepare_catalog_call` only issues the call. Today a model call is looked up twice, in the frozen list and again by wire name in the run-start offering. `ToolSet::wire_binding` goes away.
  - The expansion in `crates/runvil-internal/lua/src/vm/state.rs` dedupes offers by id only, a static offer of an id winning over a dynamic one. Today it also drops by wire name: `current_tool_bindings` drops a catalog tool an adhoc tool shadows, and `current_dynamic_tools` skips a dynamic tool whose wire name the static list holds. Both drops leave the expansion for merge rules 2 and 5, because a render may rename a tool out of a clash, and a clash across the sets must fail rather than vanish.
  - The merge is one function beside `prepare_effective_scope` (`crates/runvil-internal/engine/src/execute/scope.rs`) that builds `RenderedTools` from each Pack's tools per set and the section's adhoc tools, inside the `TOOL_SCOPE_VALIDATION_STARTED` and `SUCCEEDED` or `FAILED` pair that function reports today. A broken rule from 1 to 5 is a new Engine error beside `Error::BindSchema` (`crates/runvil-internal/engine/src/error.rs`), naming the Pack, the tool (both tools for a clash), and the rule, with the same error kinds `BindSchema` maps to. Rule 6 is the schema build, which fails through `BindSchema` as today. The Engine's `Error` is outside the facade. Each set keeps offer order: a Pack's rendered tools sit as one block where that Pack's first tool was offered into that set, in the order its render returns them, and an adhoc tool keeps its own offer position. With every Pack passing through, a section that offers each Pack's tools together keeps today's order, which `crates/runvil-internal/engine/src/execute/tests/offering.rs` pins. A section that interleaves Packs changes order, so the expectation in `an_offered_pack_reaches_only_the_rounds_of_the_section_entry_that_offered_it` (`crates/runvil-internal/engine/src/execute/tests/offering-packs.rs`) changes from `kit_a, web_fetch, kit_b` to `kit_a, kit_b, web_fetch`. The expansion tests in `crates/runvil-internal/lua/src/tools/tests-render.rs` keep pinning call order, which is the merge's input.
  - The wire-name rule lives in `tool_schema_new` (`crates/runvil-internal/model-client/src/detail.rs`), the constructor every advertised tool and every adhoc tool already passes through. Its rule tightens from `[A-Za-z0-9_.-]+` to `[A-Za-z][A-Za-z0-9_-]{0,63}`, the rule adhoc names already follow (`Alias`, `crates/runvil-internal/lua/src/alias.rs`). The merge checks every tool in both sets through it, so a bad name or a non-object schema fails the same way in either set, and it does not keep the schemas it builds.
  - The freeze becomes two-phase. When the expansion holds at least one Harness Pack tool, `Scheduler::prepare_chat` (`chat.rs`) issues `Effect::Render` after the emptiness check, the binding, and the expansion, and before the projection, the precheck, `attach_rendered_tools`, `anchor.sending`, `number_round`, and `list.commit`. Its `offered` lists each such Pack once, in the order its first tool was offered. So the second dispatch projects, prechecks, numbers, and commits exactly once, and a first round the precheck refuses still fixes the list, as `a_first_round_the_precheck_refuses_still_fixes_the_tool_list` (`crates/runvil-internal/engine/src/execute/tests/rendered_tools.rs`) pins today. It parks the chain on `Continuation::Render(binding)` (`crates/runvil-internal/engine/src/execute/scheduler/pending.rs`), holding only the resolved binding: a handle round's binding exists nowhere in VM state, and re-resolving could pick a default another chain set meanwhile. The list and the offers are not stored. On the answer, `apply_answer` (`apply.rs`) recovers them from the parked frame: the list through `SectionVm::messages` (`crates/runvil-internal/lua/src/vm.rs`, made `pub`), which is the list the request named because production builds one `MessageList` per section VM, and the offers through `vm.tool_runtime()` and `vm.adhoc_tools()`, which cannot change because a parked chain's VM runs no Lua. It checks the answer against the offered ids recomputed that way and runs the merge. On success it attaches `RenderedTools` with `MessageList::attach_rendered_tools` and calls `dispatch_chat` with the kept binding, which issues the Chat effect or answers the chain inline on an overflow or an error, as it does today. On a merge failure it answers the parked chat with that error and attaches nothing. Either way the arm returns early, the way the Timer arms do. When the expansion holds no Harness Pack tool, only adhoc tools or nothing, it merges at once. `dropped_answer` maps `Continuation::Render` to the interrupted chat answer, and a render sits outside the in-flight tool cap.
  - The render's `ModelInfo`: `ModelBinding` gains an optional catalog `ModelInfo` and one method, `render_info()`, which returns the carried info or builds one from the binding's id, description, context, and provider with `ThinkingMode::Never`. `bound_model_set` (`crates/runvil-internal/engine/src/execute/context-bound.rs`) sets the field from the `ModelInfo` it already reads, and `with_invocation` keeps it because that method takes `self` by value. `ModelBinding::new` leaves it empty, so the raw gateway id that `raw_gateway_binding` (`crates/runvil-internal/lua/src/models.rs`) builds for `models.get`, and a test rig's hand-built binding, both take the fallback inside `render_info()` with no special case at the render site. `Effect::Render` carries the binding, as `Effect::Chat` does: its record takes `alias` from it as `EffectRecord::Chat` does, and the effect loop passes `binding.render_info()` to the performer. Adding the private field changes no facade line; the new method adds one. `RunState` does not gain a `ModelBindings` store. `RunState::new` already folds that journal into `ModelSet` and does not keep it.
  - Discovery routing, for model calls only, is part of `tool_route`'s one lookup, through one new route kind. The called wire name resolves in the frozen sets: a discovery tool (a Static Set binding under the Pack id `dynamic`) or a Dynamic Set name takes the discovery route, and every other Static Set binding takes the adhoc or catalog route as today. On that route, `GetDynamicTools` answers inline from the frozen Dynamic Set through `ToolCallDispatch::Answered`, a direct call to a Dynamic Set name is answered inline with refusal text, and `CallDynamicTool` resolves its `toolName` in the Dynamic Set and hands the inner `arguments`, with the call's site unchanged, to the existing `prepare_adhoc_call` (`tool_call-adhoc.rs`) for an adhoc target or `prepare_catalog_call` as a model call for any other. Counting, access, trust, and result reporting under the original call id reuse those paths, and every refusal is decided before any preparer runs. The batch path, `dispatch_tool_calls`, stops at the discovery route the way it stops at an adhoc one, so the loop sends that call by itself. The scope gate accepts both sets' names, which never runs a Dynamic Set tool directly, because the discovery route answers every direct Dynamic Set call with refusal text. The tool-call event names its tool and vendor. The route lives in a child module, `crates/runvil-internal/engine/src/execute/scheduler/tool_call-discovery.rs`, beside `tool_call-adhoc.rs`, because `tool_call.rs` is 382 lines against the 500-line ceiling and the route's parsing, search, formatting, and refusals would pass it. `OutOfScopeToolCall.in_scope` keeps listing only the Static Set's wire names, so the error stays short when the Dynamic Set is large.
  - Tool-call origins: `prepare_catalog_call` reads both new fields from the calling frame. The entry id is the frame's `sys.id`, assigned in `walk.rs` and read through a new accessor on `SectionContext` (`crates/runvil-internal/engine/src/execute/section_context.rs`), which stores `sys` with no getter today; the render effect reads the same accessor. The model name comes from the message list: `MessageList::commit` (`crates/runvil-internal/lua/src/messages.rs`), whose one production caller is `prepare_chat`, records the round binding's `ModelId::name()` beside the last send it already keeps. `ChatAnchor` cannot supply it. `settle` (`crates/runvil-internal/engine/src/execute/scheduler/chain_record.rs`) takes the in-flight model inside `accept_chat`, before the loop issues the calls, and keeps it on `measured` only when the round reported usage. A section runs one loop or compaction at a time (`LOOP_RUNNING` and `COMPACT_RUNNING`, `crates/runvil-internal/lua/src/models_loop.rs`), so no other round commits to the list between a round's answer and the issue of its calls, even when an adhoc handler runs `models.infer`. It is the binding's name, which matches the `ModelInfo` that `render` receives; the provider-reported model can differ. `messages.rs` is 490 lines against the 500-line ceiling, so this record stays a few lines or that file splits.
  - The Harness needs two things from a tool call: the Pack, which the id's Pack segment names, and `survives_stop`, which the effect loop (`crates/harness-internal/runner/src/effect_loop.rs`) reads when the call is issued. So `ToolBinding` gains `survives_stop`, `Effect::ToolCall` holds it from the resolved binding, `ToolPerformer::survives_stop` goes away, and `HostRunContext` (`crates/harness-internal/runner/src/host-run.rs`) routes every call by its id's Pack, which covers model-only ids. The registry's comment on built-in Packs (`crates/runvil-plugin/src/registry.rs`) says a built-in's `call` is unreachable because the snapshot routes a call only to a tool in a Pack's own list. It is reworded to say the Engine never issues a call under a built-in's id, which stays true because a built-in lists no tools and so is never rendered. The `host-run.rs` module doc, which says the tools the run offers and the tools its calls may reach are the same list, is reworded too, because a model-only tool is reachable without being in the catalog.
  - `ToolPerformer::render` defaults to passing every tool through, so the eight test doubles that implement `ToolPerformer` need no `render`. Each still drops its `survives_stop`, which the trait loses. Its futures are `'static`, so it clones the `Arc<dyn Pack>` and owns its inputs. `HostRunContext` overrides it, calling each usable Pack's `render` once per non-empty set with a `RenderContext`. The effect loop performs `Effect::Render` inside the effect's own future, so cancellation and a panic behave as for a tool call. Until render-harness lands, the effect loop's render arm returns the offered tools unchanged, a few lines that item replaces.
  - The reserved Pack ids are one definition in `runvil-types` beside `PackId` (`crates/runvil-internal/types/src/packs.rs`): `RESERVED_PACK_IDS: &[&str]` plus `PackId::is_reserved()`, which compares the id's one segment without allocating. A `PackId` cannot be const-constructed (it wraps a `Vec<String>`), so the definition stays strings and is never built into a `Vec<PackId>`. It is read in three places: `PackRegistry::insert` (`crates/runvil-plugin/src/registry.rs`), whose `InstallError::AdhocPackId` generalizes to name the reserved id; `check_no_adhoc_pack` and `PackListError::Adhoc` (`crates/runvil-internal/parser/src/contract.rs`), which generalize to the check; and `RunContext::prepare` (`crates/runvil-internal/engine/src/execute/config.rs`), where `Requirements::reserved_adhoc` (`crates/runvil-internal/engine/src/execute/requirements.rs`) becomes `reserved: Vec<PackId>`, the sorted reserved ids the catalog or roster names. That vec is a per-run findings list, a different thing from the definition. `is_satisfied` needs it empty, `merge` takes the union, and `notice` names each id.
  - `effect.rs` is 498 lines against the 500-line ceiling (`crates/build-ceiling/src/lib.rs`). Before the render variants land, its record types (`EffectRecord`, `AnswerRecord`, `FailureRecord`, `ChatAnswerRecord`, and `ToolAnswerRecord`, with their impls) move into a private child module that `effect.rs` re-exports, so every existing path holds, and `PackToolSets` is defined there too. `effect_loop.rs` is 481 lines, so its render arm stays short or that file splits the same way.
  - `tools.stats` is computed, not stored. `ToolCallCounts` (`crates/runvil-internal/lua/src/scope.rs`) stays the one id-keyed map and gains a single-lock snapshot. No counter is stored per Pack or in total, and the existing increment sites stay as they are. The map starts with the ids Lua can see in both sets, plus `dynamic/search` and `dynamic/call` when the discovery tools are rendered. `section_call_counts` (`crates/runvil-internal/engine/src/execute/scheduler/tool_call.rs`) picks those ids: from the frozen sets once the section has frozen, and from its live offers of both kinds before. Today the map starts from every `dispatch` entry (`rendered_ids`, `crates/runvil-internal/lua/src/vm/state.rs`), which leaves a dynamically offered tool without a starting entry. The Lua install, in place of `install_lua_tool_calls` (`crates/runvil-internal/lua/src/tools.rs`), answers at read time: `total` sums the snapshot, a `packs` proxy sums each Pack's keys, and a `tools` proxy keeps only run catalog ids and adhoc ids, so discovery and model-only ids stay out of it and out of its unknown-key error's list. Each proxy takes its shape from `tools.packs` (`__index`, `__newindex`, `__pairs`, and `__metatable`) and its unknown-key error from `tools.calls`, because `tools.packs` reads an unknown key as nil. The discovery route adds the one new bump: a `GetDynamicTools` call counts as `dynamic/search` whatever its outcome, and a `CallDynamicTool` refusal or a refused direct call counts as `dynamic/call`. A `CallDynamicTool` call that reaches its target counts once, under the target, in the target's existing preparer.
- File and public API changes:
  - The facade changes, so `crates/runvil/public-api.txt` changes:
    - `Requirements`, as above.
    - `ToolCallOrigin` (`crates/runvil-internal/types/src/tools/origin.rs`) gains `entry: Option<String>` and `model: Option<String>`, each with `#[serde(default, skip_serializing_if = "Option::is_none")]` and a `with_` builder. `ToolCaller` stays as it is.
    - `ToolInfo` (`crates/runvil-internal/types/src/tools/info.rs`) gains `pub wire_name: Option<String>`, with the same serde attributes and a builder like `survives_stop`'s. A Pack may set it in `Pack::tools()` as well as in a render, and a catalog tool's `wire_name` is its wire name wherever a render passes it through.
    - `PackToolSets`, which joins each re-export list the other effect types pass through, from `crates/runvil-internal/engine/src/execute/run.rs` to the engine root and the facade's `effect` module (`crates/runvil/src/lib.rs`); the `Render` variants of `Effect`, `EffectAnswer`, `EffectRecord`, and `AnswerRecord`; and `Effect::ToolCall`, which gains `survives_stop`.
    - `ModelBinding` gains `render_info()`. Its new private field changes no line.
  - `runvil-plugin` gains `Pack::render` and `RenderContext`, and re-exports `ModelInfo`, `ModelId`, and `ThinkingMode`, because among workspace crates a Plugin crate may depend only on `runvil-plugin` and `shared-*` crates (`crates/build-xtask/src/product.rs`). `runvil-plugin` sits outside the facade.
  - `ToolPerformer` gains `render` and loses `survives_stop`.
  - In `crates/runvil-plugin/src/abi.rs`, the "Reserved verbs" list goes from "Verbs 3 to 7" to "Verbs 3 to 8" and gains `RV_PACK_RENDER` (8), which renders the tools a section offered from one Pack for one model and finishes later through an `RvHost` function. Following that list's rule, the verb gets its constant later.
  - `regex` joins `[workspace.dependencies]` in the root `Cargo.toml` as a normal dependency of `runvil-engine`, with the updated `Cargo.lock` committed, because every gate runs locked. The version is already resolved in the lockfile through unrelated crates, and the Engine crates reach it today only through the dev-dependency `criterion`, so this adds an edge, not a new version. The structural checks allow it.
  - Lua: `tools.stats` replaces `tools.calls` (`install_lua_tool_calls`, `crates/runvil-internal/lua/src/tools.rs`, backed by `ToolCallCounts`, `crates/runvil-internal/lua/src/scope.rs`), and `current_dynamic_tools` (`crates/runvil-internal/lua/src/vm/state.rs`) becomes `current_dynamic_set`.
  - The root `AGENTS.md` Definitions section gains two bullets after **Plugin**, and "Five words have exactly one meaning each" becomes "Seven terms have exactly one meaning each":
    - **Static Set**: the tools a section renders into the model's tool list at its first model call. It is frozen after that.
    - **Dynamic Set**: the tools the model finds and calls through the Engine's two discovery tools.
  - Docs in the `runvil-docs` repository change as the work items list.
  - The `Debug` trait is never spelled `std::fmt::Debug`: code names it `fmt::Debug` with `use std::fmt;` in scope, and every other `std::fmt::` path stays as written. A structural check, `crates/build-xtask/src/debug_spelling.rs`, run from `tidy::all_violations`, refuses the long spelling on any line of any `.rs` file in a workspace crate and names the file, the line, and the fix.
- Data, persistence, failure, security, and privacy constraints:
  - The run log gains the render effect and its answer. `EffectRecord::Render` holds the binding's `alias`, as `EffectRecord::Chat` does, plus the section entry id and the offered ids. `AnswerRecord::Render` holds the full rendered `ToolInfo`, because `EffectRecord::Chat` keeps only tool names. Adhoc and discovery tools join later, inside the Engine, so the answer does not hold them.
  - JSON written before the new optional fields existed reads unchanged, and each new field is skipped when absent.
  - `EffectRecord::ToolCall` leaves `survives_stop` out, as `EffectRecord::Chat` leaves out the options it derives from the binding: the run catalog and the render's `AnswerRecord` already give it.
  - Stale docs are fixed where they are found, never deferred, because stale docs mislead the models that read them. When a step finds a doc comment, a code comment, or a guide passage that is wrong or out of date, whether or not its own change made it so, it fixes it: in the step's own commit when the text lives in that step's repository, and otherwise by naming the file and the passage in its return, so the session fixes it in the other repository before the step's commit message is written.
  - Failure, trust, and refusal behavior is as the Functional Specification states.

</implementation-contract>
<verification-contract>

## Testing Plan

Each work item ships its tests in the same commit. Unit tests pin the new types and rules, Engine rig tests pin per-section naming, the discovery flow, origins, and the render effect, and Harness tests pin the render calls, the run log, and routing by Pack. Every exit criterion passes after every work item.

- Unit:
  - Reservation: the registry, `uses:`, and `RunContext::prepare` each refuse `dynamic` the way they refuse `adhoc`, and the notice names the id it found.
  - `tool_schema_new` accepts `[A-Za-z][A-Za-z0-9_-]{0,63}` and refuses a leading digit, a `.`, and a 65-character name.
  - A catalog tool's wire name is its `ToolInfo.wire_name` when set and its default otherwise, and `global_exists` and the `tools.offer` hint use the same name.
  - `Pack::render`'s default passes tools through, and `RenderContext` holds the section entry id and the `ModelInfo` it was built with.
  - `ModelBinding::render_info()` returns the info `bound_model_set` set, keeps it through `with_invocation`, and builds the `ThinkingMode::Never` fallback for a binding built with `new`.
  - `ToolInfo.wire_name`, `ToolCallOrigin.entry`, and `ToolCallOrigin.model` read as absent from JSON written before they existed, and are skipped when absent.
  - `tools.stats`: each counter as computed from the one map, the read-only refusal, and each unknown-key error, with a discovery id and a model-only id absent from both the `tools` view and the unknown-key error's list.
  - The merge: one test per rule 1 to 6, table-driven where the shape allows, plus one for each set's order with an interleaved offer and a render that adds a model-only tool. Rule 2's test covers the adhoc case in each set, and rule 5's covers a clash within one set, a clash across the sets, and a Pack tool named `GetDynamicTools` as separate assertions.
- Integration and end-to-end:
  - Section bindings:
    - Two catalog tools whose default names clash: a section that offers one of them works, and a section that offers both fails its first model call with an error naming both. Both tools stay in the run catalog.
    - A catalog tool with an invalid default name fails just the sections that offer it.
    - Two sections that offer the same Pack each build their own bindings, and a model call in one resolves against its own section's sets.
    - The scope gate and the tool-call event read the section's frozen sets, including when an adhoc handler runs its own model rounds between the round's answer and the next round.
    - The `tools.offer` hint for a guessed wire name still names the matching tool id.
    - The `TOOL_SCOPE_VALIDATION` events fire around the merge as they fire around the build today.
    - A catalog tool whose `ToolInfo` sets `wire_name` is advertised under that name.
    - A section that interleaves two Packs' offers sees each Pack's tools together, at that Pack's first offer.
    - A script call's effect, record, and `ToolResult` event report the tool's id as its alias.
  - Discovery:
    - The discovery tools appear exactly when the Dynamic Set is non-empty, and the description lists exactly its names, capped at 100.
    - Search works by name and by pattern, and search results hold only names and descriptions.
    - `CallDynamicTool` on a catalog tool issues `Effect::ToolCall` for its id, and on an adhoc tool runs its handler.
    - Each refusal returns text, reports `TOOL_CALL_FAILED` and a trusted `ToolResult`, and the loop continues.
    - A search with no match returns an empty array, and only a description longer than 200 characters is cut.
    - A direct model call to a Dynamic Set name, alone and in a batch, returns text pointing to `CallDynamicTool`, and the loop continues. A name outside both sets is still a hard error.
    - A batch with a discovery call in it works.
    - A script's `tools.call("dynamic/call", ...)` fails as an unknown id.
  - Caller origin:
    - A model call's origin names its round's model, and a script call's origin leaves the model empty.
    - A section that switches models between rounds reports each round's model.
    - An adhoc handler that runs `models.infer` or `models.use` leaves the model of the batch's later calls unchanged.
    - Every call's origin names its section entry, and re-entering a section and two fan-out arms of one section give different entry ids.
    - Tests compare `origin.model` with `info.id().name()`, because `ModelId::name()` is the name part of an id that also holds a server namespace.
  - Render effect:
    - A section with Harness offers issues one render effect before its first Chat effect, and its later rounds reuse the frozen sets.
    - A section whose offers expand to no Harness Pack tool, only adhoc tools or none, goes straight to its Chat effect.
    - A Pack offered into one set is rendered for that set only.
    - The render effect names the same entry id as the section's tool calls.
    - A section whose first round runs through a model handle (`models.get(...):loop()`) still renders and then chats on that handle's binding, with no re-resolution at the answer.
    - A raw gateway model id renders with a `ModelInfo` built from its binding.
    - Feeding the same render answer to a fresh run yields the same Chat tools. A replay reader is still to be built (`crates/runvil-internal/types/src/replay.rs`), so the Engine rig runs this check.
    - A dropped render answers the chat as interrupted.
  - Harness and routing, with a test Pack whose `render` returns a renamed tool and a model-only tool:
    - The Harness calls `render` once per Pack and non-empty set, with the offered tools and a `RenderContext` holding the section entry id and the round's `ModelInfo`.
    - A panicking `render` answers `Dropped`, and the run log holds the render effect and its answer.
    - The model calls both tools, and the Pack sees the round's model and the section entry on the origin.
    - `tools.call` refuses the model-only id.
    - A model-only call bumps `tools.stats.packs` and stays out of `tools.stats.tools`, including its key list.
    - Tool-call and tool-result events for a catalog, adhoc, and model-only call each report the vendor derived from the tool id's Pack (none for an adhoc tool), and a discovery call reports `runvil`.
    - A model-only tool rendered into the Dynamic Set is callable through `CallDynamicTool`.
    - Two sections on different models that render the same model-only id with different `survives_stop` each keep their own flag: a stop leaves running the call whose flag is true and drops the one whose flag is false.
- Regression, security, and performance:
  - In section-bindings, drop `tools/shape` from the scenario of `dynamic_offers_leave_the_rendered_schemas_byte_for_byte_unchanged` (`crates/runvil-internal/engine/src/execute/tests/rendered_tools.rs`), because rule 6 now fails a Dynamic Set tool whose schema is not an object, and keep it pinning unchanged bytes. In discovery-tools, rewrite it to pin that dynamic offers add exactly the two discovery schemas, at the end of the list. `every_later_round_sends_the_same_tool_bytes_across_a_model_change` keeps passing.
  - The first-round refusals in that file (`a_first_round_the_precheck_refuses_still_fixes_the_tool_list`, `a_first_round_refused_over_an_empty_list_renders_nothing`, and `a_first_round_refused_for_a_missing_model_renders_nothing`) keep passing, which pins where the render sits in `prepare_chat`.
  - Two tests in `crates/runvil-internal/lua/src/tools/tests-render.rs` pin wire-name drops inside the expansion and move to the merge's rule tests. `an_offered_adhoc_tool_shadows_an_offered_catalog_tool_of_its_wire_name` keeps its outcome under rule 2. `a_catalog_tool_a_static_adhoc_tool_shadows_stays_out_of_the_dynamic_list` (a static adhoc `web_search` beside `tools.offer_dynamic('web')`) becomes a rule 5 failure, because only an id match drops a Dynamic Set tool. Update any other test there that pins the old dynamic list. `an_adhoc_tool_may_take_an_offered_wire_name_and_wins_in_scope` (`crates/runvil-internal/engine/src/execute/tests/adhoc_tools.rs`) keeps passing.
  - `crates/runvil-internal/engine/src/execute/tests/event_vendors.rs` keeps its vendor assertions after `ToolBinding` loses its vendor. Its script-call result alias changes from `tools_echo` to `tools/echo`, and so does the script call's record in `offering.rs`. The interleaved order in `offering-packs.rs` changes as the Technical Design states.
  - `tools.stats.tools` leaves out `dynamic/search` and `dynamic/call`, and the `GetDynamicTools` listing is wrapped with the nonce, with `trusted` false on its `ToolResult`.
  - The exhaustive effect drivers fail to compile until they handle the new variants. Drivers with catch-all arms still compile but would answer a render effect `Dropped` or panic, and tests that step a run by hand expect a Chat effect first, so update every one that runs a section with catalog offers: `crates/harness-gateway-client/src/wire/request-tests.rs`, and in `crates/runvil-internal/engine/src/execute/tests/`, `rounds.rs`, `chat_record_rebuild.rs`, `parallel_tool_calls.rs`, `parallel_tool_calls-withdrawn.rs`, and `models_loop_contract-rounds.rs`. Once test support puts its tools in the catalog, every such test reaches the render effect.
  - Removing `ToolPerformer::survives_stop` removes it from the eight test doubles in `crates/harness-internal/runner/tests/it/` (`support.rs`, `performers.rs`, and `effect_loop-cap.rs`).
  - A render effect takes a provenance sequence number, so tests that pin later effect ids in runs with catalog offers may shift.
  - The test Plugin's check that verb 8 is unsupported (`crates/harness-internal/test-plugin/src/lib-tests.rs`) stays as it is, because verb 8 stays unsupported.
  - No `.rs` file spells the `Debug` trait `std::fmt::Debug`: `git grep -n -F -e "std::fmt::Debug" -- "*.rs"` prints nothing, the debug-spelling check passes over the workspace, and its fixtures show a file with the long spelling reported at each line and a file with only `fmt::Debug` passing.
- Exit criteria, after every work item:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, and `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`.
  - `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`, and `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`, both with `CARGO_BUILD_WARNINGS=deny`, plus `cargo check -p gateway --no-default-features`.
  - `cargo fmt --all --check`.
  - `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, `cargo doc -p runvil --no-deps`, `cargo doc -p harness --no-deps`, and `cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`, all with `RUSTDOCFLAGS="-D warnings"`. CI runs the last two (`.github/workflows/ci.yml`) though the AGENTS.md list omits them, and the private-items build catches doc links to the private items this plan removes.
  - `xtask api --check` on the pinned nightly that `crates/build-xtask/src/api/toolchain.rs` names, with `CARGO_BUILD_WARNINGS` unset.
  - `cargo test -p build-xtask`, `node --test crates/workshop/ui/test/docs-claims.mjs`, and `cargo deny check` against the root `deny.toml`.

</verification-contract>
<decision-record>

## Decision Record

- Decisions:
  - Two capitalized terms name the sets, Static Set and Dynamic Set, and they go into the root `AGENTS.md` Definitions so the two sets keep distinct names. User: "I want the terminology renamed. Static set, dynamic set, and I want all the variables to reflect that, and I want the agents.md at the root of the repository to have one line or two lines... And they're capitalized."
  - Both sets share one type, because they hold the same data and differ only in lifecycle. User: "The static set and the dynamic set have the same data, don't they?"
  - All model-facing tool state is per section entry, because a Pack renders the same tool differently for sections on different models. User: "every section has to be different. Every section needs its own catalog, it needs its own state, it has to be completely fresh. So anything that's going on the harness or that's run wide, anything that you're doing like run wide with these tools, that doesn't sound right."
  - Tool flags are per section: each call holds `survives_stop` from the binding of the section that made it, and the Harness works from the flag on each call. User: "those flags have to be per section".
  - Wire names are assigned and checked at each section's merge, and the run keeps one catalog by id in place of the run-start offering and the Engine's catalog bindings. A tool whose catalog wire name is invalid or clashes now fails the sections that offer it with a clear error, where today it vanishes from the whole run. The user chose this move from the review of the Engine's tool structs.
  - The frozen sets are the one record of what the model was shown: `RenderedTools` drops `dispatch`, `Advertised`, and stored schemas, `Chain.advertised` goes away, and the scope gate and tool-call event read the sets through the frame. Confidence: high. A chain has one parked chat, and `accept_chat` reads the scope before `settle`. The overwrite the `tool_route` comment warns about would happen during dispatch, after the gate, and a trace found no path for it: a child started with `call` gets its own chain (`ChainKind::Call`, `crates/runvil-internal/engine/src/execute/scheduler/dispatch.rs`). The section-bindings tests cover adhoc handlers that run model rounds. The user chose these moves.
  - Every call resolves to its binding once, in `tool_route`, so no second map has to agree with the first. The user chose this in review.
  - The run's existing data is reused: `ToolSet` holds the run's `ToolCatalog` handle and vendors per Pack, as `RunContext` does, in place of a vendor copied onto every catalog tool or binding. The render's `ModelInfo` is the catalog info `bound_model_set` already reads when it builds the round's `ModelBinding`. That binding carries it, and `render_info()` on it supplies the fallback. The user chose this reuse in review.
  - `tools.stats` is computed in Lua from the one id-keyed count map, with the `tools` view filtering out ids the run catalog and the adhoc tools do not hold. No per-Pack or total counter is stored, and the existing increment sites stay as they are. Settled in review.
  - Discovery is one route of `tool_route`'s single lookup, so a model call's name is still resolved once. The route answers or refuses from the frozen sets and hands a `CallDynamicTool` call to the preparers that already exist, with the call's site unchanged. Settled in review.
  - The one wire-name rule is `tool_schema_new`'s, tightened to `[A-Za-z][A-Za-z0-9_-]{0,63}`, because every advertised tool already passes through it and the stricter rule is the one adhoc names already follow. The user chose this in review.
  - A catalog tool's wire name is its `ToolInfo.wire_name`, or else its default, and `Scenario::offer` sets `wire_name`, so the engine tests keep their short wire names. A pass-through render already keeps a catalog `wire_name`, so this applies the same rule from section-bindings on. The user chose this in review over rewriting about 80 tests to default names.
  - Each set places a Pack's tools together at that Pack's first offer, which changes the order the model sees when a section interleaves Packs. The user accepted the change in review over keeping each catalog tool at its own offer position, which needs a special case for a pass-through render.
  - A script call reports the tool's id as its alias, because wire names belong to a section's model while a script names tools by id. Settled in review.
  - Each discovery refusal reports `TOOL_CALL_FAILED` with a trusted `ToolResult`, as a failed model call reports its failure, and a search with no match returns an empty array. Settled in review.
  - Stale docs are fixed in the step that finds them, in either repository, never deferred. User: "fix the docs whenver you find a problem. dont leave it for later. stale docs confuses models".
  - A pre-existing intermittent failure in `gateway-logging` (`a_protected_producer_times_out_into_preallocated_loss_accounting`: on Windows a condvar timeout can report before the producer's deadline) blocked verification in steps 3 and 10, so its fix landed as an explicit prerequisite commit below step 10, outside the numbered steps: "Reject a blocked log producer only after its deadline".
  - The round's model name is recorded on the message list when the round is committed and read from the calling frame, because the list already keeps its last send and a section runs one loop at a time. `ChatAnchor` is the wrong store: `settle` takes the in-flight model inside `accept_chat`, before the calls are issued, and `measured` keeps the id only when the provider reported usage. The user chose the message list in review, over threading the name through the loop.
  - Before the merge, offers are deduped by id, and every rule that compares wire names runs in the merge, because a render may rename a tool and avoid a clash. A Dynamic Set tool is dropped for sharing an id with the Static Set, never for sharing a name, so a name clash across the sets fails the merge, even a static adhoc tool beside a dynamically offered Pack tool, which today is skipped quietly. The user chose these moves, the last in review.
  - Every tool call's origin names its section entry, and `render` receives the same entry id, so a Pack can tie a call to the render that shaped it. The user chose this move, and kept it in review.
  - `ToolSet` keeps its name. A rename to `RunToolSet` would touch 80 references in 30 files with no behavior change. The user dropped the rename in review.
  - The Lua-facing catalog stays per run, read once from each Pack's `Pack::tools()`, because it is the same for every model. The user chose "Keep it per run for now".
  - The discovery tools are built into the Engine, because the Engine holds a section's Dynamic Set and the Dynamic Set holds tools from any Pack. User: "The discovery tools have to be built in. To the engine, right?"
  - The discovery tools live under the Pack name `runvil/dynamic`, with ids `dynamic/search` and `dynamic/call` and the wire names Cursor uses for the same pair, `GetDynamicTools` and `CallDynamicTool`. User: "what about runvil/dynamic", "dynamic/search and dynamic/call", and "The model-facing names should be GetDynamicTools and CallDynamicTool".
  - The `GetDynamicTools` description ends with only the Dynamic Set's wire names, because a tool the section offered statically is already in the model's tool list. User: "GetDynamicTools renders in the description a comma-separated list of the wire names of the dynamic tool set. In Runvil, this should exclude the names of what is already in the static tool set".
  - The parameters copy Cursor's names where they mean the same thing (`toolName`, `pattern`, `arguments`) and drop `namespace`, because wire names are flat and unique in a section. A `namespace` key is accepted and ignored, for models used to Cursor's shape. User, delegating the call: "use your judgement".
  - Search results follow Cursor's shapes and limits, the name list stops after 100 names, and `pattern` uses the `regex` crate, because models that know Cursor send regexes. Settled while writing this plan.
  - A model call that names a Dynamic Set tool directly gets refusal text pointing to `CallDynamicTool`, which keeps the Dynamic Set's definition strict. The user chose "Refuse it" over running the call.
  - Every Harness Pack renders late through one hook, called for every Pack the section offered from, even when it only passes its tools back. User: "the hook has to be for every pack. Every pack should... render late... Even if the only thing it does is just pass back its normal Lua facing catalog."
  - One hook covers every kind of shaping: Pack tools have a Lua-facing catalog and a late render, and the results merge. User: "there's no light hook. All the tools are the same. They have the Lua facing catalog, and then they have the late rendering hook, which creates the model facing catalog, and then those all get merged".
  - Whether Lua sees a rendered tool depends on whether the run catalog holds it. User: "the per-tool-choice should not be explicit it should naturally fall out of the API."
  - One Pack may mix Lua-visible and model-only tools, such as a files Pack whose read and write tools Lua sees while its edit tool is shaped per model. User: "a pack can have some tools that are non-shaped, and some tools that are shaped. It doesn't have to be either or".
  - `render` receives the model's facts through a `RenderContext`, following `ToolContext` (`crates/runvil-plugin/src/context.rs`), so a Pack can size its tools to the model and later fields join without breaking existing Packs. User: "the pack needs as much information about the model as possible" and "Maybe for a big model, it offers eight shaped tools. For a medium-sized model, it offers five, and for a small model, maybe it only offers two."
  - General behavior comes from a small API with general rules. User: "The number one imperative is do more with less."
  - The render is an effect at the freeze, because `render` takes what the section offered, known only then, and an effect keeps the Engine pure and puts the render in the run log. The effect holds full `ToolInfo`, so a pass-through answer echoes it, and its record lists ids. Settled while writing this plan.
  - `render` runs for each set the Pack was offered into, with the same inputs for either set, and a set the Pack was not offered into is not rendered. An earlier design kept shaped Packs out of dynamic offers; this plan reverses that, because MCP will render its server tools into the Dynamic Set. Settled while writing this plan and in review.
  - A render may reshape, rename, or drop offered tools and add model-only tools, and every catalog tool it returns is one the section offered in that set, so Lua's offer stays the upper bound on the catalog tools the model sees. `Continuation::Render` holds only the resolved binding; the offered ids are recomputed from the parked frame when the answer lands, because that section's VM runs no Lua while parked and its offers cannot change. Settled while writing this plan and in review.
  - A merge failure fails the section's model call, so a Pack bug or a name clash shows up as a clear error: a broken merge rule through its own Engine error naming the rule, and a bad name or schema through `BindSchema`, as today. Confidence: medium, because today a colliding run-start name logs one line and drops the tool. Settled while writing this plan and in review.
  - MCP server tools always land in the Dynamic Set, and Lua moves the ones it wants into the Static Set. User: "it puts them in the dynamic tool set, which it has to. And then it's up to the Lua to decide if it wants to move it into the static tool set."
  - A binding with no catalog `ModelInfo` gets one built from the binding, with `ThinkingMode::Never`, the claim that asks least of the model, so every model call has one. Chosen over skipping the render for that round and over refusing a model named by raw id; the thinking mode was settled in review.
  - `bound_model_set` sets the catalog `ModelInfo` on `ModelBinding` through a new public builder beside `with_provider`, `with_catalog_info(ModelInfo)`, so the facade gains two `ModelBinding` lines, this builder and `render_info()`, where the Technical Design counts one. `ModelBinding` lives in `runvil-model-client` and `bound_model_set` in `runvil-engine`, so the private field can be set across crates only through a public method, and the facade already lists every `ModelBinding` builder (`with_capabilities`, `with_invocation`, and `with_provider`). Confidence: high. Settled while decomposing this plan into steps.
  - `tools.stats` replaces `tools.calls`, with `total`, `packs`, and `tools`, so Lua tracks progress cheaply while model-only names stay with the model. User: "we can gather a bunch of statistics cheaply, which the Lua can then optionally use to determine the amount of progress being made, without necessarily knowing the exact names of the model-facing shaped tools." On cost: "LOL there are no other prompts to speak of".
  - The model name and the section entry go on `ToolCallOrigin`, because `ToolCaller` derives `Copy` and `Hash` and 15 files name its variants (`crates/runvil-internal/types/src/tools/origin.rs`). Settled while writing this plan.
  - The Plugin entry-point work comes after this plan, with two cheap steps now: reserve the `RV_PACK_RENDER` number, and keep `render`'s inputs and outputs plain data. User: "should we do the entry point stuff now and make sure that the linked and DLL plugins follow the API or do it later?", then "I mean put the cheap steps in the plan".
  - Per-section bindings land first, then the discovery tools, so dynamic offers work before any render work, and the render is split so the Engine half lands with every Pack passing through. User, accepting the reorder: "yeah". In review, the resolve-once edit joins the section-bindings behavior commit, because both edits are `tool_call.rs` and the second lookup exists only while the run-start offering does. The Harness render and the routing became one item, because they edit the same three files and `ToolPerformer` should change once, and the reservation became free to land beside section-bindings. In a later review, render-hook moved ahead of section-bindings, because test support names tools through `ToolInfo.wire_name`.
  - Each work item lands as one commit, or as the separate commits it names, with its tests in the same commit, and docs changes in `runvil-docs` are commits in that repository. Settled while writing this plan.
  - The `Debug` trait has one spelling, `fmt::Debug`, and every other `std::fmt::` path stays as written. The user asked that the code base not hold both `std::fmt::Debug` and `fmt::Debug`, and that the rule be enforced. No clippy lint can enforce it: both spellings resolve to the same trait, so `disallowed_types` cannot tell them apart, and `absolute_paths`, the only lint that reads spelling, flags every path of three or more segments. So a structural check in `build-xtask` reads the source text, and it runs under `cargo test -p build-xtask` and the workspace nextest run. A `macro_rules!` body is no exception: it writes `fmt::Debug` like any other code, and none names the trait today. A `std::fmt::{...}` group that lists `Debug` is refused too, because it brings in a bare `Debug`, a second spelling. It lands last, as its own step, because it is independent of the tool work. Settled while amending this plan.
- Rejected alternatives:
  - Several run-wide copies of the catalog: `ToolSet.offered`, `RunState.catalog_bindings`, a `RunState` `ToolCatalog`, or a catalog entry that copies its Pack's vendor. Reason: once names move to the merge they hold the same thing, and vendors belong to Packs. `ToolSet` holds the one `ToolCatalog` handle instead. Revisit: closed.
  - Converting the catalog into a `BTreeMap<ToolId, ToolInfo>` on `ToolSet`. Reason: `ToolCatalog` is already an `Arc<[ToolInfo]>` with a by-id lookup, so the map only deep-clones every description and schema. Revisit if a catalog ever grows large enough for the scan to matter.
  - Vendor on `ToolBinding`, stamped at the merge. Reason: it copies one per-Pack string onto every binding in every section, and the three report sites all have the run's `ToolSet` in hand. Revisit: closed.
  - A wire-name index (`BTreeMap` or a map plus an order list) beside either frozen set. Reason: the sets are tens of tools, schema order is the vec order, and an index would store every wire name twice. Revisit if a rendered set grows past hundreds of tools.
  - Stored `total` and per-Pack counters beside `ToolCallCounts`. Reason: both are sums over the one id-keyed map, so stored copies need a bump at every ensure and increment site to agree with it. Revisit if a Lua read ever has to be cheaper than one snapshot.
  - A discovery check outside `tool_route`, or a discovery dispatch or continuation type. Reason: a check outside `tool_route` looks the name up a second time, and inline answers plus the two existing preparers already cover every outcome. Revisit: closed.
  - `Effect::Render` carrying `ModelInfo` instead of the binding. Reason: its record could not derive the alias, which is a prompt-local label a `ModelId` does not hold (two aliases can name one model), and the resolved binding has to be kept for the second dispatch anyway. Revisit: closed.
  - `Continuation::Render` storing the message list and the offered ids beside the binding. Reason: a parked chain's frame still holds both, production builds one `MessageList` per section VM, and that section's VM runs no Lua while parked, so the stored copies can only disagree with the frame. Revisit if a section can ever re-offer while a render is in flight.
  - `render` taking both sets at once, through a pair type in `runvil-types`. Reason: the per-set call keeps `render`'s argument the same `Vec<ToolInfo>` shape as `tools()`, and a Pack that needs to know which set it is rendering gets that from `RenderContext` later (see Deferred). Revisit when MCP rendering is planned.
  - Keeping `dispatch` and `Advertised` beside `static_set`, storing `schemas` beside `static_set`, or copying section data onto the chain as `Chain.advertised` or a chain copy of the Dynamic Set. Reason: each is a second copy of the frozen sets. A schema is `tool_schema_new` of the binding. Revisit if a path turns up where the chain's frame differs from the round's section when its answer arrives, or if the schema projection stops being pure.
  - Threading the round's model through `Continuation::Chat`, `ChatResult`, the loop's yields, the request parser, and `CallSite`. Reason: the message list already knows its last round, and the threading touches six files, one of them near its 500-line ceiling (`crates/build-ceiling/src/lib.rs`). Revisit if a section ever runs two loops over one list at once.
  - A new map on `RunState` from each binding to its `ModelInfo`, or keeping the `ModelBindings` journal on `RunState` for the render to look up. Reason: `prepare_chat` already holds the round's `ModelBinding`, and `RunState::new` folds the journal into `ModelSet` and does not keep it. Revisit: closed.
  - Reusing `Error::BindSchema` for merge failures. Reason: its message describes a schema build failure. Revisit: closed.
  - Other names: `RunToolSet` or `RunCatalog` for `ToolSet`, the Pack name `runvil/tools`, the id `dynamic/list`, and model-facing names derived from the ids (`dynamic_search`, `dynamic_call`). Reason: the user chose the names above. Revisit: closed.
  - Registering `runvil/dynamic` as a registry built-in beside `runvil/subagents`. Reason: `HostContext::pick` (`crates/harness-internal/runner/src/host.rs`) puts every `runvil` Pack into every run's roster unless a `uses:` entry takes its id (`a_run_over_a_fresh_registry_holds_a_usable_subagents_pack_with_no_tools`, `crates/harness-internal/runner/src/host-tests.rs`), so a built-in `dynamic` would trip the Engine's reservation on every run and show Lua an empty Pack. Revisit if built-ins stop joining every roster.
  - Doing the Plugin entry point first. Reason: it is a separate, larger job, `render` maps one to one onto a verb, and the C contract is at version 0.1 and free to change (`crates/runvil-plugin/src/abi.rs`). Revisit when a library Plugin needs to shape its tools.
- Assumptions, risks, and notes:
  - Assumption: models often call a tool they have just discovered by its own name. That is why the scope gate lets Dynamic Set names through, so the call gets refusal text and the loop continues.
  - Assumption: the only readers of `tools.calls` are the gitignored `local/briefer.md` and `local/prompts/briefer.md`, which are already broken.
  - Risk: the tightened `tool_schema_new` rule fails the sections that offer a tool whose default name does not start with a letter or runs past 64 characters, or whose render or `Pack::tools()` picks a name with a `.`, all of which today's naming lets through. Id segments may start with a digit, `-`, `_`, or `.` (`crates/runvil-internal/types/src/names.rs`), and default names never hold a `.`.
  - Note: merge failures keep the error kinds `BindSchema` maps to, Lua kind `internal` and run error kind `Binding`, though the guide defines `internal` as a failure the author cannot act on (`src/language/05-lua-environment.md` in `runvil-docs`). The error's text names the tools and the rule, which is what the author acts on, and other author-facing failures, such as a missing model, already use `internal`. Revisit if the error kinds are reworked.
  - Note: two tests in `crates/workshop/ui/test/docs-claims.mjs` read the whole root `AGENTS.md`, even the Definitions section that its wording scan skips. They fail on "activat", "TOML config", "three named buckets", or "three homes", so the new Definitions bullets avoid them.
  - Note: an old `ToolInfo` fixture in `a_tool_info_logged_without_survives_stop_reads_as_not_surviving` (`crates/runvil-internal/types/src/tools/tests.rs`) holds `"wire_name":"fetch"`. With the new field it would read as `Some("fetch")`, so the key is removed and the test keeps pinning only the missing `survives_stop`. This supersedes the one-offer API plan's note against naming the field `wire_name` (`vibe/2026-10-23-1-one-offer-api.md`), because that fixture is the one stored `ToolInfo` in the repository.
  - Note: the facade does not re-export `Pack`, so `ToolInfo`'s doc names `Pack::render` in plain text. An intra-doc link would fail `xtask api --check`.
  - Note: the discovery tools are modeled on Cursor's pair, `GetDynamicTools(namespace?, toolName?, pattern?)` with an RE2 pattern of at most 256 characters, and `CallDynamicTool(namespace, toolName, arguments, mcpDetails?)`. The capture is `research/2026-10-14-harness-mcp-dynamic-tools.md`, Appendix A, in the `runvil-design` repository.
  - Note: `ToolCallOrigin.section` and `ToolCallOrigin.execution` are the section's and the run's names, so two entries of one section share both. That is why the origin gains the entry id.
  - Note: the plan was checked against `runvil` commit `c27899e7a` with a clean tree. Line numbers may drift, and symbol names are the reliable anchors. The repositories are `c:\Users\Vinnie\cursor\runvil` and `c:\Users\Vinnie\cursor\runvil-docs`.

### Deferred and Out of Scope

- Deferred:
  - The Plugin entry-point plan: C Plugins inserting Packs, each registered Pack holding its Plugin's one entry function, which takes a verb and a pointer to a size-prefixed, verb-specific struct; verbs for a Pack's Lua-facing tool list; `RV_PACK_RENDER` itself, with a serializable `ModelInfo` and `RenderContext`; and moving `plugin-web`, `plugin-user-input`, and `plugin-mcp` onto the verbs. Today a C entry point loads, and Pack insertion is still to come (`crates/runvil-plugin/src/lib.rs`). Revisit after this plan lands.
  - MCP rendering its server tools, which retires the MCP Pack's own `mcp/get_dynamic_tools` and `mcp/call_dynamic_tool` (`crates/plugin-mcp/src/dynamic.rs`), and the Dynamic Set growing after the freeze for a server that connects late. MCP server tools always land in the Dynamic Set, and Lua moves the ones it wants into the Static Set, so that plan must let Lua name server tools and say how a static offer of the whole MCP Pack behaves, because this plan renders each Pack only for the sets Lua offered it into. It may also revisit reading `Pack::tools()` per section entry. Revisit when MCP rendering is planned.
  - Per-Pack summaries and MCP server instructions in the `GetDynamicTools` description, in place of the capped name list. Revisit when the name list proves too thin for a model to choose from.
  - Rendering for Engine-owned Packs, for a shaped subagents task tool. `runvil/subagents` reserves its id today, and its tools are still to be built (`crates/runvil-plugin/src/registry.rs`). Revisit when the subagent tools are built.
  - A `failed` counter in `tools.stats`, for calls whose caller sees an error. It needs a bump wherever a failure becomes known: an `Err` result and a failed structured-output conversion in `accept_tool_call` (`crates/runvil-internal/engine/src/execute/scheduler/apply.rs`), a refused access fork in `prepare_catalog_call`, a bad return or raise in `dispatch_adhoc_tool_done`, each discovery refusal, and the `Dropped` arm of `apply_answer`. A cancel would stay out of it. Revisit when a prompt needs it.
  - More model facts on `ModelInfo` or `RenderContext`, such as model size, and where the Host gets them. Revisit when a Pack needs them to shape its tools.
  - A set marker on `RenderContext` saying whether the offered tools came from the Static Set or the Dynamic Set. A Pack cannot tell today, and MCP may need to. `RenderContext` is `#[non_exhaustive]` and built only by the Harness, so the field joins later without breaking existing Packs. Revisit when MCP rendering is planned.
  - Filters, tool metadata, refusals for out-of-scope static calls, and identical re-offers after the freeze. Revisit each when a prompt or Pack needs it.
- Out of scope:
  - Changes to how Lua offers tools.
  - Cursor's `mcpDetails` and its approval-card behavior.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked`, which builds only the gateway (`default-members` is `crates/gateway/app`, whose binary is `runvil-gateway`). Workshop, the desktop app: `cargo workshop` (the alias for `build-workshop`), which builds the gateway, stages it as the Tauri sidecar, builds `-p workshop`, and removes the staged copy. A bare `cargo build --locked -p workshop` works only after the gateway is built, because the desktop build script copies `target/<profile>/runvil-gateway` into `crates/workshop/desktop/binaries/` and tauri-build reports a missing sidecar. The Engine crates at the 1.95 floor, as CI's msrv job builds them (the 1.95 toolchain is installed): `cargo +1.95 build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`.
- Focused test command pattern: `cargo nextest run --locked -p <crate> --all-features <test-name-substring>`, for example `cargo nextest run --locked -p runvil-lua --all-features offer`; `-E 'test(<regex>)'` selects by filter expression instead. Drop `--all-features` for `workshop`, `workshop-server`, and `workshop-server-api`, and build the gateway first when `workshop` is selected. One Node test file: `node --test <file>` from its package directory, after `npm run build` there when the file reads `dist/`.
- Component test command pattern: `cargo nextest run --locked -p <crate> [-p <crate> ...] --all-features`. Engine family: `cargo nextest run --locked -p runvil -p runvil-plugin -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client --all-features`. Harness family: `cargo nextest run --locked -p harness -p harness-runner -p harness-plugin-loader -p harness-test-plugin -p harness-gateway-client --all-features`. Structural checks: `cargo test -p build-xtask`. Workshop UI packages: `npm run build --workspace ui`, then `npm test --workspaces --if-present`, both from `crates/workshop`. Gateway config UI: `npm test` in `crates/gateway/config-ui/ui` (its `pretest` builds first). Both npm installs are present; after a lockfile change, rerun `npm ci --prefix crates/workshop` and `npm ci --prefix crates/gateway/config-ui/ui`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api` with the gateway built first. The workspace run includes the `build-xtask` structural checks. cargo-nextest 0.9.128 is installed; the `default` and `ci` profiles and the STT test group are in `.config/nextest.toml`.
- Linter command: `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`, then with the same variable `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets` with the gateway built first. Clippy covers `cargo check`, so the only extra check is `cargo check -p gateway --no-default-features`. Companion gates from CI and the pre-push hook:
  - wording scan: `node --test crates/workshop/ui/test/docs-claims.mjs` (no `npm ci` needed, runs from any directory);
  - structural checks: `cargo test -p build-xtask`, or `cargo xtask tidy` for the same report without the test runner;
  - facade surface: `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`, which compares against `crates/runvil/public-api.txt`, plus `cargo +nightly-2026-09-05 nextest run --locked -p build-xtask --run-ignored only` for the nightly-only fixtures (the pinned nightly is installed);
  - UI types: `npm run typecheck --workspaces --if-present` in `crates/workshop` and `npm run typecheck` in `crates/gateway/config-ui/ui`;
  - supply chain: `cargo deny check` and `cargo audit` (both installed), and `cargo tree --locked -p gateway -e normal -i ring --depth 0` must find no `ring`, because the gateway links only the aws-lc-rs rustls backend.
- Formatter check command: `cargo fmt --all --check` (rustfmt `style_edition = "2024"`; the pre-commit hook runs it). The TypeScript packages have no formatter or linter config; `tsc --noEmit` typechecks them.
- Docs command: `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`. With the same flags, CI also runs `cargo doc -p runvil --no-deps` and `cargo doc -p harness --no-deps` (default features, as consumers read them), `cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`, and on Windows `cargo doc --locked --no-deps -p workshop-server --document-private-items`. The docs site books build from the sibling `runvil-docs` checkout: `$env:RUNVIL_DOCS='c:\Users\Vinnie\cursor\runvil-docs'; cargo xtask site --books-only` (mdBook is on PATH; `MDBOOK` overrides it).
- Test placement and naming conventions:
  - Unit tests: a small file keeps an inline `#[cfg(test)] mod tests { ... }` at its end (about 150 files do); otherwise the tests sit in a sibling `foo-tests.rs` wired as `#[cfg(test)] #[path = "foo-tests.rs"] mod tests;`, split further as `foo-tests-<label>.rs`. A module directory uses `foo/tests.rs`, which wires its splits as `#[path = "tests-<label>.rs"] mod <label>;` (for example `lua/src/tools/tests.rs`). Large suites get a `src/<module>/tests/` directory, one file per topic with `<topic>-<label>.rs` splits (for example `engine/src/execute/tests/debug_and_counts-infer-rounds.rs`).
  - Integration tests: one binary per crate at `tests/it/main.rs`, or `tests/suite/main.rs` for the `runvil` and `harness` facades, with topic modules beside `main.rs` and data in `tests/fixtures/`, `tests/prompts/`, or `tests/common/`. A few crates keep standalone targets: `gateway/stt/backend-whisper/tests/native_*.rs`, `gateway/stt/engine/tests/*.rs`, `gateway/cloud-providers/tests/sheet_binary.rs`, and `build-workshop/tests/interruption.rs`.
  - Shared test helpers: `runvil-engine`, `runvil-lua`, `runvil-parser`, and `runvil-plugin` expose a `test_support` module behind `#[cfg(any(test, feature = "test-support"))]`, and no non-dev dependency anywhere may enable an Engine or Harness crate's `test-support`. Gateway and Workshop crates use a `test-fixtures` feature instead. Lua tests build VMs with `crate::test_vm` (`TestVm`, `lua_over`, `echo_tool`, `tool_at`); Engine execute tests share `execute/tests/fixtures.rs`.
  - Test functions are snake_case sentences stating the behavior, for example `an_offered_adhoc_tool_counts_from_zero_and_an_unoffered_one_from_its_first_call`.
  - `unwrap` and `expect` are allowed in tests only. No Engine test sleeps, reads the wall clock, waits with a timeout, or draws OS entropy (the Engine `clippy.toml` files ban those calls); pseudo-random input comes from a fixed seed the failure prints; temporary directory names come from the test name, the process id, and a counter. No doc comment holds a compiled code block, so the workspace has no doctests.
  - Node tests run under `node --test`: `crates/workshop/ui/test/<topic>.mjs` (kebab-case, many under jsdom), `crates/workshop/look/test/` and `crates/workshop/platform/test/`, `crates/gateway/config-ui/ui/src/**/<name>.test.mjs` beside the module, and `tools/<script>.test.mjs` beside each script.
- Directory map:
  - `crates/`: every Rust crate plus the web UI packages. The top level is the public and shared layer: the Engine facade `runvil`; `runvil-plugin`, the contract every Plugin implements; the Harness facade `harness` and `harness-gateway-client`; the gateway's shared vocabulary and discovery crates `gateway-api-types` and `gateway-api-discovery`; the Plugins `plugin-web`, `plugin-mcp`, and `plugin-user-input`; `shared-error-source` and `shared-loopback`; and the `build-*` tooling. `crates/shared-ui` is a TypeScript and CSS package, not a crate.
  - Manifestless containers hold each family's private crates: `runvil-internal/` (`types`, `vfs`, `model-client`, `lua`, `parser`, `engine`), `harness-internal/` (`runner`, `plugin-loader`, `test-plugin`), `gateway/` (`app`, which builds `runvil-gateway`, plus `cloud-providers`, `config`, `config-ui`, `local`, `logging`, `progress`, `protocol`, `routing`, `web-search`, and the `stt/` subsystem: `api`, `engine`, `backend-whisper`, `whisper-ffi`), and `workshop/` (`desktop`, the Tauri app whose package is `workshop`; `server`, `server-api`, `agents`, `gateway`, `menu`, `protocol`, `registry`, `run-log`, `status`, `support`, `user-state`, `workspace`; and the npm workspace `ui`, `look`, and `platform`, installed once at `crates/workshop`).
  - `crates/build-*`: `build-xtask` (`cargo xtask`: `tidy` structural checks, `api` facade surface, `site`, `new-crate`), `build-workshop` (`cargo workshop`), `build-ceiling` (the 500-line file check every build script runs), `build-ui` (esbuild bundling for the UI crates' build scripts), `build-user-guide` (stages the guide books from `runvil-docs`), and `build-llama-cuda` (the CUDA `llama-server` release builder).
  - `guide/`: mdBook configs for the gateway, language, and Workshop books, plus site chrome and the landing page; the chapters live in the separate `runvil-docs` repository.
  - `prompts/`: example prompt files.
  - `tools/`: Node scripts that stage the gateway sidecar and run live TTS checks, each with a `.test.mjs`.
  - `vibe/`: dated plan and design records named `YYYY-MM-DD-N-<slug>.md`, with older plans filed into `YYYY-MM/` folders.
  - `local/` (gitignored): machine-local gateway and MCP service configs and env files, profiles, prompts, and STT fixtures.
  - `.github/workflows/`: `ci.yml` (the gate, whose `ci-green` job requires every other job) plus release, nightly, docs site, STT Miri, and native library workflows. `.githooks/`: pre-commit (fmt) and pre-push (docs-claims, headless gateway check, clippy, deny). `.cargo/config.toml`: the `xtask` and `workshop` aliases and the Windows `rust-lld` static-CRT link. `.config/nextest.toml`: nextest profiles. `.cursor/rules/`: the Workshop architecture and SPA rules.
  - Root files: `Cargo.toml` (members, shared dependency pins, workspace lints), `rust-toolchain.toml` (stable), `rustfmt.toml`, `clippy.toml`, `deny.toml`, `dist-workspace.toml` (cargo-dist), `gateway.local.example.toml`, and `AGENTS.md`. `images/`: README art. `target/`: build output.
- Component boundaries:
  - Engine (`runvil`, `runvil-*`): `runvil-types` and `runvil-vfs` at the bottom; `runvil-model-client` on types; `runvil-lua` on model-client, types, and vfs; `runvil-parser` on lua and types; `runvil-engine` on lua, parser, model-client, types, and vfs; the `runvil` facade on all six; `runvil-plugin` on types and vfs. No Engine crate depends on `tokio`, `tokio-util`, `async-trait`, `reqwest`, `getrandom`, or `fastrand`, even transitively, or on any gateway, Workshop, or Harness crate.
  - Harness (`harness`, `harness-*`): `harness-runner` on `runvil` and `runvil-plugin`; `harness-plugin-loader` on `runvil-plugin`; `harness` on runner, plugin-loader, `runvil`, and `runvil-plugin`; `harness-gateway-client` on `harness`, `runvil`, and `plugin-web` (the one allowed Plugin edge, for web's `SearchProvider`). Outside their family they name only `runvil` and `runvil-plugin`, and none takes `tokio` or `tokio-util` as a normal dependency.
  - Plugins (`plugin-*`) depend only on `runvil-plugin`, `shared-*`, and outside libraries.
  - Gateway (`gateway`, `gateway-*`) never depends on Engine, Harness, or Workshop crates. Inside the family: `gateway-api-types` at the bottom; `gateway-config` on it; `gateway-protocol` on api-types and config; `gateway-routing` on config and protocol; `gateway-local` on config, progress, protocol, and routing; `gateway-stt`, the only public member of the `stt/` subsystem, on config, local, progress, and its subsystem crates; the `gateway` app on all of them plus `shared-loopback`.
  - Workshop: the desktop app `workshop` depends only on `workshop-server-api` and `gateway-api-discovery`, and `workshop-server-api` only on `workshop-server`. The private crates form one-way tiers: vocabulary (`workshop-protocol`, `workshop-registry`, `workshop-support`), then services (`workshop-gateway`, `workshop-menu`, `workshop-status`), then features (`workshop-agents`, `workshop-run-log`, `workshop-user-state`, `workshop-workspace`), then `workshop-server`, which also depends on `harness`, `harness-gateway-client`, `runvil`, `runvil-plugin`, the three Plugins, `shared-loopback`, and `gateway-api-discovery`. Same-tier crates never depend on each other, except `workshop-registry` on `workshop-protocol`, and subsystems meet through `workshop-registry` slots that `crates/workshop/server/src/app/compose.rs` fills. Workshop reaches the gateway only through `gateway-api-types` and `gateway-api-discovery`.
  - Across families: outside the Engine family a crate names only `runvil` and `runvil-plugin`, and outside the Harness family only `harness` and `harness-gateway-client`; each container is private to its family; `shared-*` crates depend on no product crate. `cargo test -p build-xtask` enforces the family, container, facade, and Workshop tier rules; the edges listed inside each family are what the manifests declare today.
- Conventions summary:
  - Five capitalized defined terms (Engine, Harness, Host, Pack, Plugin) have one meaning each everywhere, plans included; other senses take other words ("machine", "real files", "the gateway's speech engine", "test support"). Engine crates call whatever steps a run "the caller" and never mention the Host. `node --test crates/workshop/ui/test/docs-claims.mjs` enforces the parts a scan can see.
  - Strict workspace lints that every crate inherits: clippy `all` and `pedantic` denied, `unwrap_used` and `expect_used` denied outside tests, `todo!` and `process::exit` denied, `#[expect(.., reason = ..)]` instead of `#[allow]`, `unsafe_code` denied except in six allowlisted boundary crates, and `missing_docs`, `unreachable_pub`, and `missing_debug_implementations` warned, which CI's `CARGO_BUILD_WARNINGS=deny` turns into errors, so every module and public item has `//!` or `///` docs. Process-global installers (panic hook, global logger or subscriber, rustls default provider) are banned outside binary entry points.
  - Every `workshop-*`, `harness-*`, and `plugin-*` crate opens `lib.rs` with a `//! ## Invariants` section stating what it may and may not depend on. Workshop `lib.rs` files hold only docs, attributes, `mod` declarations, and `pub use`. The `runvil` and `harness` facades hold only doc-commented `pub mod` groups and single-item `pub use` re-exports, with no `doc(hidden)`.
  - Flat source directories: one or two related files sit beside the parent as `foo-bar.rs`, wired with `#[path = "foo-bar.rs"] mod bar;`, and three or more become a `foo/` directory.
  - Every Rust file under `src`, `tests`, `benches`, or `examples` stays at or under 500 lines; each crate's build script runs `build-ceiling`, whose failure message explains how to split without widening visibility.
  - Comments state only non-obvious constraints, and every workaround cites its upstream issue URL.
  - Error and status messages are written for model consumption: concise, self-contained, and naming what is unmet with required versus actual.
  - JSON that reaches a recorder or a replay comparison round-trips exactly (`serde_json` with `float_roundtrip`, sorted keys, finite numbers).
  - Behavior changes ship with tests in the same change. Types and compiler checks come first, then behavior tests and deterministic fault injection, and gates are yes/no properties, never score thresholds.
  - Cargo features gate real constraints (a toolchain requirement or a heavy native build), never product shape. Runtime and serve paths never compile native code or invoke build tools, and no build step writes into the repository (CI fails a dirty tree).
  - Dependencies are declared once in `[workspace.dependencies]`, with a comment explaining each non-obvious pin or feature choice, and members inherit them with `workspace = true`.
  - Workshop UI: TypeScript bundled by esbuild, CSS beside its `.ts`, the `.ws-` class prefix, kebab-case files, design tokens instead of raw color, size, or spacing values, and every persisted value through the `ui-storage` adapter to the server.
  - Rust edition 2024 on the stable toolchain, with a 1.95 floor for the Engine crates.
  - Commit subjects are short imperative sentences (`Make tools.adhoc all or nothing`, `Refuse offered adhoc objects that no adhoc tool holds`), and each plan ends with a `Close plan: <slug>` commit.

</project-survey>
<execution-plan>

## Execution Instructions

The steps run in order, and each lands as one commit holding its code and its tests, with the step's name as the commit subject. Steps 4, 7, 9, and 13 commit in `c:\Users\Vinnie\cursor\runvil-docs`; every other step commits in `c:\Users\Vinnie\cursor\runvil`. Run each Tests command as written from `c:\Users\Vinnie\cursor\runvil`, one at a time, in Windows PowerShell 5.1, which has no `&&`. A command listed with an expected output passes when it prints that output; every other command passes when it exits 0.

Components, in dependency order, each with its pieces:

- Pack render contract (step 1): `Pack::render`, `RenderContext`, and `ToolInfo.wire_name`, the contract a Pack author codes against. It comes first because the per-section tool sets' test support names tools through `ToolInfo.wire_name`, and the Harness render implements the hook. One piece.
- Per-section tool sets (steps 2 to 4): each section entry names, checks, and freezes its own Static Set and Dynamic Set from the run catalog. It follows the render contract, whose `wire_name` it honors, and comes before discovery, which resolves calls in the frozen sets and joins the merge. Its pieces, the set terms, the section bindings, and their guide docs, are built in sequence: the bindings' code names use the terms, and the docs quote the rules and errors the bindings land.
- Dynamic tool discovery (steps 5 to 7): the reserved Pack id `dynamic` and the `GetDynamicTools` and `CallDynamicTool` route. It follows the per-section tool sets, because the route reads the frozen sets and merge rule 4 adds its tools. Its pieces, the reservation, the route, and its guide docs, are built in sequence: the discovery tools live under the reserved id, and the docs quote the tools' text and refusals.
- Tool stats (steps 8 and 9): `tools.stats` in place of `tools.calls`. It follows discovery, because `packs.dynamic` counts discovery calls and the `tools` view leaves their ids out, and comes before late rendering, whose model-only calls count under `packs`. Its pieces, the table and its guide docs, are built in sequence, because the docs quote the table's errors.
- Late rendering (steps 10 to 13): call origins, the render effect, and the Harness render. It follows the tool stats, because the render feeds the merge, its model-only tools count through `tools.stats`, and a Pack ties a call to its render through the origin's entry id. Its pieces, caller origins, the render effect, the Harness render, and their guide docs, are built in sequence: the effect reads the entry id the origins add, the Harness performs the effect, and the docs describe what the Harness render lets a Pack do. The three code pieces also share `tool_call.rs`, `chat.rs`, `apply.rs`, and `pending.rs`, which rules out building them side by side.
- Debug trait spelling (step 14): the `fmt::Debug` spelling across the code base and the `build-xtask` check that keeps `std::fmt::Debug` out. It comes last because it is independent of the tool work and runs after every earlier step has touched its files, so the rewrite reads each file in its final form and no later step brings the long spelling back. One piece, the rewrite and the check together, because the check's live test fails until the rewrite is done.

<step-1>

### Step 1: Add the Pack render hook and ToolInfo wire names [completed]

- Component: Pack render contract
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo build --locked`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo check -p gateway --no-default-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p harness --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `cargo deny check`
  - `cargo +1.95 build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`

Before the first edit, run `npm ci --prefix crates/workshop` and `npm ci --prefix crates/gateway/config-ui/ui`, because the build scripts bundle both UIs, and run `rustup toolchain install` for `nightly-2026-09-05` (which `xtask api` needs) and `1.95` (the Engine floor CI's `msrv` job builds) if `rustup toolchain list` lacks either.

This step adds the contract a Pack implements to shape its tools for one section entry and model. Nothing calls `render` yet, and no Engine code reads `wire_name` until step 3.

- `RenderContext<'a>` in `crates/runvil-plugin/src/context.rs`, beside `ToolContext`: `#[non_exhaustive]`, built with `RenderContext::new`, with two public fields, `entry: &'a str` (the section entry, the value Lua reads as `sys.id`) and `model: &'a ModelInfo` (the model of the section's first round). A section entry is one execution of a section: re-entering a section, a fan-out arm, a task, and a child started with `call` are each their own entry. The context holds plain data only, free of Rust-only handles such as `HostServices`, so a later C verb can pass it serialized, and later facts join as fields without breaking existing Packs.
- `Pack::render` in `crates/runvil-plugin/src/pack.rs`, with `tools()`, `ready()`, and `call()` unchanged: `fn render<'a>(&'a self, _cx: RenderContext<'a>, offered: Vec<ToolInfo>) -> PackFuture<'a, Vec<ToolInfo>>`, defaulting to `Box::pin(async move { offered })`, so `dyn Pack` stays object-safe and every existing Pack passes its tools through. Its doc says that it returns the tools the model sees, given the tools a section offered from this Pack in one set; that it may reshape, rename, or drop offered tools and add model-only tools under its own Pack id; that every catalog tool it returns is one the section offered in that set; that it receives the same inputs for either set, and a set the Pack was not offered into is not rendered; and that it runs in the effect loop with `call`'s rules: it must not block while polled, it must not panic, and it is cancellation-aware.
- `crates/runvil-plugin/src/lib.rs` re-exports `RenderContext`, and `ModelInfo`, `ModelId`, and `ThinkingMode` from `runvil-types` (`crates/runvil-internal/types/src/models.rs`), because a Plugin crate may depend only on `runvil-plugin` and `shared-*` crates (`crates/build-xtask/src/product.rs`). Its `## Invariants` list gains `render`'s rules: the effect-loop rules above, and plain-data inputs and outputs. `docs-claims.mjs` scans that section, so use Engine, Harness, Host, Pack, and Plugin only in their defined senses.
- `ToolInfo.wire_name: Option<String>` in `crates/runvil-internal/types/src/tools/info.rs`, with `#[serde(default, skip_serializing_if = "Option::is_none")]` and a builder like `survives_stop`'s. Extend `ToolInfo`'s doc: a catalog tool's wire name is its `wire_name` when set, which a Pack may set in `Pack::tools()` as well as in a render, or else its id with `/` and `.` turned into `_`, such as `web_fetch` for `web/fetch`; a catalog tool's `wire_name` stays its wire name wherever a render passes it through; and a tool that appears only in a render's answer is model-only, which Lua cannot see or call. Name `Pack::render` in plain text, because the facade does not re-export `Pack` and an intra-doc link fails `xtask api --check`.
- In `a_tool_info_logged_without_survives_stop_reads_as_not_surviving` (`crates/runvil-internal/types/src/tools/tests.rs`), remove `"wire_name":"fetch"` from the old fixture, which would now read as `Some("fetch")`, so the test keeps pinning only the missing `survives_stop`. That fixture is the one stored `ToolInfo` in the repository, which is why the field may take the name `wire_name`.
- In `crates/runvil-plugin/src/abi.rs`, the "Reserved verbs" list goes from "Verbs 3 to 7" to "Verbs 3 to 8" and gains `RV_PACK_RENDER` (8), which renders the tools a section offered from one Pack for one model and finishes later through an `RvHost` function. Following that list's rule, the verb gets no constant yet, and the test Plugin's check that verb 8 is unsupported (`crates/harness-internal/test-plugin/src/lib-tests.rs`) stays as it is.
- Regenerate `crates/runvil/public-api.txt` with `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --bless`, because `ToolInfo` is in the facade.

Tests to add:

- `Pack::render`'s default returns the offered tools unchanged, and `RenderContext::new` holds the section entry id and the `ModelInfo` it was built with.
- `ToolInfo.wire_name` reads as absent from JSON written before it existed, is skipped when absent, and round-trips exactly, with canonical key order, when its builder sets it.

While iterating, run `cargo nextest run --locked -p runvil-plugin -p runvil-types -p harness-test-plugin --all-features`.

</step-1>

<step-2>

### Step 2: Define the Static Set and the Dynamic Set [completed]

- Component: Per-section tool sets
- Tests:
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `git grep -n -E "^Seven terms have exactly one meaning each|^- \*\*Static Set\*\*: |^- \*\*Dynamic Set\*\*: " -- AGENTS.md`, which prints exactly three lines
  - `git grep -n -e "Five words" -- AGENTS.md`, which prints nothing

This commit touches only the root `AGENTS.md`, so the terms exist before the code and docs that use them.

- In the Definitions section, "Five words have exactly one meaning each" becomes "Seven terms have exactly one meaning each", and the rest of that sentence stays as it is.
- After the **Plugin** bullet, add these two bullets, word for word:
  - **Static Set**: the tools a section renders into the model's tool list at its first model call. It is frozen after that.
  - **Dynamic Set**: the tools the model finds and calls through the Engine's two discovery tools.
- Two tests in `crates/workshop/ui/test/docs-claims.mjs` read the whole root `AGENTS.md`, the Definitions section included, and fail on "activat", "TOML config", "three named buckets", or "three homes". The bullets above avoid all four.

</step-2>

<step-3>

### Step 3: Name, check, and freeze tools per section entry [completed]

- Component: Per-section tool sets
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo build --locked`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo check -p gateway --no-default-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p harness --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `cargo deny check`
  - `cargo +1.95 build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`

Terms:

- A section entry is one execution of a section; re-entering a section, a fan-out arm, a task, and a child started with `call` are each their own entry. The freeze is the entry's first model call, when the Engine builds both sets and keeps them for the rest of the entry.
- The Static Set is the tools rendered into the model's tool list at the freeze. The Dynamic Set is the tools the section offered with `tools.offer_dynamic`; step 6 lets the model reach them.
- A wire name is the name the model sees. A tool's default wire name is its id with `/` and `.` turned into `_` (`web/fetch` gives `web_fetch`), and a catalog tool's wire name is its `ToolInfo.wire_name` (step 1), or else its default.
- The run catalog is every catalog tool's `ToolInfo` by id, read once per run and shared by every section entry. It is what Lua sees. Engine crate docs and code names call a Harness Pack, one whose tools are in the run catalog, a catalog Pack, call whatever steps the run "the caller", and never mention the Host.
- The scope gate is the check in `ArrivedRound::tool_calls` (`crates/runvil-internal/engine/src/execute/scheduler/chat.rs`) that each name the model called is in the entry's Static Set. Step 6 widens the gate to the Dynamic Set, whose direct calls it answers with refusal text; until then a Dynamic Set name fails the round as it does today, and no model call reaches a Dynamic Set tool. A name outside the Static Set fails the round with `Error::OutOfScopeToolCall` (`crates/runvil-internal/engine/src/error.rs`), whose `global_exists` says whether the run catalog has a tool with that catalog wire name, and whose `in_scope` lists the Static Set's wire names.

The run keeps one Lua-facing catalog by id, and everything model-facing is built per section entry. Nothing is bound at run start:

- `ToolSet` (`crates/runvil-internal/lua/src/handles.rs`) keeps its name and replaces `offered: Vec<ToolBinding>` with the run's `ToolCatalog` handle, a clone of the `Arc` that `RunContext.tools` already holds (no deep clone and no map), and `vendors: BTreeMap<PackId, String>`, the per-Pack vendors `RunContext` holds. `ToolSet::offered`, `offered_binding`, and `wire_binding` go away; lookups use `ToolCatalog::get` and readers iterate `tools()`. In `crates/runvil-internal/engine/src/execute/context-bound.rs`, `bound_tool_set` builds the new `ToolSet`, and `offered_bindings`, `offer_refusal`, `catalog_binding`, and `catalog_bindings` go away, with `RunState.catalog_bindings` (`crates/runvil-internal/engine/src/execute/context.rs`). Every reader of a removed field fails to compile, which lists them; `tools.get`, Tool objects, and Pack objects read the run catalog from now on.
- `ToolBinding` loses `vendor` and `with_vendor`. `ToolSet::vendor_for(id)` derives the vendor from the id's Pack segment through `vendors`, with an adhoc tool reporting none, as a map miss does today. Its three readers derive it where the chain's `ToolSet` is in hand: the scope-gate event in `chat.rs`, and the `ToolResult` reports inside `prepare_dispatch` and `prepare_model_dispatch` (`crates/runvil-internal/lua/src/dispatch.rs`), whose bodies take it as a parameter.
- One lookup per call: `tool_route` (`crates/runvil-internal/engine/src/execute/scheduler/tool_call.rs`) resolves every call to its binding once, a model call by wire name in the Static Set (step 6 adds the Dynamic Set, through the discovery route) and a script call by id among the section's adhoc tools or in the run catalog, and the catalog route holds that binding, so `prepare_catalog_call` only issues the call. Today a model call is looked up twice. A script call's binding is built on demand from `catalog.get(id)` with the full id as its alias, so its `ToolCall` effect, that effect's record, and its `ToolResult` event report the id, such as `tools/echo` where today they report `tools_echo`. Update the comments in `tool_route` and `chain_record.rs` that describe the chain copy.
- The expansion in `crates/runvil-internal/lua/src/vm/state.rs` dedupes offers by id only, a static offer of an id winning over a dynamic one, and `current_dynamic_tools` becomes `current_dynamic_set`. Its wire-name drops move to merge rules 2 and 5, because a render may rename a tool out of a clash and a clash across the sets must fail rather than vanish: `current_tool_bindings` stops dropping a catalog tool an adhoc tool shadows, and `current_dynamic_set` stops skipping a tool whose wire name the static list holds. A Pack offer still expands to that Pack's catalog tools in id order, as the run-start offering held them, because `ToolCatalog` keeps the order its tools were supplied in.
- `tool_schema_new` (`crates/runvil-internal/model-client/src/detail.rs`), which every advertised and adhoc tool passes through, tightens its wire-name rule from `[A-Za-z0-9_.-]+` to `[A-Za-z][A-Za-z0-9_-]{0,63}`, the rule adhoc names already follow (`Alias`, `crates/runvil-internal/lua/src/alias.rs`). A tool whose default name starts with a non-letter or passes 64 characters, or whose `wire_name` holds a `.`, now fails the sections that offer it.
- `global_exists` (which reads `ToolSet::wire_binding` in `chat.rs` today) compares against the run catalog's wire names only, as it does today, because its error text says the name is a catalog tool the section did not offer. The `tools.offer` "did you mean" hint (`crates/runvil-internal/lua/src/tools/offer.rs`) compares against the run catalog's wire names, then the adhoc tools' names.

The merge is one function beside `prepare_effective_scope` (`crates/runvil-internal/engine/src/execute/scope.rs`), inside the `TOOL_SCOPE_VALIDATION_STARTED` and `SUCCEEDED` or `FAILED` pair that function reports today. It builds `RenderedTools` from each offered catalog Pack's tools per set, taken as `ToolInfo` lists in the shape a render answers, and the section's adhoc tools. Until step 11 lands the render, `prepare_chat` passes each Pack's offered tools straight through. This step implements these rules, in this order; rule 1's checks on a render answer arrive in step 11, and rule 4, the discovery tools, in step 6:

- From rule 1: each tool's wire name is its `ToolInfo.wire_name`, or else its default, and a catalog id keeps the catalog's `structured_output` and `describes_tools`.
- Rule 2: adhoc tools join their sets. Within a set, when an adhoc tool and a Pack tool share a wire name, the adhoc tool stays and the Pack tool is dropped, renamed and model-only tools included.
- Rule 3: a Dynamic Set tool whose id the Static Set holds is dropped.
- Rule 5: a wire name used twice anywhere in the two sets, within one set or across them, is a merge failure. That includes a static adhoc tool beside a dynamically offered Pack tool of its name, which today is skipped quietly.
- Rule 6: every tool in both sets has a wire name that passes the tightened rule and an object parameter schema. This is the schema build through `tool_schema_new`, which fails through `Error::BindSchema` as today, so a bad name or a non-object schema fails the same way in either set. The merge does not keep the schemas it builds.
- Order: each set keeps offer order by Pack. A Pack's tools sit as one block where that Pack's first tool was offered into that set, in the order they arrive, and an adhoc tool keeps its own place. Offering `kit/a`, then `web`, then `kit` gives `kit_a, kit_b, web_fetch` where today it gives `kit_a, web_fetch, kit_b`; a section that offers each Pack's tools together keeps today's order.
- A broken rule from 1 to 5 is a new Engine error beside `Error::BindSchema` in `crates/runvil-internal/engine/src/error.rs`, which is outside the facade. It names the Pack, the tool (both tools for a clash), and the rule, and maps to the same error kinds `BindSchema` maps to: Lua kind `internal` and run error kind `Binding`. Do not reuse `BindSchema`, whose message describes a schema build. Write its text for a model to read: concise, naming the tools and what is wrong. A merge failure fails the section's model call, so a catalog tool whose wire name is invalid or clashes fails just the sections that offer it, where today it disappears from the whole run after one log line.

The frozen sets are the one record of what the model was shown:

- `RenderedTools` (`crates/runvil-internal/lua/src/scope.rs`) holds `static_set` and `dynamic_set`, both `Vec<ToolBinding>` in merge order, scanned linearly with no wire-name index, because a section's sets are tens of tools and schema order is the vec order. `schemas`, `dispatch`, and `dynamic` go away, and so do `Advertised` and `Chain.advertised` (`crates/runvil-internal/engine/src/execute/scheduler/chain_record.rs`, which `scheduler/test_hooks.rs` reads). The Dynamic Set holds bindings, not ids, because after a render its names, descriptions, schemas, and flags cannot be derived from the catalog.
- The Chat effect projects its schemas from `static_set` through `tool_schema_new` when it is issued, and a later round projects the same bytes because the sets are frozen.
- The scope gate and the tool-call event read the section's frozen sets through the chain's current frame. When a round's answer arrives the chain is parked on that round, a chain has one parked chat, and `accept_chat` reads the scope before `settle`, so the frame's sets are that round's sets.
- `rendered_target` (`crates/runvil-internal/lua/src/messages.rs`) resolves a wire name in the Static Set, and `rendered_ids` (`vm/state.rs`) returns the Static Set's ids.
- New fields and variables for the two sets are named `static_set` and `dynamic_set`, and the docs and comments this step touches call them the Static Set and the Dynamic Set.

Test support moves to the catalog. `Scenario::offer(alias, tool)` (`crates/runvil-internal/engine/src/test_support/scenario.rs`) puts the tool in the run catalog with `wire_name` set to `alias`, and `Scenario::offering` and its `with_tool_set` prefill (`with_tool_set` in `crates/runvil-internal/engine/src/execute/context.rs`) build the catalog the same way. The about 80 `.offer(...)` calls in 28 engine test files keep their scripted calls and assertions, including `global_exists: true` in `model_calling_an_offered_but_unscoped_tool_is_a_hard_error` (`crates/runvil-internal/engine/src/execute/tests/tool_loop-scope-gate.rs`).

Files near the 500-line ceiling: `tool_call.rs` (382 lines), `messages.rs` (490), `scenario.rs` (450), and `error.rs` (449). Split one rather than pass 500.

Tests to add or update:

- Unit: `tool_schema_new` accepts `[A-Za-z][A-Za-z0-9_-]{0,63}` and refuses a leading digit, a `.`, and a 65-character name. A catalog tool's wire name is its `ToolInfo.wire_name` when set and its default otherwise, and `global_exists` and the `tools.offer` hint use the same name. The merge has one test per rule this step implements, table-driven where the shape allows: rule 2's covers the adhoc case in each set, and rule 5's covers a clash within one set and a clash across the sets as separate assertions. One more pins each set's order with an interleaved offer.
- Two catalog tools whose default names clash: a section that offers one of them works, and a section that offers both fails its first model call with an error naming both. Both tools stay in the run catalog.
- A catalog tool with an invalid default name fails just the sections that offer it.
- Two sections that offer the same Pack each build their own bindings, and a model call in one resolves against its own section's sets.
- The scope gate and the tool-call event read the section's frozen sets, including when an adhoc handler runs its own model rounds between the round's answer and the next round.
- The `tools.offer` hint for a guessed wire name still names the matching tool id.
- The `TOOL_SCOPE_VALIDATION` events fire around the merge as they fire around the build today.
- A catalog tool whose `ToolInfo` sets `wire_name` is advertised under that name.
- A section that interleaves two Packs' offers sees each Pack's tools together, at that Pack's first offer.
- A script call's effect, record, and `ToolResult` event report the tool's id as its alias.
- In `crates/runvil-internal/engine/src/execute/tests/rendered_tools.rs`, drop `tools/shape` from the scenario of `dynamic_offers_leave_the_rendered_schemas_byte_for_byte_unchanged`, because rule 6 now fails a Dynamic Set tool whose schema is not an object, and keep it pinning unchanged bytes. `every_later_round_sends_the_same_tool_bytes_across_a_model_change` and the first-round refusals there (`a_first_round_the_precheck_refuses_still_fixes_the_tool_list`, `a_first_round_refused_over_an_empty_list_renders_nothing`, and `a_first_round_refused_for_a_missing_model_renders_nothing`) keep passing.
- In `crates/runvil-internal/lua/src/tools/tests-render.rs`, the two tests that pin wire-name drops inside the expansion move to the merge's rule tests: `an_offered_adhoc_tool_shadows_an_offered_catalog_tool_of_its_wire_name` keeps its outcome under rule 2, and `a_catalog_tool_a_static_adhoc_tool_shadows_stays_out_of_the_dynamic_list` (a static adhoc `web_search` beside `tools.offer_dynamic('web')`) becomes a rule 5 failure, because only an id match drops a Dynamic Set tool. Update any other test there that pins the old dynamic list; the rest keep pinning call order, which is the merge's input. `an_adhoc_tool_may_take_an_offered_wire_name_and_wins_in_scope` (`crates/runvil-internal/engine/src/execute/tests/adhoc_tools.rs`) keeps passing.
- `crates/runvil-internal/engine/src/execute/tests/event_vendors.rs` keeps its vendor assertions, and its script-call result alias changes from `tools_echo` to `tools/echo`, as does the script call's record in `offering.rs`. `offering.rs` keeps pinning today's order for sections that offer each Pack's tools together. In `an_offered_pack_reaches_only_the_rounds_of_the_section_entry_that_offered_it` (`offering-packs.rs`), the expected order changes from `kit_a, web_fetch, kit_b` to `kit_a, kit_b, web_fetch`.

While iterating, run `cargo nextest run --locked -p runvil-model-client -p runvil-lua -p runvil-engine -p runvil --all-features`.

</step-3>

<step-4>

### Step 4: Document per-section tool sets [completed]

- Component: Per-section tool sets
- Tests:
  - `$env:RUNVIL_DOCS='c:\Users\Vinnie\cursor\runvil-docs'; cargo xtask site --books-only`
  - `git -C c:\Users\Vinnie\cursor\runvil-docs grep -n -i -F -e "rendered tool list" -e "rendered list" -e "dynamic list" -e "dynamic-list" -e "run's offering" -e "out of the offering" -e "out of its offering" -e "Engine chooses" -e "[A-Za-z0-9_.-]" -- src`, which prints nothing

Commit in `c:\Users\Vinnie\cursor\runvil-docs`. The guide describes step 3's behavior, so read the merge's rules and error text in runvil before writing them down.

In `src/language/12-tools.md`:

- Every passage that says a catalog tool is left out of the offering: the wire-name rules, the list of ids `tools.get` reads as `nil`, the `tools.offer` not-a-tool error, the list of what a script's `tools.call` still runs, the script-call record, and the rule for what the model is offered. A section that offers a tool whose wire name is invalid or clashes now fails its first model call with an error naming the tool, or both tools for a clash; the tool stays in the run's tool catalog; and a script still calls any catalog tool by id.
- Every passage that calls the tools a section offers from, or that `tools.get`, `tools.offer`, its hint, and `tools.call` resolve against, the run's offering: that is now the run's tool catalog, and a Pack offer still brings in its tools in tool id order.
- The wire-name rule becomes `[A-Za-z][A-Za-z0-9_-]{0,63}`, and a Pack may name a tool itself, so a catalog tool's wire name is the one its Pack gives it, or else its tool id with `/` and `.` turned into `_`. Rewrite the passages that say the Engine chooses every wire name to match; step 13 extends them.
- The order rule: a Pack's tools sit together where that Pack's first tool was offered into that set, so offering `kit/a`, then `web`, then `kit` advertises `kit_a, kit_b, web_fetch`.
- Adhoc shadowing applies within each set: the adhoc tool keeps its name and the Pack tool of that name in the same set is dropped. The Dynamic Set drops a tool only when the Static Set holds its id, so a wire name shared across the two sets fails the first model call, even a static adhoc tool beside a dynamically offered Pack tool of its name.
- A script call is recorded under the tool's id, such as `web/fetch`, never its wire name.

In `src/language/17-quick-reference.md`, the `Binding` row says a tool whose parameter schema is not a JSON object is left out of the offering: such a tool now fails the first model call of a section that offers it.

Terms: wherever `src/language/11-conversations.md`, `12-tools.md`, and `17-quick-reference.md` call the two lists the rendered tool list (or the rendered list) and the dynamic list, say Static Set and Dynamic Set, capitalized, as the root `AGENTS.md` of runvil defines them. Rename the heading "The dynamic list with `tools.offer_dynamic`" to "The Dynamic Set with `tools.offer_dynamic`" and update its links in `12-tools.md` and `17-quick-reference.md`. Leave the text that says nothing reads the Dynamic Set yet for step 7.

</step-4>

<step-5>

### Step 5: Reserve the Pack id dynamic [completed]

- Component: Dynamic tool discovery
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo build --locked`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo check -p gateway --no-default-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p harness --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `cargo deny check`
  - `cargo +1.95 build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`

The Engine's discovery tools (step 6) live under the Pack id `dynamic`, so no Pack may take it, the way none may take `adhoc`.

- One definition in `runvil-types`, beside `PackId` (`crates/runvil-internal/types/src/packs.rs`): `RESERVED_PACK_IDS: &[&str]`, holding `adhoc` and `dynamic`, plus `PackId::is_reserved()`, which compares the id's one segment against it without allocating. A `PackId` wraps a `Vec<String>` and cannot be const-constructed, so the definition stays strings and is never built into a `Vec<PackId>`.
- `PackRegistry::insert` (`crates/runvil-plugin/src/registry.rs`) refuses a reserved id through `is_reserved()` in place of its local `ADHOC` check, and `InstallError::AdhocPackId` generalizes to an error that names the reserved id it found. Keep `runvil/dynamic` out of the registry's built-ins: `HostContext::pick` (`crates/harness-internal/runner/src/host.rs`) puts every `runvil` Pack into every run's roster unless a `uses:` entry takes its id, so a built-in `dynamic` would trip the reservation on every run and show Lua an empty Pack.
- `check_no_adhoc_pack` and `PackListError::Adhoc` (`crates/runvil-internal/parser/src/contract.rs`, matched in `parse.rs`) generalize to the reserved check, so `uses:` refuses `dynamic` as it refuses `adhoc`.
- In `RunContext::prepare` (`crates/runvil-internal/engine/src/execute/config.rs`), `Requirements::reserved_adhoc: bool` (`crates/runvil-internal/engine/src/execute/requirements.rs`) becomes `reserved: Vec<PackId>`, the sorted reserved ids the run's tool catalog or Pack roster names: a per-run findings list, a different thing from the definition. `is_satisfied` needs it empty, `merge` takes the union, and `notice` names each id on its own line, in the notice's existing style, written for a model to read.
- Regenerate `crates/runvil/public-api.txt` with `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --bless`, because `Requirements` is in the facade.

Tests to add or update:

- The registry, `uses:`, and `RunContext::prepare` each refuse `dynamic` the way they refuse `adhoc`, and each refusal and the notice name the id it found. Generalize the existing adhoc tests (`crates/runvil-plugin/src/registry-tests.rs`, the parser's contract tests, and `crates/runvil-internal/engine/src/execute/requirements-tests.rs`) to cover both ids, table-driven where the shape allows.
- `merge` keeps the union of both reports' reserved ids, sorted, each once.

While iterating, run `cargo nextest run --locked -p runvil-types -p runvil-plugin -p runvil-parser -p runvil-engine --all-features`.

</step-5>

<step-6>

### Step 6: Add the GetDynamicTools and CallDynamicTool discovery tools [completed]

- Component: Dynamic tool discovery
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo build --locked`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo check -p gateway --no-default-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p harness --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `cargo deny check`
  - `cargo +1.95 build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`

Terms: the freeze is a section entry's first model call, when the Engine builds both sets and keeps them for the rest of the entry. The Static Set is the tools rendered into the model's tool list at the freeze, and the Dynamic Set is the tools the section offered with `tools.offer_dynamic`, which the model finds with `GetDynamicTools` and calls with `CallDynamicTool`. A wire name is the name the model sees. The nonce is the per-run token the Engine wraps around untrusted tool output (`prepare_dispatch`, `crates/runvil-internal/lua/src/dispatch.rs`). The scope gate is the check in `ArrivedRound::tool_calls` (`crates/runvil-internal/engine/src/execute/scheduler/chat.rs`) that each called name is in the frozen sets. Engine crate docs, comments, tests, and strings call a Harness Pack a catalog Pack, call whatever steps the run "the caller", and never mention the Host.

The tools:

- Ids `dynamic/search` and `dynamic/call` under the reserved Pack id `dynamic` (step 5), wire names `GetDynamicTools` and `CallDynamicTool`, and vendor `runvil` through `ToolSet::vendor_for`. Merge rule 4: when the frozen Dynamic Set is non-empty, both join the end of the Static Set, after rule 3 and before rules 5 and 6, so rule 5 fails any other tool, a Pack tool included, that takes either wire name.
- `GetDynamicTools` description: usage text, then the Dynamic Set's wire names, sorted, comma-separated, in parentheses. Past 100 names the list stops with "and N more; call GetDynamicTools to search".
- `GetDynamicTools` parameters, both optional: `toolName` (string) and `pattern` (string, a regex of at most 256 characters, matched against Dynamic Set wire names with the `regex` crate's default syntax, so `(?i)` makes it case-insensitive). A `namespace` key is accepted and ignored. When both are given, `toolName` wins.
- `GetDynamicTools` results: a `toolName` lookup returns `{"tool", "description", "inputSchema"}`. A pattern search, or a call with empty arguments, returns an array of `{"tool", "description"}`, one for every match in Dynamic Set order, and an empty array when nothing matches. A description longer than 200 characters, counted in characters, is cut to its first 200 and followed by "... [truncated]".
- `CallDynamicTool` parameters: `toolName` (string, required, a wire name from the list) and `arguments` (object, default `{}`). A `namespace` key is accepted and ignored.

Routing, for model calls only, is one new route kind inside `tool_route`'s single lookup (`crates/runvil-internal/engine/src/execute/scheduler/tool_call.rs`). Its parsing, search, formatting, and refusals live in a new child module, `crates/runvil-internal/engine/src/execute/scheduler/tool_call-discovery.rs`, beside `tool_call-adhoc.rs`, because `tool_call.rs` is 382 lines against the 500-line ceiling. No check outside `tool_route` and no discovery dispatch or continuation type: a second lookup would resolve the name twice, and inline answers plus the two existing preparers cover every outcome.

- The called wire name resolves in the frozen sets. A discovery tool (a Static Set binding under the Pack id `dynamic`) or a Dynamic Set name takes the discovery route; every other Static Set binding takes the adhoc or catalog route as today.
- `GetDynamicTools` answers inline from the frozen Dynamic Set through `ToolCallDispatch::Answered`, and a direct call to a Dynamic Set name is answered inline with refusal text. `CallDynamicTool` resolves its `toolName` in the Dynamic Set and hands the inner `arguments`, with the call's site unchanged, to `prepare_adhoc_call` (`tool_call-adhoc.rs`) for an adhoc target or to `prepare_catalog_call` as a model call for any other, so counting, access, trust, and result reporting under the original call id reuse those paths. Every refusal is decided before any preparer runs.
- The batch path, `dispatch_tool_calls`, stops at the discovery route the way it stops at an adhoc one, so the loop sends that call by itself.
- The scope gate accepts both sets' names. That never runs a Dynamic Set tool directly, because the discovery route answers every direct Dynamic Set call with refusal text, and it keeps the loop going when a model calls a tool it just discovered by its own name. `OutOfScopeToolCall.in_scope` keeps listing only the Static Set's wire names, so the error stays short when the Dynamic Set is large, and a name outside both sets still fails the round. The tool-call event names its tool and vendor.

Refusals, trust, and counts:

- Every discovery refusal is a plain-text tool result (`ToolCallOutcome::Plain`, `crates/runvil-internal/lua/src/protocol/answer.rs`) and the loop continues, because an error on the dispatch path raises out of `models.loop` (`crates/runvil-internal/lua/src/models_loop/machine.rs`). The refusals: an unknown `toolName`, a Static Set name, a `toolName` or `pattern` that is not a string, a bad or overlong `pattern`, a missing `toolName` on `CallDynamicTool`, a non-object `arguments`, and a direct model call to a Dynamic Set name. Each text is concise and says what was wrong and what to do next, such as naming `GetDynamicTools` for an unknown name, or saying to call a Dynamic Set tool through `CallDynamicTool` with that `toolName`.
- Each refusal reports `TOOL_CALL_FAILED` and a `ToolResult` holding its text, as a failed model call reports its failure (`prepare_model_dispatch`, `dispatch.rs`), except that the text is trusted, because it is Engine text. A `GetDynamicTools` answer reports `TOOL_CALL_SUCCEEDED`.
- A `GetDynamicTools` listing is untrusted, because descriptions come from Packs and their servers: the Engine wraps it with the nonce, and its `ToolResult` event has `trusted` false. A tool called through `CallDynamicTool` reports through its own path, under the `CallDynamicTool` call id.
- A `GetDynamicTools` call counts as `dynamic/search` whatever its outcome, and a `CallDynamicTool` refusal or a refused direct call counts as `dynamic/call`. A `CallDynamicTool` call that reaches its target counts once, under the target, in the target's existing preparer. `rendered_ids` still returns the Static Set's ids, so the section's count map starts with `dynamic/search` and `dynamic/call` once the discovery tools are rendered, and `tools.calls` lists them among its keys until step 8 replaces it.

Around the route:

- `tools.adhoc` refuses the names `GetDynamicTools` and `CallDynamicTool`, in the name check in `crates/runvil-internal/lua/src/tools.rs` beside its invalid-name error, with text that names the reserved name and says to pick another.
- A script's `tools.call("dynamic/call", ...)` fails as an unknown id, because `tools.call` resolves ids in the run catalog, which holds no `dynamic` tool.
- `regex` joins `[workspace.dependencies]` in the root `Cargo.toml` and becomes a normal dependency of `runvil-engine` (`crates/runvil-internal/engine/Cargo.toml`), with the updated `Cargo.lock` committed, because every gate runs locked. Version 1.13.1 is already in the lockfile through unrelated crates, and the Engine crates reach it today only through the dev-dependency `criterion`, so this adds an edge, not a new version. The structural checks allow it, and the `cargo +1.95` build in Tests confirms it builds at the Engine floor.

Tests to add or update:

- The discovery tools appear exactly when the Dynamic Set is non-empty, at the end of the Static Set, and the `GetDynamicTools` description lists exactly the Dynamic Set's names, sorted, capped at 100 with the "and N more" tail.
- Search works by name and by pattern, `(?i)` included, and search results hold only names and descriptions. A `toolName` lookup also returns `inputSchema`, `toolName` wins over `pattern`, and `namespace` is ignored.
- A search with no match returns an empty array, and only a description longer than 200 characters is cut, followed by "... [truncated]".
- `CallDynamicTool` on a catalog tool issues `Effect::ToolCall` for its id, and on an adhoc tool runs its handler, each reporting under the `CallDynamicTool` call id.
- Each refusal listed above returns text, reports `TOOL_CALL_FAILED` and a trusted `ToolResult`, and the loop continues.
- A direct model call to a Dynamic Set name, alone and in a batch, returns text pointing to `CallDynamicTool`, and the loop continues. A name outside both sets is still a hard error, as `model_calling_an_offered_but_unscoped_tool_is_a_hard_error` (`crates/runvil-internal/engine/src/execute/tests/tool_loop-scope-gate.rs`) pins.
- A batch with a discovery call in it works.
- A script's `tools.call("dynamic/call", ...)` fails as an unknown id, and `tools.adhoc` refuses both names.
- The `GetDynamicTools` listing is wrapped with the nonce, with `trusted` false on its `ToolResult`, and a discovery call's tool-call and tool-result events report the vendor `runvil`.
- The counts above, read through `tools.calls`.
- Merge: a rule 4 test, and rule 5's test gains a Pack tool named `GetDynamicTools` as its own assertion.
- End to end: a section that offers tools dynamically shows the model `GetDynamicTools` and `CallDynamicTool`, the model lists, searches, and calls a dynamic tool, and every discovery refusal comes back as text while the loop continues.
- Rewrite `dynamic_offers_leave_the_rendered_schemas_byte_for_byte_unchanged` (`crates/runvil-internal/engine/src/execute/tests/rendered_tools.rs`) to pin that dynamic offers add exactly the two discovery schemas, at the end of the list, and rename it to say so. `every_later_round_sends_the_same_tool_bytes_across_a_model_change` keeps passing.

While iterating, run `cargo nextest run --locked -p runvil-lua -p runvil-engine --all-features`.

</step-6>

<step-7>

### Step 7: Document the discovery tools [completed]

- Component: Dynamic tool discovery
- Tests:
  - `$env:RUNVIL_DOCS='c:\Users\Vinnie\cursor\runvil-docs'; cargo xtask site --books-only`
  - `git -C c:\Users\Vinnie\cursor\runvil-docs grep -n -i -F -e "nothing reads" -e "model cannot reach" -- src/language/12-tools.md src/language/17-quick-reference.md`, which prints nothing
  - `git -C c:\Users\Vinnie\cursor\runvil-docs grep -c -F -e "CallDynamicTool" -- src/language/12-tools.md src/language/17-quick-reference.md`, which prints a count for both files

Commit in `c:\Users\Vinnie\cursor\runvil-docs`. Read step 6's tool text, parameters, and refusal text in runvil before quoting them.

- In `src/language/12-tools.md`, replace the text that says nothing reads the Dynamic Set yet, so the model cannot reach a dynamic tool (in the section on `tools.offer_dynamic`, and the cheat-sheet comment "which nothing reads yet"), and in `src/language/17-quick-reference.md` the `tools.offer_dynamic` row's "which the model cannot reach": describe the two discovery tools in their place.
- Document both tools in `12-tools.md`:
  - When a section's Dynamic Set is non-empty, its Static Set ends with `GetDynamicTools` and `CallDynamicTool`, whose ids are `dynamic/search` and `dynamic/call` under the reserved Pack id `dynamic`, which no Pack and no `uses:` entry may take.
  - The `GetDynamicTools` description ends with the Dynamic Set's wire names, sorted, comma-separated, in parentheses, and stops after 100 with "and N more; call GetDynamicTools to search".
  - `GetDynamicTools` takes an optional `toolName` and an optional `pattern`, a regex of at most 256 characters in the `regex` crate's syntax, where `(?i)` makes it case-insensitive. `toolName` wins when both are given, and `namespace` is accepted and ignored. A lookup returns `tool`, `description`, and `inputSchema`. A search, or a call with no arguments, returns `tool` and `description` for each match in Dynamic Set order, or an empty array when nothing matches, and cuts a description longer than 200 characters to its first 200 followed by "... [truncated]".
  - `CallDynamicTool` takes `toolName` (required) and `arguments` (an object, default `{}`), and runs the named Dynamic Set tool, catalog or adhoc, under the `CallDynamicTool` call's id.
  - Mistakes come back to the model as text and the loop goes on: an unknown or Static Set `toolName`, a non-string `toolName` or `pattern`, a bad or overlong `pattern`, a missing `toolName`, a non-object `arguments`, and a direct call to a Dynamic Set tool by its wire name, whose text points to `CallDynamicTool`. A name outside both sets still fails the round.
  - The listing is untrusted tool output and is wrapped as such, while a refusal is Engine text.
  - A script's `tools.call('dynamic/call', ...)` fails as an unknown id, `tools.adhoc` refuses the names `GetDynamicTools` and `CallDynamicTool`, and a Pack tool with either wire name fails the section's first model call.
- Update the reservation messages that `src/language/12-tools.md` and `src/language/04-how-a-prompt-runs.md` quote, which still name only `adhoc`, to the messages step 5 lands for both reserved ids (the registry, `uses:`, and the requirements notice); read them in runvil before quoting.
- Leave counting to step 9, which documents `tools.stats`.

</step-7>

<step-8>

### Step 8: Replace tools.calls with tools.stats [completed]

- Component: Tool stats
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo build --locked`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo check -p gateway --no-default-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p harness --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `cargo deny check`
  - `cargo +1.95 build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`
  - `git grep -n -F -e "tools.calls" -- crates`, which prints nothing

`tools.stats` replaces `tools.calls` so Lua can judge progress cheaply, including through tools only the model sees, without knowing their names. The only prompts that read `tools.calls` are the gitignored `local/briefer.md` and `local/prompts/briefer.md`, which are already broken, so nothing keeps the old name. The Static Set and the Dynamic Set are the two tool sets a section entry freezes at its first model call; a model-only tool is one only a render's answer holds (step 12), outside the run catalog and the adhoc tools.

The table is read-only, with exactly three keys:

- `total` counts every tool call the section made, at dispatch, so a call that later fails or is cancelled still counts. A call refused before its count is taken (an unbound name, a failed scope install, or an out-of-scope model call) stays out of every counter.
- `packs[pack]` counts calls per Pack id, model-only tools included, and `tools[id]` counts calls per tool Lua can see: run catalog ids and adhoc ids.
- A `CallDynamicTool` call counts under the tool it calls. A `GetDynamicTools` call and every discovery refusal, a refused direct call included, count under `packs.dynamic`. All of them count toward `total`.
- `tools` starts with a zero entry for every id Lua can see in the section's two sets, its run catalog ids and its adhoc ids, and `packs` starts with those tools' Packs, plus `dynamic` when the discovery tools are rendered. On a call, `packs` gains an entry for a Pack it did not start with, and `tools` gains one only for an id Lua can see.
- `tools.stats` accepts exactly `total`, `packs`, and `tools`, and raises on any other key. `packs` and `tools` each raise on a key they do not hold, naming that key and listing the keys they hold, so the list from `tools` never names a discovery or model-only id. A write raises, as a write to `tools.calls` raises `tools.calls is read-only` today. `pairs` walks `packs` and `tools` in key order.
- `tools.stats` appears when `tools.calls` does today, through the one-time install that also fills `sys.model` (`install_section_scope`, `crates/runvil-internal/engine/src/execute/section_context.rs`).

How it is built:

- Computed, not stored. `ToolCallCounts` (`crates/runvil-internal/lua/src/scope.rs`) stays the one id-keyed map and gains a single-lock snapshot method. No counter is stored per Pack or in total, and the existing increment sites stay as they are; step 6's discovery bumps are the only other ones.
- The starting ids: `section_call_counts` (`crates/runvil-internal/engine/src/execute/scheduler/tool_call.rs`) seeds the ids Lua can see in both sets, plus `dynamic/search` and `dynamic/call` when the discovery tools are rendered, from the frozen sets once the section has frozen and from its live offers of both kinds before. `rendered_ids` (`crates/runvil-internal/lua/src/vm/state.rs`) widens to both sets. Today the map starts from the Static Set only, which leaves a dynamically offered tool without a starting entry.
- The Lua install replaces `install_lua_tool_calls` (`crates/runvil-internal/lua/src/tools.rs`) and answers at read time: `total` sums the snapshot, a `packs` proxy sums each Pack's keys, and a `tools` proxy keeps only run catalog ids and adhoc ids. Each proxy takes its shape from `tools.packs` (`crates/runvil-internal/lua/src/packs.rs`: `__index`, `__newindex`, `__pairs`, and `__metatable`) and its unknown-key error from `tools.calls` (today `tools.calls: "<key>" has no seeded count; seeded names: [...]` with its hint), because `tools.packs` reads an unknown key as nil. Name each error after the table that raises it. `tools.rs` is 377 lines, so put the install in a child module under `crates/runvil-internal/lua/src/tools/` if it would pass 500.
- Fix the stale doc comments on `SectionContext.counts`, `script_call_counts`, and `install_section_scope` in `section_context.rs`. They say only a script-initiated `tools.call` installs the counts and that the dispatch task increments them off the driver thread, but model calls install the counts too, and the counts are bumped on the driver thread. The doc on `script_call_counts` also says a first prose block performs the install, and none does. Update every other comment that names `tools.calls`, such as the `ToolCallCounts` doc and `install_tool_call_counts` in `vm/state.rs`.

Tests to add or update:

- Each counter as computed from the one map, the read-only refusal, and each unknown-key error, with a discovery id and a model-only id absent from both the `tools` view and the unknown-key error's list. No model-only tool exists until step 12, so seed the map with an id outside the run catalog and the adhoc tools to stand for one.
- `tools.stats.tools` leaves out `dynamic/search` and `dynamic/call`, `packs.dynamic` counts `GetDynamicTools` calls and discovery refusals, and a `CallDynamicTool` call counts under its target.
- `pairs` over `packs` and `tools` walks keys in order, and a dynamically offered tool has a zero starting entry.
- Every test that reads `tools.calls` moves to `tools.stats`: `crates/runvil-internal/lua/src/tools/tests.rs`, `crates/runvil-internal/lua/src/tools/tests-adhoc.rs`, and in `crates/runvil-internal/engine/src/execute/tests/`, `debug_and_counts.rs`, `tool_call_arm-adhoc-handlers.rs`, `scheduler/failures-script-tools.rs`, and step 6's discovery count tests.

While iterating, run `cargo nextest run --locked -p runvil-lua -p runvil-engine --all-features`.

</step-8>

<step-9>

### Step 9: Document tools.stats [completed]

- Component: Tool stats
- Tests:
  - `$env:RUNVIL_DOCS='c:\Users\Vinnie\cursor\runvil-docs'; cargo xtask site --books-only`
  - `git -C c:\Users\Vinnie\cursor\runvil-docs grep -n -F -e "tools.calls" -e "toolscalls" -- src plans`, which prints nothing
  - `git -C c:\Users\Vinnie\cursor\runvil-docs grep -n -F -e "## Calling tools from Lua and counting calls" -- src/language/12-tools.md`, which prints exactly one line

Commit in `c:\Users\Vinnie\cursor\runvil-docs`. Read step 8's error text in runvil before quoting it.

- Every mention of `tools.calls` moves to `tools.stats` in `src/language/12-tools.md` (its counting headings "Counting calls with `tools.calls`" and "When `tools.calls` appears", its table of `tools` members, the sentence that says `calls` joins that table, the list of errors `pcall` catches, the cheat sheet, and every example that reads a count), `src/language/17-quick-reference.md`, `src/language/03-blocks-and-prose.md`, and `plans/build-language-guide.md`. Update the in-chapter links to each renamed heading. Keep the heading "Calling tools from Lua and counting calls" in `12-tools.md` unchanged, so its links from chapters 01, 03, 04, 05, 07, 08, 09, 13, 14, 15, and 17 stand.
- `src/language/07-substitution.md` and `src/language/09-the-store.md` hold no literal `tools.calls`. Name `tools.stats` where they describe the section's call counts, such as 09's refused store fork that still counts toward the tool-call counts, and keep 07's `sys.model` passage, which ties `sys.model` to the section's first tool call and stays true.
- Document the table: `tools.stats` is read-only and holds exactly `total`, `packs`, and `tools`. `total` counts every tool call the section made, at dispatch, so a call that later fails or is cancelled still counts, while a call refused before its count is taken (an unbound name, a failed scope install, or an out-of-scope model call) stays out of every counter. `packs[pack]` counts calls per Pack id, and `tools[id]` counts calls per catalog or adhoc tool by its id. A `CallDynamicTool` call counts under the tool it calls, and a `GetDynamicTools` call and every discovery refusal count under `packs.dynamic`; all of them count toward `total`. `tools` starts with a zero entry for each catalog and adhoc tool in the section's two sets, and `packs` with their Packs, plus `dynamic` when the discovery tools are present. Any other key of `tools.stats`, and a key `packs` or `tools` does not hold, raises an error that names the key and lists the keys held. A write raises too, and `pairs` walks `packs` and `tools` in key order. `tools.stats` appears at the section's first tool call, as `tools.calls` did.

</step-9>

<step-10>

### Step 10: Name the section entry and the model on tool call origins [completed]

- Component: Late rendering
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo build --locked`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo check -p gateway --no-default-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p harness --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `cargo deny check`
  - `cargo +1.95 build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`

A section entry is one execution of a section: re-entering a section, a fan-out arm, a task, and a child started with `call` are each their own entry. `ToolCallOrigin.section` and `ToolCallOrigin.execution` are the section's and the run's names, so two entries of one section share both. The entry id tells them apart, and step 11's render receives the same id, so a Pack can tie a call to the render that shaped it.

- `ToolCallOrigin` (`crates/runvil-internal/types/src/tools/origin.rs`) gains `entry: Option<String>` and `model: Option<String>`, each with `#[serde(default, skip_serializing_if = "Option::is_none")]` and a `with_` builder. `ToolCaller` stays as it is, because it derives `Copy` and `Hash` and 15 files name its variants.
- `SectionContext` (`crates/runvil-internal/engine/src/execute/section_context.rs`) gains an accessor for the entry id, the frame's `sys.id`, which `enter_section` (`crates/runvil-internal/engine/src/execute/scheduler/walk.rs`) assigns; `SectionContext` stores `sys` with no getter today. Step 11's render effect reuses this accessor.
- `MessageList::commit` (`crates/runvil-internal/lua/src/messages.rs`), whose one production caller is `prepare_chat` (`crates/runvil-internal/engine/src/execute/scheduler/chat.rs`), records the round binding's `ModelId::name()` beside the last send it already keeps. `messages.rs` is 490 lines against the 500-line ceiling, so keep this record a few lines or split the file.
- `prepare_catalog_call` (`crates/runvil-internal/engine/src/execute/scheduler/tool_call.rs`) reads both fields from the calling frame. Every call's origin names its section entry; a model call's origin also names the requesting round's model, and a script call's origin leaves the model empty.
- Do not read the model from `ChatAnchor`: `settle` (`crates/runvil-internal/engine/src/execute/scheduler/chain_record.rs`) takes the in-flight model inside `accept_chat`, before the loop issues the calls, and keeps it on `measured` only when the round reported usage. Do not thread it through `Continuation::Chat`, `ChatResult`, the loop's yields, the request parser, and `CallSite` either. The message list is safe: a section runs one loop or compaction at a time (`LOOP_RUNNING` and `COMPACT_RUNNING`, `crates/runvil-internal/lua/src/models_loop.rs`), so no other round commits to the list between a round's answer and the issue of its calls, even when an adhoc handler runs `models.infer`. The name is the binding's, which matches the `ModelInfo` that `render` receives; the provider-reported model can differ.
- Update the `alias` doc comments on `Effect::ToolCall` and `EffectRecord::ToolCall` (`crates/runvil-internal/engine/src/execute/run/effect.rs`) and on `ToolPerformer::call` (`crates/harness-internal/runner/src/performers.rs`), which still say a script call reports the tool's id only when the run could not offer the tool: since step 3 a model call reports its wire name and every script call reports its id.
- Regenerate `crates/runvil/public-api.txt` with `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --bless`, because `ToolCallOrigin` is in the facade.

Tests to add:

- A model call's origin names its round's model, and a script call's origin leaves the model empty.
- A section that switches models between rounds reports each round's model.
- An adhoc handler that runs `models.infer` or `models.use` leaves the model of the batch's later calls unchanged.
- Every call's origin names its section entry, and re-entering a section and two fan-out arms of one section give different entry ids.
- `ToolCallOrigin.entry` and `ToolCallOrigin.model` read as absent from JSON written before they existed, and are skipped when absent.
- Compare `origin.model` with `info.id().name()`, because `ModelId::name()` is the name part of an id that also holds a server namespace.

While iterating, run `cargo nextest run --locked -p runvil-types -p runvil-lua -p runvil-engine --all-features`.

</step-10>

<step-11>

### Step 11: Render offered Packs through a render effect at the freeze [completed]

- Component: Late rendering
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo build --locked`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo check -p gateway --no-default-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p harness --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `cargo deny check`
  - `cargo +1.95 build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`

The render is a new effect the Engine issues at a section entry's first model call, the freeze, so the Engine stays pure and the run log records what each Pack rendered. A Harness Pack, which Engine docs and code names call a catalog Pack, is one whose tools are in the run catalog; the `adhoc` and `dynamic` Packs hold Engine-defined tools, and nothing renders them. Until step 12, every driver answers the render by passing every tool through, so the model sees what it saw before this step.

Make room first. `crates/runvil-internal/engine/src/execute/run/effect.rs` is 498 lines against the 500-line ceiling, so its record types (`EffectRecord`, `AnswerRecord`, `FailureRecord`, `ChatAnswerRecord`, and `ToolAnswerRecord`, with their impls) move into a private child module that `effect.rs` re-exports, so every existing path holds, and `PackToolSets` is defined there.

Types:

- `PackToolSets<T> { pack: PackId, static_set: Vec<T>, dynamic_set: Vec<T> }` holds one Pack's tools for each set of a section entry: the effect and its answer hold `ToolInfo`, and the effect record holds `ToolId`. It joins each re-export list the other effect types pass through, from `crates/runvil-internal/engine/src/execute/run.rs` to the engine root and the facade's `effect` module (`crates/runvil/src/lib.rs`).
- `Effect::Render { entry: String, binding: ModelBinding, offered: Vec<PackToolSets<ToolInfo>> }` carries the resolved binding, as `Effect::Chat` does, so its record derives the alias and the performer reads the binding's `ModelInfo`. `EffectAnswer::Render(Vec<PackToolSets<ToolInfo>>)` holds each Pack's rendered tools per set, in the order asked. `EffectRecord::Render { alias: String, entry: String, offered: Vec<PackToolSets<ToolId>> }` takes `alias` from the binding as `EffectRecord::Chat` takes it. `AnswerRecord::Render(Vec<PackToolSets<ToolInfo>>)` holds the full rendered tools, because `EffectRecord::Chat` keeps only tool names. Adhoc and discovery tools join later, inside the Engine, so the answer does not hold them. Record JSON round-trips exactly, with canonical key order, and JSON written before these variants existed reads unchanged.
- `Continuation::Render(ModelBinding)` in `crates/runvil-internal/engine/src/execute/scheduler/pending.rs` holds only the resolved binding, because a handle round's binding exists nowhere in VM state, and re-resolving could pick a default another chain set meanwhile. The list and the offers are not stored.
- `ModelBinding` (`crates/runvil-internal/model-client/src/model/options.rs`) gains an optional private catalog `ModelInfo` and `render_info()`, which returns the carried info or builds one from the binding's id, description, context, and provider with `ThinkingMode::Never`, the claim that asks least of the model. `bound_model_set` (`crates/runvil-internal/engine/src/execute/context-bound.rs`) sets the field from the `ModelInfo` it already reads, through a new public builder beside `with_provider`, `with_catalog_info(ModelInfo)`. `ModelBinding` lives in `runvil-model-client`, so a public method is the only way the engine can set the field, and the facade gains two `ModelBinding` lines, the builder and `render_info()`, where the Technical Design counts one (see the decision record). `with_invocation` keeps the field, because it takes `self` by value. `ModelBinding::new` leaves it empty, so the raw gateway binding that `raw_gateway_binding` (`crates/runvil-internal/lua/src/models.rs`) builds for `models.get` and a test rig's hand-built binding both take the fallback inside `render_info()`, with no special case at the render site. `RunState` gains no `ModelBindings` store, because `RunState::new` folds that journal into `ModelSet`.

The two-phase freeze:

- When the expansion holds at least one catalog Pack tool, `Scheduler::prepare_chat` (`crates/runvil-internal/engine/src/execute/scheduler/chat.rs`) issues `Effect::Render` after the emptiness check, the binding, and the expansion, and before the projection, the precheck, `attach_rendered_tools`, `anchor.sending`, `number_round`, and `list.commit`, and parks the chain on `Continuation::Render(binding)`. Its `entry` comes from step 10's `SectionContext` accessor. Its `offered` lists each such Pack once, in the order its first tool was offered, with the tools the section offered from it into each set; a set the Pack was not offered into stays empty and is not rendered. When the expansion holds no catalog Pack tool, only adhoc tools or nothing, it merges at once, as in step 3.
- On the answer, `apply_answer` (`crates/runvil-internal/engine/src/execute/scheduler/apply.rs`) recovers the list through `SectionVm::messages` (`crates/runvil-internal/lua/src/vm.rs`, made `pub`), which is the list the request named because production builds one `MessageList` per section VM, and the offers through `vm.tool_runtime()` and `vm.adhoc_tools()`, which cannot change because a parked chain's VM runs no Lua. It checks the answer against the offered ids recomputed that way and runs the merge. On success it attaches `RenderedTools` with `MessageList::attach_rendered_tools` and calls `dispatch_chat` with the kept binding, which issues the Chat effect or answers the chain inline on an overflow or an error, as it does today. On a merge failure it answers the parked chat with that error and attaches nothing. Either way the arm returns early, the way the Timer arms do. So the second dispatch projects, prechecks, numbers, and commits exactly once, and a first round the precheck refuses still fixes the list.
- `dropped_answer` maps `Continuation::Render` to the interrupted chat answer, so when the caller drops the render, through a cancel, a stop, or a panicking `render`, the model call ends as a cancelled chat and Lua sees the cancellation error. A render sits outside the in-flight tool cap.
- Merge rule 1 now checks the rendered answer, inside step 3's merge: the answer matches what was asked, the same Packs and sets, in offer order; for each Pack and set, every rendered id has that Pack's id as its first segment, an id repeated within the set keeps its first copy, and a rendered id in the run catalog is one the section offered in that set. A catalog id keeps the catalog's `structured_output` and `describes_tools` (`survives_stop` joins `ToolBinding` in step 12), so a render changes only its wire name, description, and schema, while a model-only tool's flags are whatever the render returns. Each tool's wire name is the render's `wire_name`, or else its default, so a catalog tool passed through keeps its catalog wire name. A Pack's rendered tools sit as one block, in the order its render returns them, where that Pack's first tool was offered into that set. A broken rule 1 fails through step 3's merge error, naming the Pack, the tool, and the rule.

Drivers and hand-stepped tests:

- The exhaustive effect drivers fail to compile until they handle `Effect::Render`, and each answers it with the offered tools unchanged: the engine rigs `crates/runvil-internal/engine/src/test_support.rs` and `crates/runvil-internal/engine/src/test_support/scenario-driving.rs`, the facade's `crates/runvil/tests/suite/support/scenario.rs` and `crates/runvil/examples/greeter.rs`, and the Harness effect loop `crates/harness-internal/runner/src/effect_loop.rs`, whose arm stays a few lines until step 12 replaces it. `effect_loop.rs` is 481 lines, so split it the way `effect_loop-flight.rs` is split if the arm does not fit.
- Drivers with catch-all arms still compile but would answer a render `Dropped` or panic, and tests that step a run by hand expect a Chat effect first. Since step 3 every test-support tool is in the catalog, so each such test reaches the render. Update every one that runs a section with catalog offers to expect the render effect first: `crates/harness-gateway-client/src/wire/request-tests.rs`, and in `crates/runvil-internal/engine/src/execute/tests/`, `rounds.rs`, `chat_record_rebuild.rs`, `parallel_tool_calls.rs`, `parallel_tool_calls-withdrawn.rs`, and `models_loop_contract-rounds.rs`. A render effect takes a provenance sequence number, so tests that pin later effect ids in runs with catalog offers may shift.
- Regenerate `crates/runvil/public-api.txt` with `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --bless`.

Tests to add:

- A section with catalog offers issues one render effect before its first Chat effect, and its later rounds reuse the frozen sets.
- A section whose offers expand to no catalog Pack tool, only adhoc tools or none, goes straight to its Chat effect.
- A Pack offered into one set is rendered for that set only.
- The render effect names the same entry id as the section's tool calls.
- A section whose first round runs through a model handle (`models.get(...):loop()`) still renders and then chats on that handle's binding, with no re-resolution at the answer.
- A raw gateway model id renders with a `ModelInfo` built from its binding.
- Feeding the same render answer to a fresh run yields the same Chat tools. A replay reader is still to be built (`crates/runvil-internal/types/src/replay.rs`), so the Engine rig runs this check.
- A dropped render answers the chat as interrupted.
- `ModelBinding::render_info()` returns the info `bound_model_set` set, keeps it through `with_invocation`, and builds the `ThinkingMode::Never` fallback for a binding built with `new`.
- Merge: one test per rule 1 check, table-driven where the shape allows, and one for each set's order with an interleaved offer and a render that adds a model-only tool.
- The first-round refusals in `crates/runvil-internal/engine/src/execute/tests/rendered_tools.rs` (`a_first_round_the_precheck_refuses_still_fixes_the_tool_list`, `a_first_round_refused_over_an_empty_list_renders_nothing`, and `a_first_round_refused_for_a_missing_model_renders_nothing`) keep passing, which pins where the render sits in `prepare_chat`, and so does `every_later_round_sends_the_same_tool_bytes_across_a_model_change`.

While iterating, run `cargo nextest run --locked -p runvil-model-client -p runvil-lua -p runvil-engine -p runvil -p harness-runner -p harness-gateway-client --all-features`.

</step-11>

<step-12>

### Step 12: Render Packs in the Harness and route calls by Pack [completed]

- Component: Late rendering
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo build --locked`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo check -p gateway --no-default-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p harness --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `cargo deny check`
  - `cargo +1.95 build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`

The Harness now answers the render effect by calling `Pack::render`, and each tool call holds the one flag the Harness reads, `survives_stop`, from the binding of the section that made it. A Harness Pack is a Pack the Harness serves, whose tools are in the run catalog. A model-only tool is one that appears only in a render's answer: Lua cannot see or call it, and `tools.stats` counts it only under its Pack.

- `ToolPerformer` (`crates/harness-internal/runner/src/performers.rs`) gains `fn render(&self, entry: String, model: ModelInfo, offered: Vec<PackToolSets<ToolInfo>>) -> BoxFuture<Vec<PackToolSets<ToolInfo>>>`, whose default passes every tool through, and loses `survives_stop`; `call()` and `max_in_flight()` stay. Its futures are `'static`, so an override clones each `Arc<dyn Pack>` and owns its inputs.
- `HostRunContext` (`crates/harness-internal/runner/src/host-run.rs`) overrides `render`, calling each usable Pack's `render` once per non-empty set with a `RenderContext` that holds the section entry id and the round's `ModelInfo`, and answering every Pack asked, in the order asked.
- The effect loop (`crates/harness-internal/runner/src/effect_loop.rs`) performs `Effect::Render` inside the effect's own future, in place of step 11's pass-through arm, passing `binding.render_info()` to the performer, so cancellation and a panic behave as for a tool call: a panicking `render` answers `Dropped`. `effect_loop.rs` is 481 lines, so keep the arm short or split the file the way `effect_loop-flight.rs` is split.
- `ToolBinding` (`crates/runvil-internal/lua/src/handles.rs`) gains `survives_stop`, and `Effect::ToolCall` gains `survives_stop`, taken from the binding the call resolved to, which the effect loop reads when the call is issued. The merge sets the binding's flag per section: a catalog id keeps the catalog's `survives_stop`, and a model-only tool's is whatever that section's render returns. `EffectRecord::ToolCall` leaves the flag out, as `EffectRecord::Chat` leaves out the options it derives from the binding, because the run catalog and the render's `AnswerRecord` already give it.
- The eight test doubles that implement `ToolPerformer` in `crates/harness-internal/runner/tests/it/` drop `survives_stop` and need no `render`: `Unused`, `TextTool`, `PendingTool`, `PanickingTool`, `ClosingTool`, and `GatedTool` in `support.rs`, `DelayedTool` in `performers.rs`, and `ParkingTool` in `effect_loop-cap.rs`. `survives_stop_answers_from_the_runs_snapshot_of_tool_info` in `crates/harness-internal/runner/src/host-tests.rs` tests the removed method; the per-section flag test below replaces it. `host-tests.rs` is 494 lines, so new `HostRunContext` tests go in a new split wired beside `host-tests-pick.rs`, and the test Pack goes in a new topic module beside `crates/harness-internal/runner/tests/it/main.rs`, because `support.rs` is 489 lines.
- `HostRunContext` routes every Harness call by its id's Pack segment, which covers model-only ids. Reword the registry's comment on built-in Packs (`crates/runvil-plugin/src/registry.rs`), which says a built-in's `call` is unreachable because the snapshot routes a call only to a tool in a Pack's own list, to say the Engine never issues a call under a built-in's id; that stays true because a built-in lists no tools and so is never rendered. Reword the `host-run.rs` module doc, which says the tools the run offers and the tools its calls may reach are the same list, because a model-only tool is reachable without being in the catalog.
- Regenerate `crates/runvil/public-api.txt` with `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --bless`, because `Effect::ToolCall` is in the facade.

Tests, with a test Pack whose `render` returns one renamed tool and one model-only tool:

- The Harness calls `render` once per Pack and non-empty set, with the offered tools and a `RenderContext` holding the section entry id and the round's `ModelInfo`.
- A panicking `render` answers `Dropped`, and the run log holds the render effect and its answer.
- The model calls both tools end to end, and the Pack sees the round's model and the section entry on the origin.
- `tools.call` refuses the model-only id.
- A model-only call bumps `tools.stats.packs` and stays out of `tools.stats.tools`, including its key list.
- Tool-call and tool-result events for a catalog, an adhoc, and a model-only call each report the vendor derived from the tool id's Pack (none for an adhoc tool), and a discovery call reports `runvil`.
- A model-only tool rendered into the Dynamic Set is callable through `CallDynamicTool`.
- Two sections on different models that offer the same Pack get different renders, each with its own names and flags: when they render the same model-only id with different `survives_stop`, a stop leaves running the call whose flag is true and drops the one whose flag is false.

While iterating, run `cargo nextest run --locked -p harness -p harness-runner -p harness-plugin-loader -p harness-test-plugin -p harness-gateway-client -p runvil-engine --all-features`.

</step-12>

<step-13>

### Step 13: Document late Pack rendering [completed]

- Component: Late rendering
- Tests:
  - `$env:RUNVIL_DOCS='c:\Users\Vinnie\cursor\runvil-docs'; cargo xtask site --books-only`

Commit in `c:\Users\Vinnie\cursor\runvil-docs`.

Most of this step landed early, under the docs rule, in runvil-docs `c583376` ("Document what a Pack's render can change"), written before step 12's commit message. Check each bullet below against the guide and fix only what is still missing or wrong; when nothing is, change nothing, and the step is marked complete on its Tests line with no commit.

- In `src/language/12-tools.md`, in the passages on who names a tool for the model, as step 4 left them (the wire-name section and the advice that prompt prose names each tool by its tool id): say that a Pack may rename the tools a section offered from it for that section's model and add tools only the model sees, so the names and tools a model sees can differ by section, which is why prompt prose keeps naming tools by id.
- In the counting section, say that Lua cannot get or call a tool only the model sees, and that `tools.stats.packs` counts its calls while `tools.stats.tools` leaves it out.

</step-13>

<step-14>

### Step 14: Spell the Debug trait as fmt::Debug [completed]

- Component: Debug trait spelling
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo build --locked`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo check -p gateway --no-default-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p harness --no-deps`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `cargo deny check`
  - `cargo +1.95 build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`
  - `git grep -n -F -e "std::fmt::Debug" -- "*.rs"`, which prints nothing
  - `git grep -n -E -e "std::fmt::\{[^}]*Debug[ ,}]" -- "*.rs"`, which prints nothing

The code base names the `Debug` trait one way, `fmt::Debug`, with `fmt` in scope through `use std::fmt;`. Only the `Debug` trait changes: every other `std::fmt::` path, such as `Display`, `Formatter`, `Result`, `Write`, and the `DebugStruct` family of builders, stays exactly as written, so a rewritten impl reads `impl fmt::Debug for X` over a `fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result` signature. No clippy lint can enforce the rule, because both spellings resolve to the same trait, so a structural check in `build-xtask` does. The rewrite and the check land in one commit, because the check's live test fails until the rewrite is done.

The rewrite, in place:

- Before the first edit, rerun `git grep -n -F -e "std::fmt::Debug" -- "*.rs"`, because steps 5 to 13 may add or move lines. Before those steps it listed 38 lines in 31 files: 26 impl headers such as `impl std::fmt::Debug for Run`, the supertrait bound on `RevealLauncher` (`crates/gateway/app/src/admin/walled/reveal.rs`), the delegating call `std::fmt::Debug::fmt(&self.0, f)` in `crates/gateway/app/src/api_error.rs` and in `crates/gateway/config/src/api_error.rs`, the `&dyn std::fmt::Debug` parameter of two `tracing::field::Visit::record_debug` impls, five bounds on test helpers' type parameters, the `impl std::fmt::Debug + Send + Sync + 'static + use<>` return type of `wired_push` (`crates/workshop/server/src/agents/status-tests.rs`), and one string fixture. Rewrite each to `fmt::Debug` and leave the rest of the line as it is.
- None of those files has `fmt` in scope, and none has a `use std::{...}` group to extend, so each takes its own `use std::fmt;` line beside its other `std` imports, or as its own group above the others when it has none, and `cargo fmt` orders it. Test files that glob-import their parent with `use super::*;` take their own import too. Where the cfg differs, the import follows it:
  - `crates/gateway/app/src/commands.rs`: its one line is the `#[cfg(test)]` impl for `ExecutorOverride`, and the file's `std::fmt::Formatter` and `std::fmt::Result` paths stay as written, so the import is `#[cfg(test)] use std::fmt;`. An unconditional import is unused in a normal build, which `CARGO_BUILD_WARNINGS=deny` turns into an error.
  - `crates/gateway/stt/whisper-ffi/src/lib.rs`: its line sits in the inline `#[cfg(test)] mod tests`, so the import goes in that module beside its `std` imports, not at the top of the file.
- `crates/gateway/stt/api/src/test_fixtures/warnings.rs` keeps `use std::fmt::Write as _;` as it is and gains `use std::fmt;` on its own line, because folding the two into one group would rewrite the `Write` import.
- The string fixture is `("pub use std::fmt::Debug;", 1)` in `a_re_export_rooted_outside_the_facade_dependencies_is_rejected` (`crates/build-xtask/src/facade_shape-tests.rs`), which pins that a re-export rooted at `std` is refused. It becomes `("pub use std::sync::Arc;", 1)`, which keeps that case and needs no import; `pub use fmt::Debug;` would only repeat the `pub use effect::Effect;` case beside it.
- Three group imports bring the trait in as a bare `Debug`, a second spelling, and change too: `use std::fmt::{self, Debug};` in `crates/gateway/stt/engine/src/decoder.rs` becomes `use std::fmt;`, and `use std::fmt::{Debug, Display};` in `crates/runvil-internal/engine/src/test_support/seeded.rs` and `crates/runvil-internal/types/src/test_support.rs` becomes `use std::fmt::{self, Display};`. In those three files every bare `Debug` that names the trait (a bound, an impl header, a `dyn` type) becomes `fmt::Debug`, `Display` stays as it is, and `#[derive(Debug)]`, which names the derive macro, stays as written.
- No line sits in a doc comment, an intra-doc link, or a `macro_rules!` body, but the rule and the check cover them. A doc comment writes `fmt::Debug`, and an intra-doc link to it resolves in the documented item's scope, so it needs `fmt` in scope as code does; the `cargo doc` gates fail a link that does not resolve. A `macro_rules!` body is no exception: a body that names the trait writes `fmt::Debug` like any other code, so its call sites need `fmt` in scope.
- `crates/runvil/public-api.txt` does not change, because the listing names each trait impl by the resolved trait's name (`trait_name`, `crates/build-xtask/src/api/listing/compact.rs`), not by how the source spells it.

The structural check:

- A new module, `crates/build-xtask/src/debug_spelling.rs`, declared as `mod debug_spelling;` in `crates/build-xtask/src/main.rs` between `mod api;` and `mod doc_hidden;`, with its tests in the sibling `crates/build-xtask/src/debug_spelling-tests.rs`, wired as `#[cfg(test)] #[path = "debug_spelling-tests.rs"] mod tests;` the way `doc_hidden.rs` wires its tests. Its `//!` doc states the rule and why the check reads text. `pub(crate) fn debug_spelling_violations(root: &Path) -> Vec<String>` joins `all_violations` in `crates/build-xtask/src/tidy.rs` after `no_doctests_violations`, so `cargo test -p build-xtask`, `cargo xtask tidy`, and the workspace nextest run all enforce it, and `tidy.rs`'s module doc names it among the checks it runs. Both new files stay far under the 500-line ceiling, and `tidy.rs`, at 390 lines, gains about three.
- What it scans: every `.rs` file in every workspace crate, the crates whose manifest the shared walk could not read included, sorted, through `crate::product::workspace_crates` and `crate::tidy::rust_files`, which skips `target`, as `no_doctests_violations` does. That is every tracked `.rs` file: when this step was written, all 1589 sat in a crate directory under `crates/`, and the walk found no untracked one. An unreadable file is reported, not skipped, and a scan that finds no `.rs` file fails with one violation saying it scanned nothing, as `unsafe_allowlist_violations` does, because a file that was never scanned cannot be shown clean.
- What it refuses: each line that holds `std::fmt::Debug` as a whole path, found with `str::match_indices` and counted when neither the character before `std` nor the character after `Debug` is an identifier character (`[A-Za-z0-9_]`). So `::std::fmt::Debug` is refused, while `std::fmt::DebugStruct` and the other `Debug`-prefixed builders and every other `std::fmt::` path pass. It also refuses a `use std::fmt::{...}` group, on one line or across several, that lists `Debug` as an item, bare or renamed with `as`, reported at the line holding that item, because such a group brings in a bare `Debug`, a second spelling; every other item of a group, `DebugStruct` included, passes. A line that holds the path twice is one violation. Unlike `unsafe_allowlist`, which reads tokens so that a string never matches, this check reads source text, because the rule is about the spelling wherever it appears: code, comments, doc comments, string literals, and macro bodies alike, the same text the `git grep` above reads. `build-xtask` has no `regex` dependency and needs none.
- Its own source never spells the path: the needle is `const DEBUG_PATH: &str = concat!("std::fmt", "::Debug");`, with a comment saying the split keeps this crate's source from spelling the path it refuses, and the module doc and the failure text describe the path without spelling it. The tests build every fixture that holds the long spelling from `DEBUG_PATH` with `format!`, including the builder case, written `{DEBUG_PATH}Struct`, which the `git grep` above would also match as plain text. Fixtures that list `Debug` in a `std::fmt::{...}` group are built with `format!` too, so the second `git grep` above stays empty.
- Failure text, one violation per line, written for a model in the form `no_doctests` and `doc_hidden` use: `<path>:<line>: <RULE>: required <REQUIRED>, found`, then the trimmed source line in backticks. `<path>` is the workspace-relative path with `/` separators from `tidy::slash_path`, which becomes `pub(crate)`, so the text reads the same on every platform. `RULE` is `Debug trait spelled with its std path`. `REQUIRED` says to write `fmt::Debug` with `use std::fmt;` in scope, with each code name in backticks as the other checks write them. A group-import violation uses the same `RULE` and `REQUIRED`. An unreadable file reads `<path>: unreadable source: <error>`.

Tests to add or update:

- In `debug_spelling-tests.rs`, with a `write_crate` helper like the one in `unsafe_allowlist-tests.rs` and `workspace_root` from `crate::product::test_support`:
  - The workspace passes: `debug_spelling_violations(&workspace_root())` is empty, and the failure message lists each violation on its own line.
  - A fixture crate whose `lib.rs` holds the long spelling, built from `DEBUG_PATH`, as an impl header, a bound, a `dyn` parameter, a delegating `::fmt(` call, a `::`-rooted path, a doc comment, a string literal, and one line that holds it twice is reported once per line, at that line, and each violation names the file as `crates/<dir>/src/lib.rs:<line>` and the fix `fmt::Debug`.
  - A fixture crate whose `lib.rs` holds only `fmt::Debug` with `use std::fmt;`, beside `std::fmt::Display`, `std::fmt::Formatter`, `std::fmt::Result`, `use std::fmt::Write as _;`, a `{DEBUG_PATH}Struct` builder path, and a `use std::fmt::{self, Display};` group, has no violation.
  - A fixture crate whose `lib.rs` lists `Debug` in a one-line `std::fmt::{...}` group and, on its own line, in a group that spans several lines is reported once for each, at the line holding `Debug`, with the fix `fmt::Debug`.
  - A crate whose manifest does not parse is still scanned, and a scan that finds no `.rs` file fails with one violation saying it scanned nothing.
- `a_re_export_rooted_outside_the_facade_dependencies_is_rejected` keeps passing with `pub use std::sync::Arc;` as its `std`-rooted case.

While iterating, run `cargo test -p build-xtask`, whose live test lists each line still to rewrite, then both clippy commands above, which report a missing `fmt` import and one a normal build leaves unused.

</step-14>

</execution-plan>