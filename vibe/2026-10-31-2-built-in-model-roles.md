---
name: Built-in model roles
overview: "Every prompt gets exactly five roles (default, strong, small, fast, creative), no custom roles, and models: front matter only for requirements (min_context, thinking). The Host passes a plain role-to-model map to prepare; a role with no entry uses default, and a non-empty map without default is refused. The default role is the baseline, models.current() reads the chain's model, models.use() switches it for the rest of the chain, children inherit a copy, and a 3-arg fanout sets the arms' model. Built with fewer types than exist today. Three commits."
todos:
  - id: types-model-role
    content: "runvil-types models.rs: ModelRole enum (Default, Strong, Small, Fast, Creative) with ALL, as_str() and serde Deserialize (kebab-case); unit tests"
    status: pending
  - id: parser-requirements
    content: "Parser: rename ModelRole struct to RoleRequirements { min_context, thinking: Option<bool> }; delete ModelKeyword, keywords:, description:, ModelRoles, ModelRolesIter, keyword_name; add Frontmatter::role_requirements(role); map visitor generic over key type (args keep alias grammar); update workshop ModelDto and the UI's run-api.ts and run-rows.ts; rewrite contract tests"
    status: pending
  - id: modelset-and-lua
    content: "model-client: ModelSet immutable (delete default alias API, ModelView, ModelSetError, lock helper, capabilities); Lua: Arc<ModelSet>; selection is Arc<Mutex<Option<ModelBinding>>> passed into injection and initialized to the default binding; models.use(label or handle, opts?); models.current; delete models.default and resolve_model_binding (SectionVm::current_model); handle loses capabilities; role errors"
    status: pending
  - id: engine-prepare
    content: "Engine: prepare(prompt, BTreeMap<ModelRole, ModelInfo>) checks declared requirements against the filling model, refuses a non-empty map without default (RequirementCheck::Unbound), and builds the ModelSet on RunContext; delete ModelBindings, bindings.rs, model_bindings(), bind_role_for_test, Scenario::bind_roles; RequirementCheck {ContextMinimum, Thinking, Unbound}; UnmetRequirement.role: ModelRole; RunState holds Arc<ModelSet>"
    status: pending
  - id: callers-and-fixtures
    content: "One pass per file: prepare callers (harness prepare_from_snapshot, scenarios, example, bench, tests) and every prompt and fixture with custom labels, keywords or models.default (README, prompts, workshop chat.md, roughly 30 fixture files); regenerate public-api.txt; first design doc update"
    status: pending
  - id: gate-commit-1
    content: "Commit 1 gate: cargo test and clippy for types, parser, model-client, lua, engine, harness, facade, workshop server; UI build; stale-name and wording sweeps; check that model-call events record the model id actually used"
    status: pending
  - id: chain-selection
    content: "Commit 2: Chain owns the selection Arc; ChainStart carries the starting value (default binding for root chains, parent's value for call and spawn, taken at dispatch); end_h1 hands H1's value to the root walk; section setup passes the chain's Arc into injection; tests; design doc update"
    status: pending
  - id: fanout-shim
    content: "Commit 3: 3-arg tasks.fanout(model, worker, collection) entirely in __impl_fanout.lua using privileged nil-safe selection get/set captured at install: switch, spawn every arm, restore before the first join and on the spawn-failure path; tests; design doc update"
    status: pending
  - id: final-gate
    content: "Gates for commits 2 and 3: tests, clippy, public-api check, stale-name and wording sweeps"
    status: pending
isProject: false
---

# Built-in model roles and chain-level current model

<product-contract>

## Product Requirements

Prompt authors declare model roles in `models:` front matter and pick one with `models.default`, although the Host binds every declared role to a single model. This change gives every prompt five fixed roles, makes `models:` optional and limited to requirements, and lets the Host map roles to models. The current model becomes state of the chain, inherited by its children. It ships with a smaller API than exists today and with no compatibility paths.

- Problem and users:
  - Prompt authors must write a `models:` block naming a role label and a `models.default(label)` call before the first model call, then the Host binds every declared role to its one current model (`crates/runvil-internal/parser/src/contract/models.rs`, `crates/runvil-internal/engine/src/execute/fill.rs`, `README.md`).
  - Host integrators (the harness, the Workshop server, embedders of `RunContext`) pass one optional model to `prepare` and cannot express per-role models (`crates/runvil-internal/engine/src/execute/config.rs`).
  - Authors cannot carry a model choice across sections or into fanout arms: the selection lives on the section VM and each new VM starts without one, so arms and later sections fall back to the prompt-wide default (`crates/runvil-internal/lua/src/vm.rs`, `crates/runvil-internal/lua/src/models.rs`).
- Goals:
  - A prompt with no `models:` block runs on the Host's default model.
  - Five fixed roles (`default`, `strong`, `small`, `fast`, `creative`) that a prompt may attach requirements to.
  - A Host maps roles to models with one plain map, and a role without an entry uses `default`.
  - `models.current()` and `models.use(...)` act on a chain-level current model that `call` children, spawned tasks and fanout arms inherit, and a 3-arg `tasks.fanout` sets the arms' model.
  - The same observable results with fewer types and functions than exist today.
- Non-goals:
  - Custom prompt-declared roles.
  - Any compatibility path, alias, stub, migration error, or note about previous behavior.
  - A role for compaction.
  - Per-role model selection in production Hosts.
- Success criteria:
  - A prompt file with no `models:` block and no `models.default` call runs unchanged against a Host that supplies one model.
  - Every deleted name listed in the sweep (Testing Plan) is absent from living code and docs.
  - Every gate in Execution Instructions passes.
- Constraints:
  - Code, doc comments, error messages, test names, README text and the design document describe present behavior only. None says "replaces", "formerly", "previously", "no longer", "legacy", "removed" or "migrated", and no test asserts that a deleted name is gone.
  - Prompts, fixtures and docs are rewritten in place, with no "was X, now Y" annotations.
  - Dated logs under `vibe/` and the generated `.vibelight/` directory are not edited.
  - Each step commit leaves the workspace building with its tests passing.
  - A run's result does not depend on when the scheduler admits a queued task: inherited model state is copied when a call or spawn is dispatched.
- Open questions: None

## Functional Specification

The author-facing surface is five fixed roles, optional requirements in front matter, and the Lua calls `models.current`, `models.use`, `models.get` and `tasks.fanout`. The Host-facing surface is one role-to-model map passed to `prepare`. A chain's current model starts on `default`, persists across the chain's sections, and is copied into children when they are dispatched. Failures surface as parse errors, `prepare` refusals, or run-time errors that name the five roles.

- Actors and workflows:
  - Terms: a chain is the engine's unit of sequential execution, which is the H1 pass, the root walk, a `call` child, or a spawned task, and each fanout arm is a spawned task (`crates/runvil-internal/engine/src/execute/scheduler/chain.rs`). The H1 pass runs the Lua blocks under the prompt's title before the first section (`crates/runvil-internal/engine/src/execute/scheduler/h1.rs`). A section VM is the Lua VM built for one section. A handle is the Lua value that `models.use`, `models.get` and `models.current` return.
  - Prompt author: writes optional `models:` requirements, then uses `models.current()`, `models.use(role or handle, options?)`, `models.get(role)`, `models.infer` and `tasks.fanout`.
  - Host: calls `prepare` with a role-to-model map and handles the returned `Requirements`.
  - A chain starts on the `default` role. `models.use` switches it for the rest of that chain, across its sections.
  - `call` children, `tasks.spawn` tasks and fanout arms start with a copy of the parent's current model taken when the call or spawn is dispatched. The root walk starts with the model H1 ended on. A child's switch never reaches its parent or its siblings.
- Inputs and outputs:
  - Front matter `models:` maps role names to `{ min_context, thinking }`. `min_context` is a positive token count that the model filling the role must meet. `thinking: true` requires a model that can think and turns thinking on for that role's requests. `thinking: false` requires a model that can run without thinking and turns it off.
  - Host map: `BTreeMap<ModelRole, ModelInfo>`. An empty map means the Host has no model. A non-empty map must hold `default`. A role without an entry uses the `default` entry.
  - The model handle exposes `label`, `model_id`, `description`, `context`, `thinking` and `provider`, plus the methods `infer` and `loop`.
  - Lua syntax, every affected form:

    ```lua
    -- FRONT MATTER (YAML shown as comments)
    --   (no models: key)                               valid: the five roles exist
    --   models: { strong: { min_context: 200000 } }    valid: checked against the model filling strong
    --   models: { strong: { thinking: true } }         valid: model must be able to think; strong's requests think
    --   models: { small: { thinking: false } }         valid: model must run without thinking; small's requests do not
    --   models: { fast: {} }                           valid, adds nothing
    --   models: { reviewer: {} }                       PARSE ERROR: unknown role, expected default, strong, small, fast or creative
    --   models: { strong: { tier: 1 } }                PARSE ERROR: unknown field, expected min_context or thinking

    -- READ the chain's current model (a chain starts on the `default` role)
    local m = models.current()           -- handle: m.label, m.model_id, m.description, m.context, m.thinking, m.provider
                                         -- errors "the Host supplied no model" when the Host map is empty

    -- SWITCH the chain's current model (persists across this chain's sections)
    local m = models.use("strong")       -- returns the handle
    models.use("fast", { temperature = 0.2, max_tokens = 512 })
    models.use("default")                -- the `default` role (not a fanout arm's given model)
    models.use(prev)                     -- also accepts a handle; options apply on top of it
    models.use("reviewer")               -- runtime error: not a model role, lists the five

    -- INSPECT or CALL without switching
    local s = models.get("small")
    models.infer(prose)                  -- on the current model
    models.infer(s, prose)               -- explicit handle, frozen binding
    s:infer("prompt")                    -- handle methods
    s:loop()

    -- SAVE AND RESTORE around a block of work
    local prev = models.current()
    models.use("creative")
    local drafts = tasks.fanout(worker, items)
    models.use(prev)

    -- FANOUT
    tasks.fanout(worker, items)                          -- arms inherit the chain's current model
    tasks.fanout(models.get("fast"), worker, items)      -- 3-arg: model handle first, parent not switched
    -- inside an arm: models.use("strong") affects only that arm and its children
    ```

- States and validation:
  - Parse time: an unknown role name, an unknown field, a duplicate role key, or `min_context: 0` is a parse error. The unknown-role error lists the five names.
  - `prepare` time: each declared role's requirements are checked against the model that fills it, fallbacks included.
  - Run time: `models.use` and `models.get` reject a label that is not one of the five, except that `models.get` resolves a raw gateway model id when the run has an application-state snapshot (`crates/runvil-internal/lua/src/models.rs`). `models.use`, `models.get` and `models.current` on a role with no model fail with "the Host supplied no model".
- Errors and recovery:
  - `prepare` reports unmet requirements, and a non-empty map without `default`, in `Requirements`. The caller refuses the run before it starts, as it does for other unmet requirements (`crates/runvil-internal/engine/src/execute/requirements.rs`). The Host recovers by supplying a model that meets the requirement, or a `default` entry.
  - With an empty map, using a model raises `ModelRequired`, and a prompt that never calls a model still runs.
  - A 3-arg fanout restores the parent's model before its first join and also when a spawn fails part-way.
- Security and privacy behavior:
  - Chain isolation only: each chain owns its selection, so a child cannot change its parent's or a sibling's model. No new data crosses the Lua sandbox, and the handle fields listed above are all it exposes.
- Acceptance criteria:
  - A prompt with no `models:` calls `models.infer` on a Host with one `default` model and succeeds.
  - `models:` naming anything other than the five roles fails to parse with an error listing them.
  - With the Host map `{ default: A, strong: B }`, `models.use("strong")` makes the next `models.infer` run on B, and `models.use("fast")` runs it on A.
  - A switch made in one section is still in effect in the next section of the same chain.
  - A parent that switches after spawning but before a queued task is admitted does not change that task's model.
  - `tasks.fanout(models.get("fast"), worker, items)` runs every arm on `fast` and leaves the parent's model as it was.
  - A non-empty Host map without `default` is refused before the run starts.

</product-contract>
<implementation-contract>

## Technical Design

The Host's role map enters `prepare`, which resolves the five roles, checks requirements, and builds an immutable model set shared by the whole run. Each chain owns its current model, and children receive a copy when they are dispatched. Section VMs receive the chain's selection at Lua injection, and the engine reads it through one accessor. The 3-arg fanout is implemented in the Lua shim with no protocol change.

- Architecture:
  - `prepare` resolves the five roles from the Host map (the role's entry, else the `default` entry), checks declared requirements against the resolved models, and builds the run's `ModelSet`. The set is immutable and shared as `Arc<ModelSet>`. Today it sits behind a mutex and a `ModelView` trait only because `models.default` writes to it; the `RunState` documentation names that as its only writer (`crates/runvil-internal/engine/src/execute/config.rs`, `fill.rs`, `context-bound.rs`, `context.rs`, `crates/runvil-internal/model-client/src/model/options.rs`).
  - Each chain owns its current model as `Arc<Mutex<Option<ModelBinding>>>`. Root chains start on the `default` binding, or on `None` when the Host map is empty. `ChainStart` carries the starting value. Call and spawn dispatch copy the parent's value into the child's `ChainStart`, and `end_h1` writes the value H1 ended on into the root walk it starts (`crates/runvil-internal/engine/src/execute/scheduler/chain.rs`, `dispatch.rs`, `tasks-spawn.rs`, `h1.rs`).
  - The copy happens at dispatch because a queued task holds no VM until the scheduler admits it, so a copy at admission would let a later switch in the parent leak into it (`crates/runvil-internal/engine/src/execute/scheduler/tasks-spawn.rs`).
  - Each section VM receives its chain's selection as an argument to injection, which is where the `models` table is installed (`crates/runvil-internal/lua/src/vm/inject.rs`, `crates/runvil-internal/engine/src/execute/section_vm.rs`). The engine reads the current model through `SectionVm::current_model()` for inference, chat rounds and a section's first tool call (`crates/runvil-internal/lua/src/vm/state.rs`, `crates/runvil-internal/engine/src/execute/scheduler/dispatch.rs`, `chat.rs`, `crates/runvil-internal/engine/src/execute/section_context.rs`). The selection holds the whole `ModelBinding`, which lets `models.use` accept a handle, including a raw-id handle from `models.get`.
  - The 3-arg fanout lives in the Lua shim (`crates/runvil-internal/lua/src/__impl_fanout.lua`). It sets the parent's selection to the given handle's binding, spawns every arm, and restores the selection before the first join and before re-raising on a spawn failure. The shim already spawns every arm before any join, no author code runs in the parent between the switch and the restore, and `models.use` emits no event, so the switch is unobservable. The shim reads and sets the selection through privileged captures installed with it (`crates/runvil-internal/lua/src/coro.rs`).
  - Data flow:

    ```mermaid
    flowchart TD
      hostMap[Host role map] --> prep[prepare]
      prep -->|"refusals"| reqs[Requirements]
      prep -->|"5 bindings"| set[ModelSet]
      set --> h1[H1 chain]
      h1 -->|"value at end_h1"| root[Root walk]
      root -->|"copy at call"| callChild[call child]
      root -->|"copy at spawn"| arm[fanout arm]
    ```

- Modules and interfaces:
  - `ModelRole` enum in `crates/runvil-internal/types/src/models.rs`, re-exported by the facade in `crates/runvil/src/lib.rs`. The parser, the Lua crate and the engine already depend on `runvil-types`.
  - Parser (`crates/runvil-internal/parser/src/contract/models.rs`, `contract.rs`): `RoleRequirements { min_context, thinking }`, which is the parser's existing `ModelRole` struct under a new name and frees `ModelRole` for the enum. `Frontmatter::role_requirements(ModelRole)` is the only accessor. The shared contract-map visitor becomes generic over the key type, so `args` keeps alias-grammar string keys and `models` uses `ModelRole`, with duplicate-key rejection shared.
  - Engine: `RunContext::prepare`, `RequirementCheck` as `{ ContextMinimum, Thinking, Unbound }`, and `UnmetRequirement.role: ModelRole`. A declared role's `thinking` also sets the thinking switch in that role's `ModelInvocation`, as `bound_model_set` does for the keyword today (`crates/runvil-internal/engine/src/execute/context-bound.rs`).
  - Lua (`crates/runvil-internal/lua/src/models.rs`, `models-userdata.rs`): `models.use`, `models.current` and `models.get` as specified above, and the handle without a `capabilities` field.
  - Host API:

    ```rust
    // ModelRole lives in runvil_types::models, re-exported as runvil::model::ModelRole.
    enum ModelRole { Default, Strong, Small, Fast, Creative }   // ALL, as_str(); Deserialize (kebab-case)

    // RunContext::prepare(self, prompt: &Prompt, models: BTreeMap<ModelRole, ModelInfo>)
    //     -> (RunContext, Requirements)
    ctx.prepare(&prompt, BTreeMap::new())                                        // model-less Host
    ctx.prepare(&prompt, BTreeMap::from([(ModelRole::Default, model)]))          // one model for all five
    ctx.prepare(&prompt, BTreeMap::from([(ModelRole::Default, d), (ModelRole::Strong, s)]))
    // A non-empty map without Default adds
    //   UnmetRequirement { role: ModelRole::Default, check: RequirementCheck::Unbound, .. }
    // and requirements.refusal() fails the run, like any other unmet requirement.
    ```

- File and public API changes:
  - Added: `ModelRole` with `ALL` and `as_str()`; `RoleRequirements` with `min_context()` and `thinking()`; `Frontmatter::role_requirements`; `RequirementCheck::Thinking` and `RequirementCheck::Unbound`; Lua `models.current`, `models.use(handle)` and the 3-arg `tasks.fanout`.
  - Changed: the signature of `RunContext::prepare`; the type of `UnmetRequirement.role` from `String` to `ModelRole`; the Workshop prompt-contract JSON model entries become `{ label, min_context, thinking }` (`crates/workshop/server/src/routes/prompts.rs`), read by `crates/workshop/ui/src/services/run-api.ts` and `crates/workshop/ui/src/parts/run/run-rows.ts`.
  - Deleted from the facade's public API (`crates/runvil/public-api.txt`): `ModelBindings` with its five methods and `RunContext::model_bindings()`; `ModelRoles`, `ModelRolesIter` and `Frontmatter::models()`; `ModelKeyword`; `ModelBinding::capabilities` and `with_capabilities`; `RequirementCheck::HardKeyword`.
  - Deleted from the Lua and front matter surface: `models.default`, the handle's `capabilities` field, front matter `keywords:` and per-role `description:`.
  - Deleted across modules: `ModelView`, `ModelSetError`, the default alias and its accessors on `ModelSet`, the run-wide model-set mutex, `ModelSelection` and `resolve_model_binding`.
  - Regenerate `crates/runvil/public-api.txt`.
  - Rewrite in place: `README.md`; `prompts/hello.md`, `prompts/greet.md`, `prompts/research-person.md`, `prompts/analyst-example.md`; `crates/workshop/agents/agents/chat.md`; about 30 fixtures that embed `models:` front matter under `crates/runvil-internal/engine`, `crates/harness-internal/runner/tests/it`, `crates/harness/tests/suite`, `crates/runvil/tests/suite`, `crates/harness-gateway-client/src/wire/request-tests.rs` and `crates/workshop/server`; and the models section of `runvil-design/design/architect-runvil-prompting-language.md` in the sibling design repository.
- Data, persistence, failure, security, and privacy constraints:
  - The only externally visible data change is the Workshop prompt-contract JSON above. No other persisted format changes.
  - `prepare` refuses a run before any effect is issued: unmet requirements and a missing `default` are carried in `Requirements`, and the harness only merges and refuses (`crates/harness-internal/runner/src/prepare.rs`). No non-test code outside `requirements.rs` and the constructing code in `fill.rs` matches on `RequirementCheck` or reads `UnmetRequirement.role` or `.check`, so those types can change freely.
  - The harness converts its `Option<ModelInfo>` from `current_model` into the Host map at one place, `prepare_from_snapshot`: an empty map for `None`, `{ default: m }` for `Some(m)` (`crates/harness-internal/runner/src/environment.rs`, `crates/harness-internal/runner/src/prepare.rs`).
  - Chain isolation: no two chains share a selection, and inheritance copies a value.

</implementation-contract>
<verification-contract>

## Testing Plan

Tests cover the parse rules, `prepare` resolution, the Lua surface, chain inheritance and the 3-arg fanout, each in the step that introduces the behavior. Two sweeps check that deleted names and backward-looking wording are gone. Each component ends with a gate of full tests, lints and the sweeps.

- Unit:
  - Types: `ModelRole` deserializes each of the five kebab-case names, and an unknown name's error lists all five.
  - Parser: no `models:`; each role declared; `min_context` and `thinking` read back; an unknown role rejected with the five names; an unknown field (`tier`) rejected; a duplicate role key rejected; `min_context: 0` rejected; `args` keys still checked against the alias grammar.
  - `prepare`:
    - a one-model map gives all five roles that model;
    - a map with a `strong` entry gives only `strong` that model;
    - `min_context` on a fallback role is checked against the default model;
    - `thinking: true` against a model whose thinking mode is `Never`, and `thinking: false` against one whose mode is `Always`, are each refused;
    - a non-empty map without `default` is refused with `Unbound`;
    - an empty map leaves the set empty, and `models.infer` raises `ModelRequired`;
    - a declared `thinking` sets the thinking switch on that role's requests.
  - Lua:
    - a prompt with no `models:` can `models.use("strong")`;
    - `models.current` reflects `models.use`;
    - `models.use(handle)` works with options applied on top;
    - an unknown label errors with the five names listed;
    - a role with no model errors with the Host-supplied-none message;
    - the handle exposes `label`, `model_id`, `description`, `context`, `thinking` and `provider`.
- Integration and end-to-end:
  - Chain selection (second component):
    - a switch persists across a chain's sections;
    - a `call` child and a spawned task each inherit the selection;
    - a child's switch does not reach the parent or a sibling;
    - a switch in H1 reaches the root walk;
    - a parent that switches after spawning but before a queued task is admitted does not change that task's model;
    - all arms of one fanout start on the same model.
  - Fanout (third component):
    - a 3-arg fanout runs every arm on the given model;
    - the parent's model after the call is what it was before;
    - a spawn failure part-way still restores it;
    - a raw-id handle works with an empty Host map;
    - a non-handle first argument is an error.
  - A prompt with no `models:` runs end to end through the harness against a one-model Host, using the rewritten fixtures.
- Regression, security, and performance:
  - Deleted-name sweep: search `crates`, `README.md` and `prompts` in the runvil repository, and `design` in the sibling design repository, for `models.default`, `models.bind`, `default_alias`, `set_default_alias`, `ModelView`, `ModelSetError`, `ModelBindings`, `model_bindings`, `ModelRoles`, `ModelRolesIter`, `ModelKeyword`, `keyword_name`, `keyword_wire`, `with_capabilities`, `HardKeyword`, `resolve_model_binding`, `trivial fill` and `bound model role`. Expect no hits. Skip `vibe/` and `.vibelight/`.
  - Wording sweep: search the added lines of the diff for `no longer`, `formerly`, `previously`, `legacy`, `deprecat`, `backward`, `replaces` and `migrat`. Expect no hits.
  - Public API check: `crates/runvil/public-api.txt` is regenerated and matches the surface listed in Technical Design.
  - Security: the child-isolation tests above.
  - Usage attribution: with the Host map `{ default: A, strong: B }`, the events of a `strong` round carry B's model name. Events take their `model` field from the completion (`crates/runvil-internal/engine/src/execute/turn.rs`, `crates/runvil-internal/types/src/event.rs`).
  - Performance: no requirement.
- Exit criteria:
  - `cargo test` and clippy pass for the types, parser, model-client, lua, engine, harness, facade and Workshop server crates, and the Workshop UI builds.
  - Both sweeps return no hits and the public API check passes.
  - Each component meets these criteria at its last step before the next component begins.

</verification-contract>
<decision-record>

## Decision Record

- Decisions:
  - Exactly five fixed roles and no custom roles. Roles name tiers the Host maps to models, and a custom label added only ceremony because the Host bound every label to the same model. User: "default, strong, small, fast, creative" and "no custom roles".
  - `thinking` is a requirement on a model, not a role. User: "\"thinking\" is a property of a model not a model role".
  - No `compact` role yet. User: "for compact we defer because we dont have enough evidence to know what is needed."
  - `models:` is optional and carries only `min_context` and `thinking`. With fixed role names the soft keywords and per-role `description` had no runtime effect; they fed only the handle's `capabilities` and `description` fields and the Workshop run panel text (`crates/runvil-internal/engine/src/execute/context-bound.rs`, `crates/workshop/ui/src/parts/run/run-rows.ts`), and the two hard keywords are one tri-state. User: "no prompt that lacks a \"models:\" section should fail"; from the maintainers' design discussion, "we dont want to remove the possibility of role requirements, just avoid the need for the typical prompt to have to write" a `models:` block; and the user chose the option of only `min_context` and `thinking`.
  - The Host passes a plain `BTreeMap<ModelRole, ModelInfo>`, required in every case, empty allowed, and a non-empty map must hold `default`. A role without an entry uses `default`. A missing `default` is refused through `Requirements` as `Unbound`, and the keys are the closed `ModelRole` enum so a typo cannot fall back silently. User: "Just require a map in all cases. And the map can be empty. if it is not empty, it must have at least \"default\"" and "the harness will use the default model for any missing role"; the user chose the plain map and the closed role type.
  - The current model is chain state. A chain starts on `default` and a switch persists until the next switch. User: "a chain should start with \"default\" and that stays until the model is switched. models.current() returns the current model object, models.use(<role>) switches roles and returns the model object."
  - `models.default` is deleted and the `default` role is the baseline. The call was the only writer of the shared model set, which is why the set needed a mutex. The idea came from the maintainers' design discussion, where David Rauschenbach wrote "maybe we should get rid of that, and add a \"default\" role", and Jason agreed with dropping `models.default`.
  - Children inherit a copy of the parent's current model, taken at dispatch. User: "fanout inherits the chain's current model". Taking the copy at dispatch keeps a queued task's model independent of when the scheduler admits it.
  - A 3-arg `tasks.fanout(model, worker, collection)` with the model handle first, implemented in the Lua shim. User: "maybe we add a 3-arg fanout which puts the model object first". The shim switch-and-restore needs no protocol or scheduler change and spares authors a manual save and restore.
  - `models.use` accepts a handle as well as a role label. It comes from the design discussion's `model.use(prev)` snippet and keeps the save-and-restore pattern working. It was proposed during planning and kept in every revision of this plan without objection.
  - No backward compatibility, and no trail to old behavior in code, docs, errors or tests. User: "I want no backward compatibility. Dont leave behind notes referring to the old behavior, etc.. just change it."
  - Reduce the API surface while keeping the results. User: "see if you can reduce the API surface and get the same results". Applied as: an immutable `ModelSet`, `prepare` building the model set directly so `ModelBindings` goes, and the selection storing a whole `ModelBinding`.
  - `Frontmatter::role_requirements(role)` returns `Option<&RoleRequirements>`: `None` when the prompt does not declare the role, and `Some` with neither field set when it declares `role: {}`. The Workshop prompt contract lists exactly the declared roles, which needs that distinction. The plan named the accessor without its return type.
  - The work is built as three components in eight steps. Each step is one commit that leaves the workspace building with its tests passing, and the component that ends at a step carries that step's gate of full tests, lints, the public API check and both sweeps. The eight steps are: (1) the `ModelRole` type; (2) the Host role map in `prepare`; (3) the immutable model set with the `default` role as baseline; (4) `models.use` with a handle, `models.current` and the handle fields; (5) requirements-only front matter with the Workshop contract and UI, which ends the first component and its gate; (6) the chain-owned current model with inheritance at dispatch, which ends the second component and its gate; (7) the model-first `tasks.fanout`, which ends the third component and its gate; (8) one design-document step in `runvil-design`, written after all the code, that describes the final surface of all three components. The rationale is fewer review cycles, and a document that describes the finished surface once instead of three partial states. The owner said "12 steps seems like a lot" about a split that gave each component its own design-document commit, and chose this eight-step shape. The three components are independently shippable, and only the second carries real risk.
- Rejected alternatives:
  - Custom prompt-declared roles alongside the five. The user rejected them. Revisit only if the five tiers prove insufficient, which would be a change to the prompt language.
  - A `thinking` role. The user rejected it because thinking is a model property.
  - Keeping `prepare` on a single optional model. Rejected because per-role models are the purpose of the map. No revisit condition.
  - A required default-model parameter in place of a map. Rejected because the harness supports Hosts with no model, where `current_model` returns `None` for no selection and an empty catalog (`crates/harness-internal/runner/src/environment.rs`), and such a Host must still run prompts that call no model. Revisit if the harness stops supporting model-less runs.
  - A `RoleModels` newtype whose constructors make a map without `default` unrepresentable. Rejected for the plain map and an `Unbound` refusal, which adds no type, error or constructors. Revisit if Hosts build maps in many places and want the rule enforced by construction.
  - `FromStr` and an `UnknownRole` error for `ModelRole`. Rejected because the serde derive already lists the five names. Revisit when a Host reads role names from configuration.
  - Rolling the selection forward like `var` at each section exit. Rejected for a chain-owned selection, because rolling forward needs a read-back at about five exit sites (`end_section`, `apply_jump`, chain `finish`, `step.rs`, `end_h1` under `crates/runvil-internal/engine/src/execute/scheduler`). No revisit condition.
  - A model field on the spawn request, and a separate setter on the VM called before injection. Rejected for the shim switch-and-restore and an injection argument, which is the only place the `models` table is installed. Revisit if a consumer needs the fanout's model switch as an event.
  - Keeping the soft keywords, `description` and the handle's `capabilities`. Rejected as dead surface. Revisit if a Host needs role metadata.
  - A `models.default` stub or a migration error. Rejected by the no-compatibility decision.
- Assumptions, risks, and notes:
  - Paths in this plan are relative to the root of the `runvil` repository, except paths starting with `runvil-design/`, which are in the sibling design repository.
  - "No prompt that lacks a `models:` section should fail" is read as a statement about front matter. With an empty Host map a prompt fails only when it calls a model, which follows from "the map can be empty".
  - Model-call events carry a `model` field set from the completion, not from the request (`crates/runvil-internal/engine/src/execute/turn.rs`, `crates/runvil-internal/types/src/event.rs`), so attribution by model is available. Whether `Completion::model()` falls back to the requested model when a backend omits the name was not established, and events then carry a `ModelMetadataDegraded` signal. A test pins the attribution rather than assuming it.
  - The search for `RequirementCheck` consumers skipped gitignored paths and the Workshop UI `dist` bundles, and a read of `role` or `check` through JSON would not have matched the type names. It found no non-test consumer.
  - Persistence is a cost footgun: a section that calls `models.use("strong")` for one expensive step leaves every later section of that chain on `strong`. The behavior is documented and tested, and a scoped form can follow.
  - Requirement failures reflect fallbacks: `strong: { min_context: 200000 }` is refused on a Host whose default model is smaller and that supplies no `strong` entry. That is intended and can surprise a Host author.
  - The `model` field of the Lua `sys` table is fixed at a section's first tool call (`crates/runvil-internal/engine/src/execute/section_context.rs`), so with chain-level persistence it shows the chain's current model at that moment.
  - `creative` may overlap with the `temperature` option on `models.use` (`crates/runvil-internal/lua/src/models.rs`). It stays because the user chose it as the fifth role.
  - Commit messages describe the change as usual. The no-trail constraint covers code, docs, error messages and tests. The user did not answer whether it also covers commit messages.
  - Confidence: medium-high. Every seam was checked against the code, and what remains is the size of the first component and the event-attribution check.

### Deferred and Out of Scope

- Deferred: a role for compaction. Revisit when evidence shows what `models.compact` needs (`crates/runvil-internal/lua/src/models.rs`).
- Deferred: per-role model selection in production Hosts. The harness passes only `default` because its host snapshot carries one `selected_model` (`crates/harness-internal/runner/src/environment.rs`). Revisit when that snapshot carries per-role selections.
- Deferred: a scoped model-switch form. Revisit if chain-level persistence leaks cost in practice.
- Deferred: `FromStr` for `ModelRole`. Revisit when a Host reads role names from configuration.
- Out of scope: the handle's duplicate `name` and `label` fields, and the `h:infer` and `h:loop` forms that duplicate `models.infer(h, ...)` and `models.loop(h, ...)` (`crates/runvil-internal/lua/src/models-userdata.rs`).
- Out of scope: a model argument on `tasks.spawn`.
- Out of scope: editing dated logs under `vibe/` and the generated `.vibelight/` directory.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked` builds the default members (`runvil`, `runvil-plugin`, `harness`, `harness-gateway-client`) with no native toolkit. The desktop app is `cargo build --locked -p workshop`, which needs `npm ci --prefix crates/workshop` and a staged Gateway sidecar (`node tools/fetch-gateway-sidecar.mjs --stub --target <triple>`, removed afterward with `node tools/stage-gateway-sidecar.mjs remove --target <triple>`). The UI bundle alone is `npm run build --workspace ui`, run in `crates/workshop`.
- Focused test command pattern: `cargo nextest run --locked -p <crate> <test-name-substring>`. A single UI test file is `node --test crates/workshop/ui/test/<file>.mjs`. The structural checks are `cargo test -p build-xtask`.
- Component test command pattern: `cargo nextest run --locked -p <crate>` for a Rust crate, and `npm test --workspace <ui|look|platform>` run in `crates/workshop` for a UI package.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then the workshop crates separately with `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`. UI: `npm test --workspaces --if-present` in `crates/workshop`. Wording scan: `node --test crates/workshop/ui/test/docs-claims.mjs`.
- Linter command: `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`, and for workshop `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`. Both run with `CARGO_BUILD_WARNINGS=deny` in the gate. The UI also has `npm run typecheck --workspaces --if-present` in `crates/workshop`.
- Formatter check command: `cargo fmt --all --check`
- Docs command: `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api` with `RUSTDOCFLAGS="-D warnings"`. The facade docs are `cargo doc -p runvil --no-deps` and `cargo doc -p harness --no-deps`, both without `--all-features`. The public surface check is `cargo +<pinned nightly> xtask api --check`, where the nightly is named in `crates/build-xtask/src/api/toolchain.rs`.
- Test placement and naming conventions: a unit test lives in a sibling file named `<stem>-tests.rs`, wired as `#[cfg(test)] #[path = "<stem>-tests.rs"] mod tests;`, or in a `tests/` module directory beside the source when it has three or more files. An integration suite is one directory per binary, `crates/<crate>/tests/<suite>/main.rs` (for example `crates/runvil/tests/suite`, `crates/harness/tests/suite`, `crates/plugin-std/tests/it`) with one topic file per area, and prompt fixtures sit in `tests/prompts`. Test functions are `#[test]` with long sentence-style snake_case names that state the behavior. UI tests are `.mjs` files run by `node --test`, in `crates/workshop/ui/test/` and as `src/**/*.test.mjs`. Benches are `harness = false` and need the crate's `test-support` feature. Tests under `tools/` are `<tool>.test.mjs`.
- Directory map:
  - `crates/` holds the shared and public layer: `runvil` (Engine facade), `runvil-plugin`, `harness`, `harness-gateway-client`, `plugin-mcp`, `plugin-std`, `plugin-user-input`, `plugin-web`, `shared-error-source`, and the build crates (`build-ceiling`, `build-ui`, `build-user-guide`, `build-workshop`, `build-xtask`).
  - `crates/runvil-internal/` holds the Engine's private crates: `engine`, `lua`, `model-client`, `parser`, `types`, `vfs`.
  - `crates/harness-internal/` holds the Harness's private crates: `runner`, `plugin-loader`, `test-plugin`.
  - `crates/workshop/` holds the Workshop crates (`agents`, `desktop`, `gateway`, `gateway-discovery`, `menu`, `protocol`, `registry`, `run-log`, `server`, `server-api`, `status`, `support`, `user-state`, `workspace`) and the npm workspace members `ui`, `look`, `platform`.
  - `guide/` holds the books (`language`, `workshop`), the site chrome, and the landing page.
  - `prompts/` holds the sample prompts. `tools/` holds the Node scripts for the Gateway pin and sidecar. `gateway-pin/` holds the pinned Gateway release (`pin.json`).
  - `vibe/` holds dated plan notes, with `scratch/` ignored. `local/` holds local-only files. `.vibelight/` holds generated crate outlines. `.github/` holds the CI, nightly, release, installer-smoke, and site workflows. `.githooks/` holds `pre-commit` (format check) and `pre-push` (wording scan, clippy, `cargo deny`). `.cursor/rules/` holds the Workshop rules. `.config/nextest.toml` holds the nextest profiles. `images/` holds README images.
- Component boundaries: the Engine is `runvil-types` and `runvil-vfs` at the bottom, then `runvil-model-client`, `runvil-lua`, `runvil-parser`, and `runvil-engine`, fronted by `runvil` and `runvil-plugin`. Only those two crates may depend on `crates/runvil-internal`. The Harness is `harness-runner` and `harness-plugin-loader` behind `harness`, which depends on `runvil` and `runvil-plugin`, and `harness-gateway-client` sits on `harness` and `plugin-web`. Each Pack crate (`plugin-*`) depends only on `runvil-plugin`. The Workshop tiers run one way, server to features to services to vocabulary: `workshop-server` composes the feature and service crates. `workshop-agents` and `workshop-run-log` sit on `harness`, and the rest sit on `workshop-registry`, `workshop-support`, and `workshop-protocol`, with `workshop-registry` also depending on `workshop-protocol`. The desktop app `workshop` depends only on `workshop-server-api`, which fronts `workshop-server`. Engine crates never depend on `tokio`, `reqwest`, or the other crates the structural rules ban. Code in one family reaches another only through a crate at the `crates/` level, and `cargo test -p build-xtask` enforces the boundaries.
- Conventions summary: the Rust edition is 2024 on a pinned stable 1.99 toolchain with an MSRV of 1.95, and `rustfmt` uses `style_edition = "2024"`. Workspace lints deny `unsafe_code`, `unwrap_used`, `expect_used`, `todo`, and clippy `all` and `pedantic`, and `allow_attributes` is denied, so a suppression uses `#[expect(..., reason = "...")]`. Source directories are flat, and a subdirectory needs at least three files, otherwise the files sit beside the parent as `foo-bar.rs` with an explicit `#[path]`. Every `workshop-*` `lib.rs` is a facade whose `//!` doc carries a `## Invariants` marker. The seven defined terms (Engine, Harness, Host, Pack, Plugin, Static Set, Dynamic Set) are capitalized and each has one meaning, enforced by `docs-claims.mjs`. Engine crates call their caller "the caller" and never name the Host. Behavior changes ship with tests in the same change. Error messages are written for a model to read. Files use LF endings, and Windows links with `rust-lld` and a static CRT.

</project-survey>
<execution-plan>

## Execution Instructions

<step-1>

### Step 1: ModelRole type [completed]

- Component: Fixed roles and Host map
- Tests:
  - `cargo nextest run --locked -p runvil-types model_role`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy --locked -p runvil-types --all-targets`
  - `git add -N .; git diff -U0 HEAD | rg -n -i "^\+.*(no longer|formerly|previously|legacy|deprecat|backward|replaces|migrat)"` (a pass prints nothing and exits with code 1)

Add the closed set of five roles to `runvil-types`.

- Placement: this component goes first because the other two build on its immutable model set, its selection argument and the Host role map, and the third needs the second's copy at dispatch. This piece, the role type, is built first and on its own because the parser, model-client, Lua, engine and facade all name it and it needs none of them.
- Build state: the workspace builds and passes after this step. Nothing outside `runvil-types` changes.
- Artifacts: in `crates/runvil-internal/types/src/models.rs` add the enum `ModelRole` with variants `Default`, `Strong`, `Small`, `Fast`, `Creative`. Add `ModelRole::ALL: [ModelRole; 5]` in that order and `ModelRole::as_str(self) -> &'static str` returning `default`, `strong`, `small`, `fast` and `creative`. Derive `Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize` with `#[serde(rename_all = "kebab-case")]`, and add `#[non_exhaustive]` the way `ThinkingMode` has it, so the type works as a `BTreeMap` key. Do not add `FromStr`, `Display` or an unknown-role error type, because the serde error already lists the five names. The doc comment says each role is a tier the Host maps to a model, and that a role with no model of its own uses `default`. Put the tests in the file's existing `mod tests`.
- Test cases (sentence-style test names that contain `model_role`):
  - each of `"default"`, `"strong"`, `"small"`, `"fast"` and `"creative"` deserializes with `serde_json::from_str` to its variant;
  - `ModelRole::ALL` lists the five in the order above, and each role's `as_str()` text deserializes back to the same role;
  - the name `"reviewer"` fails to deserialize, and the error text names all five of `default`, `strong`, `small`, `fast` and `creative`.
- Rules: nothing you write (code, comments, docs, error messages, test names) says "replaces", "formerly", "previously", "no longer", "legacy", "removed" or "migrated", or notes what a name used to be, and no test asserts that a deleted name is gone. Do not edit `vibe/` or `.vibelight/`.

</step-1>

<step-2>

### Step 2: Host role map in prepare

- Component: Fixed roles and Host map
- Tests:
  - `cargo check --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo check --locked -p workshop-server --all-targets`
  - `cargo nextest run --locked -p runvil-engine -p runvil -p harness-runner -p harness -p harness-gateway-client --all-features`
  - `cargo nextest run --locked -p workshop-agents`
  - `cargo nextest run --locked -p build-xtask`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy --locked -p runvil-engine -p runvil -p harness-runner -p harness --all-targets --all-features`
  - `cargo +nightly-2026-09-05 xtask api --check`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `rg -n "ModelBindings|model_bindings|bind_role_for_test|bind_roles|HardKeyword|trivial fill|bound model role" crates README.md prompts` (a pass prints nothing and exits with code 1)
  - `rg -n "models\.default" prompts README.md crates/runvil crates/harness crates/harness-internal crates/harness-gateway-client crates/workshop` (a pass prints nothing and exits with code 1)
  - `git add -N .; git diff -U0 HEAD | rg -n -i "^\+.*(no longer|formerly|previously|legacy|deprecat|backward|replaces|migrat)"` (a pass prints nothing and exits with code 1)

Change `RunContext::prepare` to take the Host's role map, so five roles exist for every prompt and a Host can give each its own model. The parser keeps reading `models:` with its current label grammar until step 5, so in this step prompts and fixtures use the five role names as their labels.

- Placement: this piece, "Host map to model set", is steps 2 to 4, built one after another. The plan treats the engine, model-client and Lua changes as one compile unit, but that holds only if the front matter reader changes at the same time. Keeping the parser's reader until step 5 lets each of the three steps build and pass on its own. The piece comes before the front matter piece because the front matter's requirements are checked against the models this piece resolves.
- Build state: the workspace builds and passes after this step. The model set is still behind its mutex and still has its default alias, which step 3 removes.
- Behavior:
  - `prepare(self, prompt: &Prompt, models: BTreeMap<ModelRole, ModelInfo>) -> (RunContext, Requirements)`. A role's model is its own map entry, else the `default` entry. An empty map means the caller has no model: the model set stays empty and nothing is checked. A non-empty map without a `default` entry adds one unmet requirement `UnmetRequirement { role: ModelRole::Default, check: RequirementCheck::Unbound, required: "a model", actual: "none" }`, and the model set stays empty.
  - Declared requirements are read with the parser's current accessor, `prompt.frontmatter().models().get(role.as_str())`, for each role in `ModelRole::ALL`. A declared label that is not one of the five is ignored in this step. `min_context` above the context of the model that fills the role is `RequirementCheck::ContextMinimum` with `required` the minimum and `actual` the model's context, both as decimal text. The hard keyword `thinking` against `ThinkingMode::Never`, and `no-thinking` against `ThinkingMode::Always`, is `RequirementCheck::Thinking` with `required` set to `thinking: true` or `thinking: false` and `actual` set to `Never` or `Always`. Soft keywords and `description` are not checked.
  - The model set holds five `ModelBinding`s in `ModelRole::ALL` order, labelled with `as_str()`, for every prompt whether or not it declares the role. Build each as `bound_model_set` builds one today (the declared keywords become `capabilities`, a hard keyword sets `ModelInvocation::new().with_thinking(Some(true))` or `Some(false)`, the declared description else the model info's, `info.context()`, and `.with_catalog_info(info.clone())`). When the set has a `default` binding, call `set.set_default_alias("default".to_owned())` so a section that never calls `models.use` runs on it. Step 3 replaces that call.
  - The refusal notice lines, exactly: `- role 'strong': requires a context of at least 200000 tokens; the model filling it provides 32000`, then `- role 'strong': requires 'thinking: true'; the thinking capability of the model filling it is Never`, then `- role 'default': requires a model; the caller's role map has no 'default' entry`. Engine text says "the caller" and never names the Host.
- Artifacts (Engine):
  - `crates/runvil-internal/engine/src/execute/config.rs`: the new `prepare` signature. Replace the field `model_bindings: ModelBindings` with `model_set: ModelSet` (empty in `new`, and in the `Debug` output). Delete `RunContext::model_bindings()` and `bind_role_for_test`. Rewrite the `prepare` and `with_ui` docs for the role map and the five roles (`models.get` resolves a raw gateway model id for a label outside the five roles when a UI snapshot is set).
  - `crates/runvil-internal/engine/src/execute/fill.rs`: `fill_model_set(prompt, models, &mut requirements) -> ModelSet` in place of `fill_model_bindings`, holding the build described above. Delete `bound_model_set` from `context-bound.rs`, and make the code in `RunState::new` (`crates/runvil-internal/engine/src/execute/context.rs`) that called it take the prepared set from the context. It still wraps it in `Arc<Mutex<ModelSet>>`.
  - Delete `crates/runvil-internal/engine/src/execute/bindings.rs` with `git rm`, its `mod bindings;` line in `execute.rs`, and `ModelBindings` from the exports in `crates/runvil-internal/engine/src/lib.rs`. Re-export `ModelRole` from the engine's `model` module next to `ModelInfo`.
  - `crates/runvil-internal/engine/src/execute/requirements.rs` and `requirements-tests.rs`: `RequirementCheck { ContextMinimum, Thinking, Unbound }`, `UnmetRequirement.role: ModelRole`, the notice lines above using `role.as_str()`, and the docs of `Requirements::unmet_models`, `UnmetRequirement` and `RequirementCheck`.
  - `crates/runvil-internal/engine/src/test_support/scenario.rs`: the scenario holds `BTreeMap<ModelRole, ModelInfo>` for `prepare`. `Scenario::current_model(model)` inserts the `default` entry, add `Scenario::role_model(role, model)`, and delete `Scenario::bind_roles` with its field and the loop that called `bind_role_for_test`.
- Artifacts (facade and Harness):
  - `crates/runvil/src/lib.rs`: in the `model` module drop `ModelBindings` and add `ModelRole`. Regenerate `crates/runvil/public-api.txt` with `cargo +nightly-2026-09-05 xtask api --bless`. `runvil::prompt::ModelRole` (the parser's struct) stays until step 5, and the two names live in different modules.
  - `crates/harness-internal/runner/src/prepare.rs`: `prepare_from_snapshot` converts its `Option<ModelInfo>` into the map in this one place, an empty map for `None` and `{ default: model }` for `Some(model)`. Update the doc of the `model` field there to say it is the Host's default model.
  - Every other `prepare` caller passes a map: `None` becomes `BTreeMap::new()` and `Some(m)` becomes `BTreeMap::from([(ModelRole::Default, m)])`. Find them with `rg -n "\.prepare\(" crates`. They include `crates/runvil-internal/engine` (`benches/models_loop.rs`, `src/execute/run/tests.rs`, `tests-drops.rs`, `src/execute/tests/live_infer.rs`, `src/execute/tests/suite/prepare.rs`, `src/execute/requirements-tests.rs`), `crates/runvil` (`examples/greeter.rs`, `tests/suite/prepare.rs`, `prompt.rs`, `replay.rs`, `support/scenario.rs`), `crates/harness-internal/runner/tests/it` (`support.rs`, `effect_loop-cap.rs`, `effect_loop-broker.rs`, `prepare*.rs`, `tool_context.rs`) and `crates/harness-gateway-client/src/wire/request-tests.rs`.
- Artifacts (prompts, fixtures, README): find every prompt and fixture that declares a custom label or calls `models.default` with `rg -n "models\.default|models:" crates prompts README.md`. Rewrite each in place so it works with the five roles:
  - a prompt that needs no requirement drops its `models:` block and its `models.default(...)` call, and runs on the `default` role;
  - a prompt that declares a requirement keeps a `models:` block keyed by a role name (`strong`, `small`, `fast`, `creative` or `default`), keeps only `min_context` and the hard keywords `thinking` and `no-thinking`, and drops soft keywords and `description:` unless a test in this step asserts on them;
  - `models.use('<label>')` and `models.get('<label>')` calls name a role;
  - the four sample prompts `prompts/hello.md`, `prompts/greet.md`, `prompts/research-person.md` and `prompts/analyst-example.md`, the Workshop agent prompt `crates/workshop/agents/agents/chat.md`, and the roughly 30 fixtures under `crates/runvil-internal/engine`, `crates/harness-internal/runner/tests/it`, `crates/harness/tests/suite`, `crates/runvil/tests/suite` and `crates/harness-gateway-client/src/wire/request-tests.rs`.
  - `README.md` shows `prompts/greet.md` verbatim, so change the code block in step with the prompt, and rewrite the paragraph under it in plain words: the prompt declares no models, so `models.infer(prose)` runs on the Host's default model, a prompt can pick another of the five roles (`default`, `strong`, `small`, `fast`, `creative`) with `models.use(role)`, and the Host decides which model fills each role.
- Test cases (engine tests in `crates/runvil-internal/engine/src/execute/tests/suite/prepare.rs` and `requirements-tests.rs`, facade tests in `crates/runvil/tests/suite/prepare.rs`):
  - a one-model map `{ default: M }` gives all five roles M: the model set holds five bindings, each with M's id;
  - a map `{ default: A, strong: B }` gives `strong` the model B and the other four roles A;
  - `models: { strong: { min_context: 200000 } }` with only a 32000-token `default` model adds one unmet entry with role `ModelRole::Strong`, check `ContextMinimum`, required `200000` and actual `32000`;
  - `keywords: [thinking]` on `strong` against a model whose mode is `ThinkingMode::Never`, and `keywords: [no-thinking]` on `small` against a model whose mode is `ThinkingMode::Always`, are each refused with check `Thinking`, required `thinking: true` and `thinking: false`, and actual `Never` and `Always`;
  - a `no-thinking` role on a `ThinkingMode::Switchable` model prepares, and its round's request has `thinking()` of `Some(false)`;
  - a non-empty map holding only a `strong` entry gives exactly one unmet entry `{ role: Default, check: Unbound, required: "a model", actual: "none" }`, and `requirements.refusal()` is a `RunErrorKind::RequirementsUnmet` error whose notice contains the `default` line above;
  - an empty map leaves the model set empty and adds no unmet entry, `models.infer` raises `ModelRequired`, and a prompt that never calls a model runs to its result;
  - a prompt with no `models:` block and no `models.default` call runs `models.infer` against a Host with one `default` model, `models.use('strong')` works in it, and with `{ default: A, strong: B }` the `models.use('strong')` round runs on B and the `models.use('fast')` round runs on A;
  - usage attribution: with `{ default: A, strong: B }`, a `models.use('strong')` round's request names B's model name and the `model` field of that round's events is B's model name. The test pins this and does not assume it, because whether `Completion::model()` falls back to the requested model when a backend omits the name was not established;
  - a prompt with no `models:` block runs end to end through the Harness against a one-model Host, in the rewritten fixtures of `crates/harness/tests/suite` and `crates/harness-internal/runner/tests/it`.
- Rules: nothing you write (code, comments, docs, error messages, test names, prompts, fixtures, README) says "replaces", "formerly", "previously", "no longer", "legacy", "removed" or "migrated", or notes what a name used to be, and no test asserts that a deleted name is gone. Rewrite prompts, fixtures and docs in place with no "was X, now Y" notes. Do not edit `vibe/` or `.vibelight/`.

</step-2>

<step-3>

### Step 3: Immutable model set with the default role as baseline

- Component: Fixed roles and Host map
- Tests:
  - `cargo nextest run --locked -p runvil-model-client -p runvil-lua -p runvil-engine -p runvil -p harness-runner -p harness --all-features`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy --locked -p runvil-model-client -p runvil-lua -p runvil-engine --all-targets --all-features`
  - `cargo +nightly-2026-09-05 xtask api --check`
  - `rg -n "models\.default|default_alias|ModelView|ModelSetError|resolve_model_binding|ModelSelection" crates README.md prompts` (a pass prints nothing and exits with code 1)
  - `git add -N .; git diff -U0 HEAD | rg -n -i "^\+.*(no longer|formerly|previously|legacy|deprecat|backward|replaces|migrat)"` (a pass prints nothing and exits with code 1)

Make the run's model set immutable, and make the `default` role the baseline a section starts on. The Lua selection now holds a whole `ModelBinding`, and the Lua `models.default` call and everything that existed only to support it are deleted.

- Placement: second step of the "Host map to model set" piece, built after step 2 because it reads the five role bindings that step puts in the set, and before step 4 because `models.current` and `models.use(handle)` read and write the selection this step creates.
- Build state: the workspace builds and passes after this step. The facade's public API listing does not change. Each section VM still gets a fresh selection, so a switch lasts for the rest of its section only. Step 6 makes the chain own it.
- Behavior:
  - `ModelSet` has no default alias and no writer, so it is built once at prepare and shared as `Arc<ModelSet>`. `ModelView`, `ModelSetError`, the poisoned-lock helper and the run-wide mutex are deleted.
  - Each section VM's injection takes the selection as an argument, of type `Arc<Mutex<Option<ModelBinding>>>`. The engine builds it per section as `Arc::new(Mutex::new(ctx.default_binding()))`, so a section starts on the `default` binding, or on `None` when the caller's map was empty.
  - `models.use(label, options?)` accepts a role name. It looks the role up in the set and stores `options.apply(binding)` as the selection, then returns the handle. The option checks and their error texts stay as they are (for example `models.use option temperature must be a number, got string`).
  - `models.get(label)` returns the handle for a role without selecting it. For a label that is not one of the five roles it resolves a raw gateway model id when the run has an application-state snapshot (`RunContext::with_ui`), as it does today with the 8192-token fallback context, and otherwise fails.
  - Error texts: a label outside the five fails with `models.use label "reviewer" is not a model role: expected default, strong, small, fast or creative`, and `models.get label "reviewer" ...` with the same tail. A role with no model (the caller's map was empty) fails with an error that contains `the Host supplied no model`, for `models.use` and `models.get`.
  - The engine reads the current model for inference, chat rounds and a section's first tool call through the new `SectionVm::current_model(&self) -> Result<Option<ModelBinding>>`. It returns an `Error::internal` with `model selection lock was poisoned` on a poisoned lock.
- Artifacts (model-client):
  - `crates/runvil-internal/model-client/src/model/options.rs`: `ModelSet` keeps `bindings()`, `binding(alias)` and `push`. Delete its `default` field, `default_alias()` and `set_default_alias()`, plus `ModelSetError`, `ModelView`, `lock_model_set` and the `ModelView` impl for `Mutex<ModelSet>`. Rewrite the `ModelSet` doc to say it is built once at prepare and shared.
  - `crates/runvil-internal/model-client/src/model.rs` and `lib.rs`: drop `ModelView` and `ModelSetError` from the re-exports and docs.
  - Tests: `options-tests.rs` loses the poisoned-set tests, and `model/tests.rs` rewrites the test named for growing by push and taking a default alias so it covers push and lookup only.
- Artifacts (Lua, `crates/runvil-internal/lua/src`):
  - `models.rs`: delete `ModelSelection`, `lock_models` and the `models.default` function. `install_models(lua, globals, set: &Arc<ModelSet>, selection: &Arc<Mutex<Option<ModelBinding>>>, raw_ids)`. Validate a label against `ModelRole::ALL` (the crate already depends on `runvil-types`) instead of the alias grammar. Fix the module doc.
  - `vm.rs` and `vm/inject.rs`: `bound_models: Arc<ModelSet>`. The VM no longer creates a selection of its own. Injection takes it, and the `model_selection` field and accessor go.
  - `vm/state.rs`: delete `resolve_model_binding` and add `SectionVm::current_model`. Update the exports in `lib.rs`, the `From<ModelSetError>` impl in `error.rs`, and the helpers in `test_support.rs` (`model_bag_handles`) and `test_vm.rs` (`run_injection`).
  - `models-tests.rs`: rewrite for the new signatures.
- Artifacts (engine, `crates/runvil-internal/engine/src`):
  - `execute/context.rs`: `RunState` holds `Arc<ModelSet>`, `models()` returns `&ModelSet`, and add `RunState::default_binding(&self) -> Option<ModelBinding>`, a clone of the `default` binding. Delete the `ModelView` use.
  - `execute/section_context-construct.rs` (it passes the model set to the VM) and `execute/section_vm.rs`: create the per-section selection as above and pass it to injection.
  - `execute/scheduler/chat.rs`, `execute/scheduler/dispatch.rs` and `execute/section_context.rs`: call `vm.current_model()` where they called `resolve_model_binding(ctx.models(), vm.model_selection())`. Update the re-exports in `lua.rs` and `model.rs`.
  - `execute/fill.rs`: remove the `set_default_alias` call that step 2 added.
  - Tests that build a `ModelSet` by hand with a default alias push a binding labelled `default` instead: `execute/tests/scheduler.rs`, `execute/tests/models_loop.rs`, `execute/tests/precheck_anchor.rs`, `execute/tests/models_loop-author-shapes.rs`, `execute/tests/scheduler/failures-script-tools.rs`, `model/tests.rs`, `model/tests-always.rs`, `error/tests.rs`. Find any other with `rg -n "default_alias|ModelView|ModelSelection|resolve_model_binding" crates`.
- Test cases (Lua unit tests in `models-tests.rs`, engine tests through `Scenario` with the map `{ default: A, strong: B }`):
  - a section that never calls `models.use` runs `models.infer` on A, the `default` role's model;
  - `models.use('strong')` makes the next `models.infer` run on B, and `models.use('fast')` runs it on A;
  - `models.use('strong', { temperature = 0.2, max_tokens = 512 })` applies both options to the next round, and the existing option-validation tests keep their texts;
  - `models.use('reviewer')` fails with `models.use label "reviewer" is not a model role: expected default, strong, small, fast or creative`, and `models.get('reviewer')` with no UI snapshot fails with the `models.get` form of the same text;
  - with an empty caller map, `models.use('default')` and `models.get('default')` fail with an error containing `the Host supplied no model`, `models.infer` raises `ModelRequired`, and a prompt that never calls a model still runs;
  - with a UI snapshot, `models.get('vendor/raw-model')` resolves a raw gateway model id with the 8192-token fallback context;
  - `SectionVm::current_model()` returns the `default` binding on a new section VM and the selected binding after `models.use('strong')`.
- Rules: nothing you write (code, comments, docs, error messages, test names) says "replaces", "formerly", "previously", "no longer", "legacy", "removed" or "migrated", or notes what a name used to be, and no test asserts that a deleted name is gone. Do not edit `vibe/` or `.vibelight/`.

</step-3>

<step-4>

### Step 4: models.use with a handle, models.current, handle fields

- Component: Fixed roles and Host map
- Tests:
  - `cargo nextest run --locked -p runvil-model-client -p runvil-lua -p runvil-engine -p runvil -p harness-runner -p harness --all-features`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy --locked -p runvil-model-client -p runvil-lua -p runvil-engine -p runvil --all-targets --all-features`
  - `cargo +nightly-2026-09-05 xtask api --check` (after `--bless`)
  - `rg -n "capabilities" crates/runvil-internal/model-client/src crates/runvil-internal/lua/src/models.rs crates/runvil-internal/lua/src/models-userdata.rs crates/runvil-internal/engine/src/execute/fill.rs` (a pass prints nothing and exits with code 1)
  - `rg -n "with_capabilities" crates README.md prompts` (a pass prints nothing and exits with code 1)
  - `git add -N .; git diff -U0 HEAD | rg -n -i "^\+.*(no longer|formerly|previously|legacy|deprecat|backward|replaces|migrat)"` (a pass prints nothing and exits with code 1)

Finish the author-facing Lua surface for model roles: `models.current()`, `models.use` accepting a handle, and a handle without the keyword-set field.

- Placement: last step of the "Host map to model set" piece. It adds behavior on top of the selection that step 3 made hold a whole binding, so it comes after it. It stays out of the front matter piece because nothing here reads the parser.
- Build state: the workspace builds and passes after this step. The facade listing changes (`ModelBinding::capabilities` and `with_capabilities` go), so regenerate `crates/runvil/public-api.txt` with `cargo +nightly-2026-09-05 xtask api --bless`.
- Behavior:
  - `models.current()` returns the handle for the section's current selection, which starts on the `default` role. With an empty caller map it fails with an error containing `the Host supplied no model`.
  - `models.use(role_or_handle, options?)` returns the handle. Given a handle (from `models.current()`, `models.get(...)`, or an earlier `models.use`, including a raw-id handle) it stores `options.apply(handle's binding)` as the selection, so options apply on top of the handle. Any other argument type fails with `models.use takes a model role name or a model handle, got number` (the Lua type name replaces `number`).
  - The handle exposes `label`, `name`, `model_id`, `description`, `context`, `thinking` and `provider`, plus the methods `infer` and `loop`. The `capabilities` field goes. Leave the duplicate `name` and `label` fields and the handle methods as they are.
  - A handle's description is the model info's description, because a role has no description of its own.
- Artifacts:
  - `crates/runvil-internal/lua/src/models.rs`: `models.current` and the handle branch of `models.use`, using `is_handle` and `LuaModelHandle` from `models-userdata.rs`.
  - `crates/runvil-internal/lua/src/models-userdata.rs`: delete the `capabilities` field and its method.
  - `crates/runvil-internal/model-client/src/model/options.rs`: delete the `capabilities` field of `ModelBinding`, `ModelBinding::capabilities()` and `with_capabilities`, and fix the docs. Update `options-tests.rs` and `model/tests.rs`.
  - `crates/runvil-internal/engine/src/execute/fill.rs`: stop passing the declared keywords and description into the binding. The binding's description is `info.description()`.
  - Update the tests and fixtures that assert `capabilities` or a declared role's `description` to assert the model info's description instead, find them with `rg -n "capabilities" crates`.
- Test cases (`crates/runvil-internal/lua/src/models-tests.rs` and engine tests with the map `{ default: A, strong: B }`):
  - `models.current()` returns a handle with label `default` before any `models.use`, and after `models.use('strong')` returns a handle with label `strong` and B's `model_id`;
  - `local prev = models.current(); models.use('strong'); models.use(prev)` puts the section back on A, so the next `models.infer` request names A;
  - `models.use(prev, { temperature = 0.2 })` applies the temperature on top of the handle's binding, so the next request names the handle's model and its `options.temperature()` is `Some` of 0.2;
  - with a UI snapshot, a raw-id handle from `models.get('vendor/raw-model')` is accepted by `models.use`, and the next `models.infer` request names `vendor/raw-model`'s gateway model name;
  - `models.use(5)` fails with `models.use takes a model role name or a model handle, got number`;
  - with an empty caller map, `models.current()` fails with an error containing `the Host supplied no model`;
  - a handle exposes `label`, `model_id`, `description`, `context`, `thinking` and `provider`, and its `infer` and `loop` methods keep working in the existing tests.
- Rules: nothing you write (code, comments, docs, error messages, test names) says "replaces", "formerly", "previously", "no longer", "legacy", "removed" or "migrated", or notes what a name used to be, and no test asserts that a deleted name is gone. Do not edit `vibe/` or `.vibelight/`.

</step-4>

<step-5>

### Step 5: Requirements-only front matter, Workshop contract and UI, then the commit gate

- Component: Fixed roles and Host map
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop-server -p workshop-server-api`
  - `node --test crates/workshop/ui/test/run-api.mjs crates/workshop/ui/test/run-panel.mjs`
  - `npm test --workspace ui --prefix crates/workshop`
  - `npm run typecheck --workspace ui --prefix crates/workshop`
  - `npm run build --workspace ui --prefix crates/workshop`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy --locked -p workshop-server -p workshop-server-api --all-targets`
  - `$env:RUSTDOCFLAGS = '-D warnings'; cargo doc --locked --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `cargo +nightly-2026-09-05 xtask api --check` (after `--bless`)
  - `rg -n "keywords" crates/workshop/ui/src/services/run-api.ts crates/workshop/ui/src/parts/run crates/workshop/ui/test/run-api.mjs crates/workshop/ui/test/run-panel.mjs` (a pass prints nothing and exits with code 1)
  - `rg -n "keywords:" crates README.md prompts --glob "!crates/workshop/ui/**"` (a pass prints nothing and exits with code 1)
  - `rg -n "models\.default|models\.bind|default_alias|ModelView|ModelSetError|ModelBindings|model_bindings|ModelRoles|ModelRolesIter|ModelKeyword|keyword_name|keyword_wire|with_capabilities|HardKeyword|resolve_model_binding|trivial fill|bound model role" crates README.md prompts` (a pass prints nothing and exits with code 1)
  - `rg -n "ModelRole|RoleRequirements|role_requirements|RequirementCheck::(Thinking|Unbound)|fn prepare|prepare\(self" crates/runvil/public-api.txt` (a pass prints lines for the new facade surface: the `ModelRole` enum with `ALL` and `as_str`, `RoleRequirements` with `min_context` and `thinking`, `Frontmatter::role_requirements`, the `Thinking` and `Unbound` checks, `UnmetRequirement::role` as `ModelRole`, and `RunContext::prepare` taking `BTreeMap<ModelRole, ModelInfo>`)
  - `git add -N .; git diff -U0 HEAD | rg -n -i "^\+.*(no longer|formerly|previously|legacy|deprecat|backward|replaces|migrat)"` (a pass prints nothing and exits with code 1)

Make `models:` front matter optional and limited to requirements on the five roles. Give the Engine, the Workshop server and the Workshop UI the new reader. This is the last step of the first component, so its tests include the component's whole exit gate for the `runvil` repository.

- Placement: this piece, "Requirements-only front matter", comes after the Host map piece because the requirement checks now read the parser's new accessor and the fixtures already use role names. The parser, the engine reader, the Workshop server and the Workshop UI are one step. Removing the parser's old types stops the engine and the server from compiling until they read the new accessor, and once the server sends the new JSON the UI's reader returns `null` for every model row, so a commit that left the UI behind would leave the run panel without its model rows. The design document is step 8, written after all the code, so it describes the final surface.
- Build state: the workspace and the UI build and pass after this step, and the whole gate above passes. The facade listing changes, so regenerate `crates/runvil/public-api.txt` with `cargo +nightly-2026-09-05 xtask api --bless`. If a gate command fails because of an earlier step's leftover (a deleted name still present, a doc link, a listing difference), fix it here.
- Behavior:
  - Front matter `models:` maps role names to `{ min_context, thinking }`. `min_context` is a positive token count and `thinking` is a boolean. Both are optional, and `fast: {}` is valid and adds nothing. A prompt with no `models:` key is valid.
  - Parse errors: an unknown role name (`reviewer`) names the five roles `default`, `strong`, `small`, `fast` and `creative` (the serde unknown-variant error for `ModelRole`); an unknown field (`tier`) fails with ``unknown field `tier`, expected `min_context` or `thinking` ``; a duplicate role key fails; `min_context: 0` fails.
  - `Frontmatter::role_requirements(role: ModelRole) -> Option<&RoleRequirements>` is the only accessor. It is `None` when the prompt does not declare the role, and `Some` with neither field set for `role: {}`.
  - `thinking: true` requires a model that can think and turns thinking on for that role's requests. `thinking: false` requires a model that can run without thinking and turns it off. A role's requirements are checked against the model that fills the role, fallbacks included.
  - The Workshop server sends each declared role as `{ label, min_context, thinking }`, where `min_context` is a number or `null` and `thinking` is a boolean or `null`. The run panel shows one read-only row per declared role: the role label, then its requirements as text, built from the parts that are set and joined with `, `: `min context N` when `min_context` is set, `thinking` when `thinking` is `true`, `no thinking` when it is `false`. A role with no requirements shows its label with empty details.
- Artifacts (parser, `crates/runvil-internal/parser/src`):
  - `contract/models.rs`: rename the struct `ModelRole` to `RoleRequirements { min_context: Option<NonZeroU32>, thinking: Option<bool> }` with `#[serde(deny_unknown_fields)]` and accessors `min_context()` and `thinking()`. Delete `ModelKeyword`, `ModelRoles`, `ModelRolesIter`, and the struct's `keywords` and `description`. The `models` key deserializes into a `BTreeMap<ModelRole, RoleRequirements>` (`ModelRole` from `runvil_types::models`). Rewrite the module doc.
  - `contract.rs`: make the shared contract-map visitor (`deserialize_contract_map` and its `ContractKeys`) generic over the key type. `args` keeps its alias-grammar string keys, `models` uses `ModelRole`, and the duplicate-key rejection stays shared.
  - `frontmatter.rs`: replace `models()` with `role_requirements`. `detail.rs`: delete `keyword_name`. `lib.rs`: update the exports (`RoleRequirements` in, `ModelKeyword`, `ModelRole`, `ModelRoles`, `ModelRolesIter` out). Update the re-export in `crates/runvil-internal/engine/src/parser.rs`.
  - `contract/tests.rs`: rewrite the model-role tests for the new surface.
- Artifacts (engine and facade):
  - `crates/runvil-internal/engine/src/execute/fill.rs`: read each role's requirements with `prompt.frontmatter().role_requirements(role)`. `min_context` gives `ContextMinimum` as before. `thinking: Some(true)` against `ThinkingMode::Never` gives `Thinking` with required `thinking: true` and actual `Never`, and `thinking: Some(false)` against `Always` gives required `thinking: false` and actual `Always`. A declared `thinking` sets `ModelInvocation::new().with_thinking(requirements.thinking())` on that role's binding only. Delete the uses of `ModelKeyword` and `detail::keyword_name`.
  - `crates/runvil/src/lib.rs` (the `prompt` module): export `RoleRequirements` and drop `ModelKeyword`, `ModelRole`, `ModelRoles`, `ModelRolesIter`. Regenerate `crates/runvil/public-api.txt`. Callers of `Frontmatter::models()` in `crates/runvil/tests/suite/prompt.rs` and the engine's `test_support` move to `role_requirements`.
- Artifacts (Workshop server, `crates/workshop/server/src/routes/prompts.rs` and `prompts-tests.rs`): `ModelDto` becomes `{ label: String, min_context: Option<u32>, thinking: Option<bool> }`, built for every role in `ModelRole::ALL` order that `role_requirements` returns `Some` for, with `label` set to `role.as_str()`. A missing value serializes as `null`. Delete `keyword_wire`.
- Artifacts (Workshop UI):
  - `crates/workshop/ui/src/services/run-api.ts`: `RunContractModel` becomes `{ label: string; minContext: number | null; thinking: boolean | null }`. Its reader checks `label` is a string, `min_context` is `null` or a number, and `thinking` is `null` or a boolean, and returns `null` for any other shape as the current reader does.
  - `crates/workshop/ui/src/parts/run/run-rows.ts`: build each model row's details from `minContext` and `thinking` as described above, and drop the keyword and description text.
  - `crates/workshop/ui/test/run-api.mjs` and `crates/workshop/ui/test/run-panel.mjs`: rewrite the fixtures to `{ label: "strong", min_context: 32000, thinking: true }` and rename the test titled "a model role renders keywords and min_context read-only" to say it renders min_context and thinking read-only.
- Artifacts (fixtures and prompts): every `keywords: [thinking]` becomes `thinking: true` and `keywords: [no-thinking]` becomes `thinking: false`. Every remaining soft keyword and role `description:` is deleted. Find them with `rg -n "keywords:|description:" crates prompts README.md` and review each hit inside a `models:` block. Do this in `crates/runvil-internal/engine/src/execute/tests/suite/prepare.rs`, `crates/runvil/tests/suite/prepare.rs`, `prompts/*.md`, `crates/workshop/agents/agents/chat.md` and any other fixture the search finds. The UI files are covered by the Workshop UI artifacts above.
- Test cases:
  - Parser: a prompt with no `models:` key has `role_requirements` of `None` for all five roles; each role declared as `{ min_context: 4096, thinking: true }` reads back both values; `models: { fast: {} }` parses and gives `Some` with neither value; `models: { reviewer: {} }` fails and the error text names all five roles; `models: { strong: { tier: 1 } }` fails with the unknown-field text above; a duplicate role key fails; `min_context: 0` fails; `args` keys are still checked against the alias grammar (keep the existing `args` tests).
  - Engine: `models: { strong: { thinking: true } }` against `ThinkingMode::Never` and `models: { small: { thinking: false } }` against `ThinkingMode::Always` are each refused, with the entries and notice lines from step 2; `thinking: false` on `small` makes a `models.use('small')` round's request have `thinking()` of `Some(false)`, and `thinking: true` on `strong` gives `Some(true)` for `strong` rounds only; `min_context` on a role with no entry of its own is checked against the `default` model.
  - Workshop server: the prompt-contract JSON for `models: { strong: { min_context: 200000, thinking: true }, fast: {} }` has `models` equal to `[{ "label": "strong", "min_context": 200000, "thinking": true }, { "label": "fast", "min_context": null, "thinking": null }]`, and a prompt with no `models:` has `models` equal to `[]`.
  - Workshop UI: the reader accepts `{ label: "strong", min_context: 32000, thinking: true }` and gives `minContext` 32000 and `thinking` `true`; it accepts `{ label: "fast", min_context: null, thinking: null }` and gives `null` for both; it returns `null` for an entry whose `thinking` is a string; the run panel row for `{ label: "strong", min_context: 32000, thinking: true }` reads `min context 32000, thinking`, the row for `thinking: false` with no `min_context` reads `no thinking`, and a role with neither shows only its label.
- Rules: nothing you write (code, comments, docs, error messages, test names, prompts, fixtures) says "replaces", "formerly", "previously", "no longer", "legacy", "removed" or "migrated", or notes what a name used to be, and no test asserts that a deleted name is gone. Do not edit `vibe/` or `.vibelight/`.

</step-5>

<step-6>

### Step 6: Chains own the current model and children inherit it, then the commit gate

- Component: Chain-level current model
- Tests:
  - `cargo nextest run --locked -p runvil-engine --all-features chain_model`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy --locked -p workshop-server -p workshop-server-api --all-targets`
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop-server -p workshop-server-api`
  - `cargo +nightly-2026-09-05 xtask api --check`
  - `rg -n "models\.default|models\.bind|default_alias|ModelView|ModelSetError|ModelBindings|model_bindings|ModelRoles|ModelRolesIter|ModelKeyword|keyword_name|keyword_wire|with_capabilities|HardKeyword|resolve_model_binding|trivial fill|bound model role" crates README.md prompts` (a pass prints nothing and exits with code 1)
  - `git add -N .; git diff -U0 HEAD | rg -n -i "^\+.*(no longer|formerly|previously|legacy|deprecat|backward|replaces|migrat)"` (a pass prints nothing and exits with code 1)

Make each chain own the selection that its sections' VMs share, so a switch lasts across the chain's sections, and start every chain from the right value: a child from a copy of its parent's current model taken at dispatch, and the root walk from the model the H1 pass ended on. This is the last code step of the second component, so its tests include the component's exit gate for the `runvil` repository.

- Placement: this component comes second because it needs the immutable model set, the `default` baseline and the selection argument from the first component (step 3), and the third component needs the copy at dispatch that this step adds. It is one piece in one step: ownership and inheritance share the `ChainStart` field and the same tests, and a split would need an in-between commit where every child starts on `default`, which no behavior in the plan wants.
- Build state: the workspace builds and passes after this step, and the gate above passes. The facade listing does not change.
- Behavior:
  - A chain is the engine's unit of sequential execution: the H1 pass, the root walk, a `call` child, or a spawned task (a fanout arm is a spawned task). A section VM is the Lua VM built for one section.
  - Each chain holds one `Arc<Mutex<Option<ModelBinding>>>`. It is created when the chain starts, from the chain's starting value, and every section VM of that chain gets that same `Arc` at injection. So `models.use` in one section is still in effect in the next section of the same chain, and no two chains share a selection.
  - The starting value is the `default` binding for the H1 pass and for the root walk of a prompt that has no H1 blocks. It is `None` when the caller's map was empty, because `ctx.default_binding()` is `None` then.
  - A `call` child and a `tasks.spawn` task (so each fanout arm) start with a copy of the parent chain's current model, taken when the call or spawn is dispatched. A spawned task waits on the ready queue with no VM until the scheduler admits it, so a copy taken at admission would let a switch the parent makes after spawning change the task's model. Copying at dispatch keeps a run's result independent of when the scheduler admits a queued task.
  - The root walk that follows H1 starts with the model the H1 pass ended on. This holds for the walk that starts when H1 ends normally and for the walk that starts when a jump leaves H1.
  - A child's switch never reaches its parent or its siblings, because each chain's selection is its own and inheritance copies a value.
  - The `sys.model` value a section reads at its first tool call is the chain's current model at that moment.
- Artifacts (`crates/runvil-internal/engine/src/execute`):
  - `scheduler/chain.rs`: add `model: Option<ModelBinding>` to `ChainStart` (the starting value) and a `selection: Arc<Mutex<Option<ModelBinding>>>` field to `Chain`, created in `start_chain` from `start.model`. Add `Chain::current_model(&self) -> Result<Option<ModelBinding>>`, which clones the value under the lock and returns an `Error::internal` with `model selection lock was poisoned` on a poisoned lock. Add a `model: Option<ModelBinding>` parameter to `start_root_chain` and pass it into the `ChainStart` it builds, and update the reason on its `#[expect(clippy::too_many_arguments)]` so it names the five values a root start forwards.
  - `scheduler/dispatch.rs` (`prepare_call`, which builds the call child's `ChainStart`) and `scheduler/tasks-spawn.rs` (the spawn dispatch that builds the task's `ChainStart`): set `model` to `chain.current_model()?` on the parent chain, read at that moment and before `start_chain` runs.
  - `scheduler/h1.rs`: `start_h1` passes `self.ctx.default_binding()` to `start_root_chain`. `end_h1` passes `chain.current_model()?` of the H1 chain to the root walk it starts. `end_h1_at_jump` calls `end_h1`, so this one site covers the jump too. `scheduler/walk.rs`: `start_root_walk` passes `self.ctx.default_binding()`.
  - `scheduler/test_hooks.rs`: its two `ChainStart` sites (the wedge chain and the probe chain) set `model: self.ctx.default_binding()`.
  - `section_vm.rs` and `section_context-construct.rs` (whichever builds the section VM): take the chain's selection `Arc` as a parameter and pass it to injection, in place of the fresh selection step 3 creates per section. The scheduler's callers pass `&chain.selection`.
- Test cases (new file `crates/runvil-internal/engine/src/execute/tests/scheduler/chain_model.rs`, wired into `scheduler.rs`; `Scenario` with the map `{ default: A, strong: B, fast: C }`, each case reading `models.current().label` or `model_id`):
  - a prompt whose `## One` runs `models.use('strong')` and whose `## Two` returns `models.current().label` returns `strong`, and a `models.infer` in `## Two` makes a request that names B;
  - a prompt that never calls `models.use` returns `default` from `models.current().label` in every section;
  - the `sys.model` value read at the first tool call of `## Two` reflects the `strong` switch made in `## One`;
  - a `call` child inherits the parent's `strong` switch, and a spawned task inherits it;
  - a child's `models.use('fast')` does not reach the parent (the parent's next `models.current().label` is still `strong`) or a sibling task;
  - a `models.use('strong')` made in H1 is the starting model of the root walk, both when H1 ends normally and when a jump leaves H1;
  - a parent that spawns two tasks under `tasks.concurrency(1)`, then switches to `fast` before the second, queued task is admitted, leaves that task on the model the parent had when it spawned;
  - all arms of one 2-argument `tasks.fanout` start on the same model, with the concurrency limit set to 1 so later arms are queued;
  - audit every existing test that assumed a fresh selection per section: find them with `rg -n "models\.use" crates/runvil-internal/engine/src` and fix any test whose expectation depended on a switch ending with its section.
- Rules: nothing you write (code, comments, docs, error messages, test names) says "replaces", "formerly", "previously", "no longer", "legacy", "removed" or "migrated", or notes what a name used to be, and no test asserts that a deleted name is gone. Do not edit `vibe/` or `.vibelight/`.

</step-6>

<step-7>

### Step 7: Model-first tasks.fanout, then the commit gate

- Component: Model-first fanout
- Tests:
  - `cargo nextest run --locked -p runvil-engine -p runvil-lua --all-features fanout_model`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy --locked -p workshop-server -p workshop-server-api --all-targets`
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop-server -p workshop-server-api`
  - `cargo +nightly-2026-09-05 xtask api --check`
  - `rg -n "models\.default|models\.bind|default_alias|ModelView|ModelSetError|ModelBindings|model_bindings|ModelRoles|ModelRolesIter|ModelKeyword|keyword_name|keyword_wire|with_capabilities|HardKeyword|resolve_model_binding|trivial fill|bound model role" crates README.md prompts` (a pass prints nothing and exits with code 1)
  - `git add -N .; git diff -U0 HEAD | rg -n -i "^\+.*(no longer|formerly|previously|legacy|deprecat|backward|replaces|migrat)"` (a pass prints nothing and exits with code 1)

Add the model-first form `tasks.fanout(model, worker, collection)`, implemented entirely in the Lua fanout shim. This is the third component's only code step, so its tests include the component's exit gate for the `runvil` repository.

- Placement: this component is last because the shim relies on the second: spawned arms copy the parent's selection at dispatch, so setting the parent's selection before spawning sets every arm's model. It is a single piece with a single code step, because the shim, its privileged captures and its tests are one behavior that one set of tests covers.
- Build state: the workspace builds and passes after this step, and the gate above passes. The scheduler, the spawn request and every protocol type stay as they are.
- Behavior:
  - `tasks.fanout(worker, collection)` is unchanged: arms inherit the chain's current model.
  - `tasks.fanout(model, worker, collection)` takes a model handle first. The shim reads the parent's current selection, sets the selection to the handle's binding, spawns every arm, and restores the saved selection before the first join and also before it re-raises a spawn failure. The parent's model after the call is the model it had before. No author code runs in the parent between the switch and the restore, and `models.use` emits no event, so the switch cannot be observed.
  - Inside an arm, `models.use('strong')` affects only that arm and its own children, and `models.use('default')` selects the `default` role, not the model the fanout was given.
  - Errors: a first argument that is not a model handle (including `nil`) fails with `tasks.fanout: the first argument must be a model handle, got nil` (the Lua type name replaces `nil`), and any argument count other than 2 or 3 fails with `tasks.fanout takes (worker, collection) or (model, worker, collection)`.
- Artifacts (`crates/runvil-internal/lua/src`):
  - `__impl_fanout.lua`: handle the 3-argument form with the switch, spawn-all and restore above. Use the privileged captures the shim receives at install: `get_selection()` returns the chain's current handle or `nil`, `set_selection(handle_or_nil)` sets the selection (`nil` clears it) and fails for any other value, and `is_model_handle(value)`. Reject a non-handle first argument before switching.
  - `coro.rs`: install the three captures when the shim is installed, over the selection `Arc<Mutex<Option<ModelBinding>>>` that injection received (the calling chain's own since step 6) and `LuaModelHandle`. They are passed to the shim chunk and are not reachable from author code.
- Test cases (new file `crates/runvil-internal/engine/src/execute/tests/scheduler/fanout_model.rs`, wired into `scheduler.rs`; `Scenario` with the map `{ default: A, strong: B, fast: C }`, and Lua unit tests in `crates/runvil-internal/lua/src` for the captures):
  - a 3-argument fanout with `models.get('fast')` runs every arm on C: each arm returns `models.current().model_id` and every result is C's;
  - after the call the parent's model is what it was before: a parent on `strong` reads `strong` from `models.current().label` after the fanout;
  - a spawn failure part-way still restores the parent's model: trigger the depth-cap refusal the shim already re-raises (see the comment above `fanout` in `__impl_fanout.lua`), catch it with `pcall`, and check that `models.current().label` is the label before the call;
  - a raw-id handle works with an empty caller map: with a UI snapshot set and no models in the map, `tasks.fanout(models.get('vendor/raw-model'), worker, items)` runs every arm on that gateway model;
  - a first argument that is not a handle (a string, a number, and `nil`) fails with the text above, and an argument count other than 2 or 3 fails with the second error text;
  - `models.use('default')` inside an arm selects the `default` role, and `models.use('strong')` inside an arm leaves the parent's and the other arms' models alone;
  - the 2-argument form still gives every arm the parent's current model.
- Rules: nothing you write (code, comments, docs, error messages, test names) says "replaces", "formerly", "previously", "no longer", "legacy", "removed" or "migrated", or notes what a name used to be, and no test asserts that a deleted name is gone. Do not edit `vibe/` or `.vibelight/`.

</step-7>

<step-8>

### Step 8: Design document for roles, the chain's current model and fanout

- Component: Model-first fanout
- Tests:
  - Run from the `runvil-design` repository, the sibling of `runvil`: `rg -n "models\.default|models\.bind|default_alias|ModelView|ModelSetError|ModelBindings|model_bindings|ModelRoles|ModelRolesIter|ModelKeyword|keyword_name|keyword_wire|with_capabilities|HardKeyword|resolve_model_binding|trivial fill|bound model role" design` (a pass prints nothing and exits with code 1)
  - Run from the `runvil-design` repository: `git add -N .; git diff -U0 HEAD | rg -n -i "^\+.*(no longer|formerly|previously|legacy|deprecat|backward|replaces|migrat)"` (a pass prints nothing and exits with code 1)

Rewrite the models part and the task descriptions of `design/architect-runvil-prompting-language.md` in the `runvil-design` repository to describe the behavior steps 1 to 7 ship. This step's commit goes in `runvil-design`, not in `runvil`.

- Placement: the last step of the plan. It is written once, after all the code, so the document describes the finished surface of the three components and not three partial states. It is its own step because it is a commit in a different repository. It is filed under the third component, which finished its code at step 7, so the plan keeps three components.
- Build state: documentation only. Nothing in `runvil` changes.
- Behavior to document (present tense, no history):
  - Every prompt has exactly five roles: `default`, `strong`, `small`, `fast`, `creative`. There are no custom roles. Front matter `models:` is optional and holds only requirements per role, `min_context` (a positive token count) and `thinking` (`true` requires a model that can think and turns thinking on for that role's requests, `false` requires one that can run without thinking and turns it off). An unknown role name or field is a parse error.
  - The Host passes a role-to-model map to `prepare`. A role with no entry uses the `default` entry. An empty map means the Host has no model, and a prompt fails only when it calls a model. A non-empty map without `default` is refused before the run starts. Requirements are checked against the model that fills the role, fallbacks included.
  - The Lua surface: a chain starts on the `default` role. `models.use(role_or_handle, options?)` switches the chain's current model and returns the handle, and the switch lasts across that chain's sections until the next switch. `models.current()` returns the current handle, `models.get(role)` inspects without switching, and `models.infer` and `models.loop` run on the current model or an explicit handle. A handle exposes `label`, `model_id`, `description`, `context`, `thinking` and `provider`, plus `infer` and `loop`. Error texts: `models.use label "reviewer" is not a model role: expected default, strong, small, fast or creative`, and `the Host supplied no model`.
  - `call` children, `tasks.spawn` tasks and fanout arms start with a copy of the parent's current model, taken when the call or spawn is dispatched. The root walk starts with the model the H1 pass ended on. A child's switch never reaches its parent or its siblings.
  - The save-and-restore pattern is `local prev = models.current(); models.use('creative'); ...; models.use(prev)`. A `models.use('strong')` for one expensive step leaves every later section of that chain on `strong`, which is a cost to plan for, and the save-and-restore pattern undoes it.
  - `tasks.fanout(worker, items)` runs every arm on the chain's current model. `tasks.fanout(model, worker, items)` takes a model handle first and runs every arm on it, without switching the parent: the parent's model after the call is what it was before, including when a spawn fails part-way. Example: `tasks.fanout(models.get('fast'), worker, items)`. Inside an arm, `models.use('strong')` affects only that arm and its children, and `models.use('default')` selects the `default` role. Error texts: `tasks.fanout: the first argument must be a model handle, got nil` and `tasks.fanout takes (worker, collection) or (model, worker, collection)`. The save-and-restore example with a 2-argument fanout stays valid: `local prev = models.current(); models.use('creative'); local drafts = tasks.fanout(worker, items); models.use(prev)`.
  - Wherever the document says a switch lasts for the rest of the section, it says the chain instead.
- Artifacts: in `design/architect-runvil-prompting-language.md`, rewrite the `## Models` section, every other mention of `models.bind` and `models.default`, and the `tasks.fanout` and `tasks.spawn` descriptions and examples. Find the places with `rg -n "models\.bind|models\.default|keywords|models\.use|models\.current|selection|tasks\.fanout|tasks\.spawn" design`. The `models.bind` and `models.default` hits are in the deferred-work list near the top, the sample prompt that calls `models.default("writer")`, the `models.bind` and `models.default` headings and effects, the paragraph about the section's model selection and explicit handles, and the later examples. Rewrite each example in place to run on the `default` role or to call `models.use` with a role. Follow the document's existing heading and example style.
- Rules: nothing you write says "replaces", "formerly", "previously", "no longer", "legacy", "removed" or "migrated", or notes what a name used to be. Do not edit dated logs.

</step-8>

</execution-plan>
