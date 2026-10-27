---
name: The std Pack, Cursor's six file tools
overview: "`crates/plugin-std` exists and passes every gate: the `runvil/std` Pack offers `Read`, `Write`, `StrReplace`, `Delete`, `Glob`, and `Grep` under Cursor's names and Cursor's own definitions, validates every call against the stored schema, and answers a not-implemented stub until real file logic lands. This plan records what shipped and specifies the remaining work: fix the one schema that drifted, then replace each stub with a real operation over the call's VFS access."
todos:
  - id: name
    content: "Confirm the crate name and the two name layers: crate `plugin-std`, Pack id `std`, Lua ids `std/read` and so on, model-facing wire names `Read`, `Write`, `StrReplace`, `Delete`, `Glob`, `Grep`"
    status: completed
  - id: template
    content: "Read `plugin-web` end to end as the template for `install`, `construct`, the tool list, and the test layout"
    status: completed
  - id: schemas
    content: "Extract Cursor's six definitions into `crates/plugin-std/schemas/*.json`, embedded at build time, with `schemas/EDITS.md` recording every line changed from Cursor's text"
    status: completed
  - id: crate
    content: "Build the crate: the Pack, the argument check against the stored schema, the not-implemented stub, and the three unit test files"
    status: completed
  - id: wire
    content: "Install the Pack in Workshop's agent Host through `with_plugins` in `crates/workshop/server/src/agents.rs`"
    status: completed
  - id: grep-schema-fix
    content: "Settle whether Cursor's Grep really lacks `context` and `-n`, as the bundle report says and as this run's own tool context contradicts, then correct `schemas/grep.json` and the two defaults, with a test that pins the exact parameter set"
    status: pending
  - id: read-real
    content: "Replace the Read stub: 1-indexed offset, negative offset from the end, limit, the 100,000-character cap with its exact error, six-wide right-aligned line numbers, and the not-shown markers"
    status: pending
  - id: write-real
    content: "Replace the Write stub: one `Access::write`, parent creation through `Access::mkdir`, and the exact `Wrote contents to <path>` success text"
    status: pending
  - id: replace-real
    content: "Replace the StrReplace stub, reusing `Access::str_replace` where its single-occurrence semantics match and adding the trimmed-line fallback it lacks"
    status: pending
  - id: delete-real
    content: "Replace the Delete stub with `Access::remove`, mapping `Ok(false)` to the not-found result and refusing a directory"
    status: pending
  - id: glob-real
    content: "Replace the Glob stub with `Access::glob`, after confirming recursion, case sensitivity, and dotfile behavior against Cursor's ripgrep walk"
    status: pending
  - id: grep-real
    content: "Replace the Grep stub. Decide first whether the VFS gains a search capability or the Pack walks with `Access::glob` and reads with `Access::read_string`, because `Access` has no grep"
    status: pending
  - id: approval
    content: "Map `VfsError::ApprovalRequired` to a model-readable refusal, and later wire it to real user approval"
    status: pending
  - id: shell
    content: "Decide the shell separately, most likely a Pack over bashkit rather than a real process"
    status: pending
  - id: listdir
    content: "Decide ListDir: Cursor gates it behind `enableDeprecatedLsTool` defaulting to false and is replacing it with a proto-only V2, so the plan defers it"
    status: pending
isProject: false
---

# The std Pack: Cursor's six file tools in Runvil

<product-contract>

## Product Requirements

Runvil agents need the file tools every coding agent has. This plan adds them as one more Plugin crate beside `plugin-mcp`, `plugin-user-input`, and `plugin-web`, and it deliberately copies Cursor's surface rather than inventing one, because a model already trained on Cursor's tools then needs no learning at all.

- Problem and users:
  - Runvil has web search, a fetch tool, an ask tool, and MCP, but no way for a model to touch a file. Every Host that wants a coding agent must build that itself.
  - The users are prompt authors writing agents for Workshop, and the Hosts that run those prompts.
  - The two constraints that shape the design are the VFS and the mode policy, both of which already exist. File access must go through `runvil_vfs` so a run's mode governs it, and the model-facing names must match Cursor's so a model needs no new instructions.
- Goals:
  - One Pack, `runvil/std`, offering six tools under Cursor's own names and definitions.
  - Every call validated against the stored definition, so a bad call fails as an invalid argument rather than reaching the operation.
  - Every file operation performed through the call's VFS `Access`, so Ask and Plan mode gate it with no work in the Pack.
  - A first stage that proves the crate, the definitions, the ids, the wire names, and the Lua path before any file logic exists.
- Non-goals:
  - Shell. An in-process shell over the same filesystem is a separate Pack, so a run that wants one declares it.
  - Images and PDFs. `Read` is text only.
  - User approval for mutations. The VFS already answers `VfsError::ApprovalRequired`; wiring it to an operator flow is later work.
  - A subagent or task tool, a notebook editor, a linter reader, and Cursor's `AwaitShell`, `ReadLints`, and `EditNotebook`. This crate is the file tools and nothing else.
  - Any change to `crates/runvil/public-api.txt`. The crate is a Plugin, not part of the Engine's facade.
- Success criteria:
  - `cargo nextest run -p plugin-std` passes, and every tool's test covers both the success path and each kind of argument failure.
  - A model that knows Cursor's file tools can call every one of these by the same name with the same arguments, and gets a plain answer rather than a schema refusal.
  - Every filesystem operation reaches files through `Access`, so a run in `Mode::Ask` cannot mutate anything and a run in `Mode::Plan` mutates only markdown paths.
  - The whole workspace suite, both clippy runs, `cargo fmt --all --check`, both rustdoc gates, `cargo test -p build-xtask`, and `node --test crates/workshop/ui/test/docs-claims.mjs` pass.
- Constraints:
  - Plugin crates may depend only on `runvil-plugin`, `shared-*` crates, and outside libraries. `cargo test -p build-xtask` enforces it.
  - The crate names no async runtime. The Harness polls every effect on one task, so a call must not block while polled.
  - Every Rust file stays at or under 500 lines, enforced by `build-ceiling` from the crate's own `build.rs`.
  - Crate docs carry an `## Invariants` section, and the wording scan reads them.
  - Error messages are written for a model to read: concise, factual, and naming the parameter and the value.
- Open questions:
  - Whether Cursor's Grep really has no `context` and no `-n`. The bundle report says it has neither; this run's own tool context shows both. One of the two is stale, and the answer decides `schemas/grep.json`.
  - Whether the VFS learns to search. `Access` has no grep, so `Grep` is either a Pack-level walk or a new VFS capability with its own claim.
  - Whether `StrReplace` owns its matching or calls `Access::str_replace` and layers the fallback above it. Mixing the two puts the semantics in two places.

## Functional Specification

A prompt declares `std` in its frontmatter and its sections gain six tools. Lua calls them by id; the model calls them by Cursor's name. In this build every well-formed call succeeds with a text saying the implementation is missing, and only a malformed call fails.

- Actors and workflows:
  - A prompt declares the Pack with `uses: [std]`, which makes it required rather than merely present.
  - Lua calls `tools.call("std/read", { path = "/notes.md" })`, or offers the Pack with `tools.offer("std")` or a single tool with `tools.offer("std/read")`.
  - The model then calls the tool by its wire name, `Read`, and the Harness routes the call back to `std/read`.
  - A Host that never installs the Pack leaves `std` unresolved, and a prompt declaring it is refused before the run starts, naming the Pack.
- Inputs and outputs:
  - The six ids and their wire names are fixed: `std/read` is `Read`, `std/write` is `Write`, `std/replace` is `StrReplace`, `std/rm` is `Delete`, `std/glob` is `Glob`, `std/grep` is `Grep`.
  - Each tool's description and parameter schema come from its JSON file, embedded with `include_str!` and parsed once into a `LazyLock`.
  - A well-formed call answers this text, trusted, with the tool's wire name in both places:

    `The <name> tool is not implemented in this build. The implementation is missing and nothing in this session will change that. Calling <name> again is safe, but it will return this same result every time, whatever the arguments. Carry on without it.`

  - A call naming a tool the Pack does not offer answers `std has no tool named <segment>`.
- States and validation:
  - The Pack takes no configuration. `construct` accepts `null` or `{}` and refuses every other value with `std takes no configuration`.
  - The argument check honors only the keywords these definitions use: `type`, `properties`, `required`, `enum`, and `items`. A schema keyword it does not know is not a reason to refuse a call, because the tool's own operation decides what it can do.
  - A `null` argument list reads as an empty object, so a tool taking no parameters is callable with no arguments.
  - A parameter the schema does not declare passes the check, so the operation sees it and refuses it itself if it must.
- Errors and recovery:
  - A missing required parameter is `InvalidArguments` with the text ``std_<Name>: `<param>` is required, and the call left it out``.
  - A required parameter passed as `null` says `` ` <param>` is required, and the call passed null``.
  - A wrong type says `` `<param>` must be a JSON string, and the call passed 2``.
  - A value outside an enum says `` `<param>` must be one of "content", "files_with_matches", "count", and the call passed "lines"``.
  - Arguments that are not an object say `the arguments must be a JSON object`.
  - A definition file that fails to parse fails `construct` with a backend error naming the tool, so a broken edit surfaces at Host build time rather than as a call failure.
- Security and privacy behavior:
  - A successful stub answer is trusted, because it is first-party text that embeds no attacker-influenced data.
  - Real operation output will be untrusted wherever it embeds file content. `Read`, `Glob`, `Grep`, and `StrReplace`'s context all carry data the model did not produce, so each marks its output accordingly when the stubs are replaced.
  - Argument validation happens before any operation, so a malformed call cannot reach the filesystem at all.
- Acceptance criteria:
  - Every one of the six tools appears in `Pack::tools()` under the Pack id `construct` receives, with its wire name set.
  - A well-formed call of each tool answers the not-implemented text naming that tool, as a successful `ToolOutput`.
  - Each kind of malformed call fails with `InvalidArguments` and a message naming the parameter.
  - `install` inserts exactly one Pack, `runvil/std`, with the config under the `std` key, and `PackRegistry` refuses a second insert.

</product-contract>
<implementation-contract>

## Technical Design

The crate sits beside the other three Plugins and reuses their shape exactly: a `PackInfo` constant, an `install` that inserts it, and a `construct` that builds one shared object holding the tool list. Everything novel is in how the definitions are stored and checked.

- Architecture:
  - `src/lib.rs` holds the crate docs, the module wiring, the Lua-id-to-wire-name table, and the `## Invariants` section the wording scan requires.
  - `src/spec.rs` embeds the six JSON files with `include_str!`, parses them once into a `LazyLock<Result<Vec<Definition>, ToolError>>`, and hands out a `&'static [Definition]`. A parse failure is a `ToolError` naming the tool, so it surfaces at `construct` rather than at a call.
  - `src/check.rs` is the argument check, `check(tool, schema, args)`. It walks `required`, then `properties`, then `enum`, then `type`, then `items`, and returns the first refusal naming the parameter.
  - `src/pack.rs` holds the `PackInfo`, `install`, `construct`, the `Std` object, the `Pack` impl, and the `NOT_IMPLEMENTED` constant.
  - Nothing in the crate names a runtime, a client, or a service. `construct` reads no Host-wide service and the Pack declares no `needs`.
- Modules and interfaces:
  - `Definition` is a segment, a wire name, a description, and a parameters value. The JSON file's own shape is a private `File` struct of the same three fields without the segment. The segment comes from the file name in the `SOURCES` table rather than from the JSON, so a file cannot disagree with its own name.
  - `construct(name, config, services)` refuses any configuration but null or an empty object, then builds one `ToolInfo` per definition from the id, the description, and the parameters, with the wire name set. The id is the Pack id `construct` receives, a slash, and the segment, so it always sits under the Pack as the Plugin contract requires.
  - `Pack::tools` clones the stored list. `Pack::call` resolves the tool by the last id segment, looks its definition up by that segment, runs the check, and answers the stub text as trusted output.
  - The definition lookup is a free function rather than a method, because it reads only the static tables and the compiler refuses an unused receiver.
- File and public API changes:
  - The crate is new, so `crates/plugin-std` joins the workspace's member glob and the root `Cargo.toml` gains one entry under `[workspace.dependencies]`. `crates/runvil/public-api.txt` is unchanged, because a Plugin is not part of the Engine's facade.
  - `crates/workshop/server/Cargo.toml` gains `plugin-std.workspace = true`, and `with_plugins` in `crates/workshop/server/src/agents.rs` installs the Pack beside the other three.
  - `crates/workshop/ui/test/docs-claims.mjs` gains `crates/plugin-std` to its Plugin doc list, so the scan reads the crate's comments for the Pack-activation ban and the defined terms.
  - `Cargo.lock` gains the crate and its `serde` edge.
- Data, persistence, failure, security, and privacy constraints:
  - The definitions are checked in as JSON rather than written in Rust, so the stored text is auditable against Cursor's own and a diff shows exactly which words changed. `schemas/EDITS.md` records each edit as old line then new line.
  - The snapshot date is 2026-10-27. Nothing regenerates the files; following a Cursor update is a deliberate re-snapshot with its own commit.
  - No new dependencies beyond `serde` and `serde_json`, both already in the workspace. No unsafe code, no filesystem access of the crate's own, no build tooling.
  - The real operations, when they land, go through `ToolContext::access()`, which lends an `Access` rooted at `/`. That is what puts every write under the Mode policy: the access gates every mutation before it performs one and answers an approval-required error under `Mode::Ask`, and a refusal for a non-markdown path under `Mode::Plan`. The Pack's only job is to translate that error for the model.

### The VFS surface the real operations use

`Access` in `crates/runvil-internal/vfs/src/handle/operations.rs` already provides most of what the six tools need. This table is the starting point for the real-operation work.

- `read` and `read_string` serve `Read`, `Grep`, and `StrReplace`.
- `write` serves `Write`, and `mkdir` gives it the parent creation it needs.
- `str_replace` serves `StrReplace` for the single-occurrence case.
- `remove` serves `Delete`, and it answers `Ok(false)` for an absent path rather than an error.
- `glob` serves `Glob` and gives `Grep` its candidate list.
- `list` and `stat` serve `Access`'s directory and metadata needs.

Three facts follow from that table:

- `Access::str_replace` already rejects an empty old string and already requires exactly one occurrence, so only Cursor's trimmed-line fallback and `replace_all` are missing above it.
- `Access::remove` answering `Ok(false)` for an absent path is what `Delete` maps to its not-found result.
- `Access` has no search method at all. `Grep` is the one tool with no direct mapping, which is why its backend decision gates the work.

</implementation-contract>
<verification-contract>

## Testing Plan

The tests that shipped cover the crate bottom-up: the definitions, the argument check, the Pack, and the whole path through a Host-shaped call. The remaining work has its own tests per tool.

- Unit:
  - `src/spec-tests.rs` pins the six segments in order and the six wire names, checks that every definition carries a description and an object schema with at least one property, checks that every required parameter is declared and described, and refuses a definition that still names behavior this build lacks.
  - `src/check-tests.rs` drives the check with a hand-written schema: a matching call passes, a missing required parameter fails naming it, a required parameter passed as null fails, a wrong type fails naming the parameter and the value, non-object arguments fail, a value outside an enum fails listing the allowed values, an integer schema accepts a whole number and refuses a fraction, an array schema checks every element, and an undeclared parameter passes through.
  - `src/pack-tests.rs` pins the `PackInfo`: the FQN, no prelude, no per-run needs, and the stub text's exact clauses, including that it names the tool twice and starts by saying what is missing.
- Integration and end-to-end:
  - `tests/it/tools.rs` goes through `install`, `construct`, and `TestCall`. It checks that `install` inserts one Pack with the config under the `std` key and that a second insert is refused, that `construct` names six tools under whatever Pack id it receives and refuses any configuration but null or an empty object, that every tool carries Cursor's wire name, that no tool survives a stop or reports structured output, that every tool answers the stub text naming itself, and that each kind of bad call fails with the parameter named.
- Regression, security, and performance:
  - The stub text test asserts the whole sentence, so a future edit that softens "nothing in this session will change that" or drops the "safe to call again" clause fails.
  - The definition test asserts that no description mentions PDFs, the Shell tool, or the Cursor workspace root, so an edit that reintroduces a promise this build cannot keep fails.
- Exit criteria, every command below passes:
  - `cargo nextest run --locked -p plugin-std --all-features`.
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`.
  - Both clippy runs with `CARGO_BUILD_WARNINGS=deny`, and `cargo check -p gateway --no-default-features`.
  - `cargo fmt --all --check`.
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`.
  - `cargo test -p build-xtask` and `node --test crates/workshop/ui/test/docs-claims.mjs`.

</verification-contract>
<decision-record>

## Decision Record

- Decisions:
  - Name it `std`, not `core`. Three subagents surveyed Zed, Pi, Goose, Codex, OpenCode, Gemini CLI, Cline, OpenHands, Claude Code, Continue, Roo Code, LangChain, and a set of blog posts. Nobody has a standard name for this bundle; the commonest is a plain `tools` directory, and the only distinctive one is Goose's `developer`. `std` reads as "the standard tools", sits well beside `plugin-mcp` and `plugin-web`, and avoids confusion with the Engine and Harness crates. Product owner's choice: "std yes".
  - Two name layers, because Runvil has two audiences. Lua calls tools by id and models call them by wire name, and the two do not have to agree. The Lua ids stay short and prefixed (`std/rm`), and the wire names copy Cursor exactly (`Delete`), so a model that knows Cursor's tools needs no new instructions.
  - Copy Cursor's definitions into checked-in JSON rather than writing descriptions by hand. Rationale: the model was trained against that exact text, and a stored copy can be diffed against Cursor's.
  - Return the stub as a successful `ToolOutput`, never a `ToolError`. Rationale: a valid call must succeed so that success and failure are both exercised end to end, which could not happen if every call failed. Product owner: "I want it to be a success, so that we can test what happens with success versus failure."
  - The stub names itself twice and says a repeat call is safe but identical. Rationale: a model that reads "not implemented" may retry, and it should learn from the text that retrying is pointless rather than conclude its arguments were wrong. Product owner: "point out it is safe to call again, but produces identical result."
  - Edit Cursor's text where it promises behavior this build does not have, and record each edit. Rationale: a description that promises PDFs or points at a `Shell` tool we do not offer invites calls that will fail. Product owner: "yes edit those lines, we dont have pdf."
  - Gate mutations with the VFS Mode policy rather than a Host service the tools check. Rationale: every operation already passes through the access, so a tool cannot forget the check and a future tool cannot bypass it, and the CLI case needs nothing because a Host that wants Agent mode simply never hands out a handle.
- Rejected alternatives:
  - A Host-wide service the file tools consult to decide whether they may mutate. Rejected: it relies on each tool remembering to ask, and it duplicates a policy the VFS already enforces.
  - Marking mounts read-only instead. `RealBackend` has a static `Writability::ReadOnly`, but it is a mount-time property, not a per-run setting, so it cannot express Ask mode flipping mid-run.
  - `core` as the crate name. Rejected: it suggests the minimal no-allocation subset in Rust, and it reads as Engine or Harness rather than as a tool bundle.
  - Exact Cursor text with no edits. Rejected by the product owner, because several lines describe behavior this Pack does not have.
  - Keeping `StrReplace`'s matching entirely in the VFS, or entirely in the Pack. Still open; neither is chosen yet.
- Assumptions, risks, and notes:
  - Cursor's `Grep` in this run's tool context has `context` and `-n`, and the reverse-engineered report says it has neither. The plan assumes the report is right about the shipped tool and this context is a variant, but that is unverified, so the schema fix waits on an answer.
  - Cursor's `ListDir` is gated behind `enableDeprecatedLsTool` defaulting to false, and its replacement exists only as proto (`ListDirV2Params`, `ListDirV2Stream`, and a server config field `listDirV2PredefinedIgnoreGlobs`). There is no model-facing definition to copy, so this plan leaves it out.
  - Cursor's `Glob`, `Grep`, and `ListDir` respect `.gitignore` and Cursor's own `.cursorignore`. The VFS has no ignore-file concept, so a Runvil listing or search sees `target/` and everything else. That divergence is unaddressed and will show up when the real operations land.

### Deferred and Out of Scope

- Deferred: `ListDir`, until Cursor ships its replacement and there is a definition worth copying.
- Deferred: user approval for mutations, which needs an operator flow the VFS's approval-required error can feed.
- Deferred: ignore-file handling in the VFS, if the divergence turns out to matter in practice.
- Out of scope: the shell, which is a separate Pack, most likely over bashkit.
- Out of scope: any change to the Engine, the Harness, or the facade. This is a Plugin crate and its tests.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p gateway` (the workspace default member; plain `cargo build` builds only it). This crate: `cargo build -p plugin-std`.
- Focused test command pattern: `cargo nextest run --locked -p plugin-std --all-features`; a single test: add its name as a filter. Structural checks: `cargo test -p build-xtask`. Wording scan: `node --test crates/workshop/ui/test/docs-claims.mjs`.
- Component test command pattern: `cargo nextest run --locked -p <crate> --all-features`; drop `--all-features` for `workshop`, `workshop-server`, and `workshop-server-api`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`.
- Linter command: `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `CARGO_BUILD_WARNINGS=deny cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`; headless shape: `cargo check -p gateway --no-default-features`.
- Formatter check command: `cargo fmt --all --check`.
- Docs command: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`.
- Test placement and naming conventions: unit tests sit beside their module as a `<stem>-tests.rs` sibling wired by `#[cfg(test)] #[path = "<stem>-tests.rs"] mod tests;`, each opening with a `//!` line naming what it covers. Integration tests compile as one binary per crate under `tests/it/` for the Plugin crates, with `main.rs` plus one module per topic.
- Directory map: `crates/` holds the four Plugin crates at its top level (`plugin-mcp`, `plugin-std`, `plugin-user-input`, `plugin-web`) beside `runvil-plugin`, the contract they implement. The VFS is private to the runvil family at `crates/runvil-internal/vfs`. Workshop's Host is `crates/workshop/server`.
- Component boundaries: `plugin-*` crates depend only on `runvil-plugin`, `shared-*` crates, and outside libraries, and `cargo test -p build-xtask` enforces it. Only `workshop-server` and `harness-gateway-client` may name a Plugin crate among the Engine, gateway, and Harness crates. The VFS crate is std-only with no normal or build dependency.
- Conventions summary: Rust 2024, workspace version 0.3.0, BSL-1.0; workspace lints inherited (pedantic denied, `unwrap_used` and `expect_used` denied outside tests, `unsafe_code` denied); every Rust file at most 500 lines, enforced by the crate's own `build.rs` including the `build-ceiling` source; every source file opens with a `//!` doc; crate docs carry `## Invariants`; kebab-case sibling files `foo-bar.rs` wired by `#[path]` until a group reaches three files; capitalized defined terms (Engine, Harness, Host, Plugin) checked by `docs-claims.mjs`; model-readable error messages naming required versus actual; no doctests anywhere, enforced structurally.

</project-survey>
<execution-plan>

## Execution Instructions

The five shipped stages were done in one pass. The remaining work is one schema fix followed by six tools, each of which can land alone.

<step-1>
Already done, recorded for the record: the crate exists with the Pack, the six definitions, the argument check, the stubs, the three unit test files, the integration test, the workspace wiring, and the workshop-server install. Its tests and every workspace gate passed.
</step-1>

<step-2>
Settle the Grep schema question, then fix `schemas/grep.json`. Compare this run's tool context, which has `context` and `-n`, against the report's claim that Cursor's Grep has neither. Whichever answer comes back, correct the file and add a test to `spec-tests.rs` that asserts the `Grep` parameter set exactly, so it cannot drift again. Note the answer in the plan's Decision Record rather than leaving the question standing.
</step-2>

<step-3>
`Read`, because it is pure formatting over one `Access::read_string` and it establishes the line-number formatter the other tools reuse. Implement the 1-indexed offset, the negative offset counting from the end, `limit`, the 100,000-character cap as an error rather than a truncation, the six-wide right-aligned line numbers, and the above/below not-shown markers. Flip the tool's entry in `tests/it/tools.rs` from the stub text to real behavior in the same change.
</step-3>

<step-4>
`Write` and `Delete` together, since both are one VFS call plus a message. `Write` is `Access::write` after `Access::mkdir` for the parent, and it answers `Wrote contents to <path>` with the argument as given. `Delete` is `Access::remove`, mapping `Ok(false)` to the not-found result and refusing a directory. Each gets its own commit.
</step-4>

<step-5>
`StrReplace`, after deciding whether it owns its matching or layers the trimmed-line fallback above `Access::str_replace`. Either way the requirement is that untouched lines keep their exact bytes, because rewriting the whole normalized file reindents every line. Implement `replace_all`, and keep the failure strings exact.
</step-5>

<step-6>
`Glob`, once its case sensitivity, recursion, and dotfile behavior are confirmed against Cursor's ripgrep walk. Adjust in the Pack rather than changing shared VFS behavior for one tool, and note any divergence in the Decision Record.
</step-6>

<step-7>
`Grep`, last, because it is the largest and because its backend decision comes first. Either the VFS gains a search capability with its own claim, which is better for large trees, or the Pack walks with `Access::glob` and reads with `Access::read_string`, which is less infrastructure. Record the choice and its consequence for memory.
</step-7>

<step-8>
After the six tools are real, wire `VfsError::ApprovalRequired` to a model-readable refusal, so a run in `Mode::Ask` that tries to write gets an answer explaining that approval is required rather than a raw VFS error. Real approval is later work and needs an operator flow.
</step-8>

</execution-plan>
