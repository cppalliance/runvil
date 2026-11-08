---
name: Run Inbox
overview: Give every chain of a run an inbox in the Engine, let the Host send operator messages through RunControl and chains steer and resume their tasks, add a built-in runvil/user Pack, remove the user-input plugin, run services, and the Workshop's token wait registry, and update the language guide in runvil-docs.
todos: []
isProject: false
---

<plan-context>

# Run Inbox and User Pack

Every chain of a run gets an inbox in the Engine. The Host sends the operator's messages to the main chain with `RunControl::send`, a chain steers the tasks it owns with `tasks.send`, and a chain reads its own inbox with `tasks.receive()`. A message resumes a chain waiting on its inbox. A chain inside `models.loop` reads it at its next model request: a model reply still streaming is cut off and requested again with the message appended, while a running tool batch finishes first. Otherwise the message waits in the chain's queue. A stop never drops a queued message, and messages a run never read come back on its report. A task can hand back a result with `tasks.reply` and stay open for follow-ups, keeping its conversation, up to a cap on idle tasks. A new built-in Pack, `runvil/user`, marks a prompt that needs a human, and Hosts learn when the main chain waits for one from the events they record. The `plugin-user-input` crate, its input broker, run services, and the Workshop's token-based wait registry go away. The language guide in the sibling `runvil-docs` repository is updated to match.

## Notes

- Repository: `c:\Users\Vinnie\cursor\runvil`, a Rust workspace on Windows (PowerShell). Every path in this plan is relative to its root, except the guide paths in steps 5 and 6 (see Docs).
- Setup: `npm ci --prefix crates/workshop` (Node 22) before cargo commands that build the Workshop (`.github/workflows/ci.yml`).
- Build: `cargo build --locked -p workshop` (`.github/workflows/ci.yml` line 198).
- Package tests: `cargo nextest run --locked -p <package>`, for example `-p runvil-engine`, `-p runvil-lua`, `-p harness-runner`, `-p workshop-agents` (`plan/2026-11-08-1-per-run-packs.plan.md`, Notes).
- Full suite, both of (`AGENTS.md` lines 45 to 51 hold the suite, lint, format, rustdoc, facade, structural, and wording checks):
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
- Lint: run `$env:CARGO_BUILD_WARNINGS = 'deny'` first, then both of:
  - `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
- Format: `cargo fmt --all --check`.
- Structural checks: `cargo test -p build-xtask`.
- UI checks, run in `crates/workshop`: `npm run typecheck --workspaces --if-present`, `npm run build --workspace ui`, `npm test --workspaces --if-present` (`.github/workflows/ci.yml` lines 228 to 241).
- Wording: `node --test crates/workshop/ui/test/docs-claims.mjs`, from any directory.
- Rustdoc: run `$env:RUSTDOCFLAGS = '-D warnings'` first, then `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, `cargo doc -p runvil --no-deps`, `cargo doc -p harness --no-deps`, `cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`, and `cargo doc --locked --no-deps -p workshop-server --document-private-items` (`.github/workflows/ci.yml` lines 97 to 108 and 154). A doc link to an item a step removes fails these.
- API surface: run `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue`, then `cargo +nightly-2026-09-05 xtask api --check` (toolchain pinned in `crates/build-xtask/src/api/toolchain.rs` line 19). A step that changes the `runvil` facade regenerates `crates/runvil/public-api.txt` with `cargo +nightly-2026-09-05 xtask api --bless`; a plain `xtask api` only prints the difference (`crates/build-xtask/src/api.rs` lines 31 to 35).
- Docs: the language guide is the sibling repository `runvil-docs` (`c:\Users\Vinnie\cursor\runvil-docs`), and the `src/language/` paths in steps 5 and 6 are relative to its root. Build it from the `runvil` root: run `$env:RUNVIL_DOCS = 'c:\Users\Vinnie\cursor\runvil-docs'`, put `$env:USERPROFILE\.cargo\bin` on `PATH` (or set `$env:MDBOOK`), run `cargo xtask site --books-only`, which stages the chapters, builds them with mdbook, and checks links, then run `cargo test -p build-user-guide` (`runvil-docs` files `CONTRIBUTING.md` line 7 and `plans/build-language-guide.md` line 149). House rules: no em dash and no double dash, every code fence opens with four backticks, and each paragraph stays on one line (`runvil-docs` file `CONTRIBUTING.md` lines 14 to 18). Write each rule as steps 1 to 4 built it, checked against their tests. Steps 5 and 6 come before step 7 so that the final step builds and tests `runvil` as a whole. The guide therefore drops the `user-input` Pack, `input.ask`, the input broker, and host services, and describes the chat agent's new loop, before step 7 changes the code to match.
- Guide steps: steps 5 and 6 change files only in `runvil-docs`, so for those two steps `runvil-docs` is the repository: run their git commands there and take their commit hashes from it. The plan copy stays in `runvil`, outside `runvil-docs`, so commit it apart from the guide: right after a guide step's first commit in `runvil-docs`, commit the plan copy alone in `runvil` with the same message. From then on, an amend that stages the plan copy amends that `runvil` commit, and an amend of guide files amends the `runvil-docs` commit. On resume, take a guide step's commit from `runvil-docs`'s HEAD. Before step 5 makes its first change, check that `runvil-docs` has a clean worktree; when it does not, stop and ask the user.

## Terms

- The **main chain** is a run's H1 pass and section walk. Every other chain is a task (`tasks.spawn`, `fanout`) or a `call` child (`ChainKind` in `crates/runvil-internal/engine/src/execute/scheduler/chain_record.rs` lines 43 to 61).
- A chain's **inbox** is its queue of text messages, oldest first. The main chain and each task have one, and a chain reads its own with `tasks.receive()`. The main chain is task `0` (`sys.taskid`), so the `tasks` functions that act on the caller's own task, as `tasks.note` does, work there too.
- A `call` child has no inbox of its own. It blocks its caller and runs inside the caller's task, so `sys.taskid` reads the caller's (`crates/runvil-internal/engine/src/execute/scheduler/chain.rs` lines 98 to 102). It uses its caller's inbox, transitively, and everything this plan says of the main chain or a task also holds for their `call` children. A message to the main chain therefore reaches a `models.loop` running inside a `call`.
- A task's **owner** is the chain that spawned it.
- A task is **idle** once it has replied with `tasks.reply` and waits on its inbox.

## Lua syntax

Every Lua form the plan adds, changes, or removes. The sections below give the full rules.

```lua
-- FRONTMATTER, as YAML comments
-- uses: [user]
--   new: declares the runvil/user built-in, which
--   refuses the prompt at its first step when the
--   run has no user
-- uses: [user-input]
--   removed with the user-input Plugin: a prompt
--   that still declares it is refused at its first
--   step, missing required Pack: user-input

-- NEW. A chain's inbox, read with tasks.receive().
-- A call child uses its caller's inbox.
local text = tasks.receive()
-- waits for this chain's next message and returns it
-- as sent; queued messages return at once, oldest
-- first; on the main chain, the operator's messages
if tools.packs.get('user') then
  messages:user(tasks.receive())
end
-- a prompt that runs with or without a user checks
-- for the user Pack, which only a run with a user has

-- NEW. tasks.send: an owner steers a task it spawned.
local t = tasks.spawn('## Worker')
tasks.send(t, 'also cover the tests')
-- true: queued in t's inbox, by Task handle
tasks.send(t.task, 'also cover the tests')
-- the same, by id string
tasks.send(t, 'more')
-- false once t has ended: returned, failed, canceled,
-- or closed by the idle cap; the text is dropped
tasks.status(t).blocked
-- 'inbox' while t waits in tasks.receive(), and t
-- gives back its slot

-- NEW. tasks.reply: a task answers and stays open.
local text = tasks.reply('summary')
-- in a task: publishes 'summary' as its latest result,
-- idles, and returns its owner's next message
tasks.reply(42)
-- publishes '42': the value converts as a section's
-- return value does
tasks.reply()
-- publishes '', the result of a walk that returns
-- nothing
tasks.status(t)
-- an idle task reads state 'running', blocked
-- 'inbox', no ok, and the section it replied from
tasks.ready(t)
-- true while t's latest reply waits undelivered
tasks.pending()
-- lists t while it is idle, since it has not ended
local _, ok, reply = tasks.join_any({ t })
-- delivers t's latest reply once; ok is true
local results = tasks.join({ t })
-- the same: results[1].result is the reply
tasks.send(t, 'now shorten it')
-- resumes t's tasks.reply with the text; t runs again
tasks.cancel(t)
-- an idle task ends canceled
return 'done'
-- an owner may end with idle tasks: they are abandoned
-- with no tasks_live error; a running task still
-- raises it

-- CHANGED. models.loop reads the chain's inbox.
models.loop(compact)
-- While the loop runs, a message to its chain cuts off
-- a model reply still streaming and asks again with
-- the message appended, or waits for a running tool
-- batch to finish first. Nothing raises, and the
-- withdrawn request counts toward neither the round
-- cap nor turns. The loop appends every queued message
-- before each request and checks once more before it
-- returns. It still returns nothing.
messages[i].content
-- a message the loop appended on the main chain:
-- 'This message arrived while you were working:\n\n'
--   .. text
-- on a task:
-- 'The task that started you sent this message while '
--   .. 'you were working:\n\n' .. text

-- REMOVED with the user-input Plugin
input.ask()
-- input is nil: read the operator with tasks.receive()
tools.offer('user-input')
-- raises: no Pack named user-input
tools.offer('user-input/ask')
-- raises: not a tool in this run; the model has no
-- user-input_ask tool
tools.offer_dynamic('user-input')
-- raises: no Pack named user-input
tools.call('user-input/ask')
-- raises unbound_tool
tools.packs.get('user-input')
-- nil
tools.get('user-input/ask')
-- nil

-- REFUSED. Each raises.
tasks.reply('x')
-- on the main chain or a call child of it: the main
-- chain has no owner
tasks.reply({ done = true })
-- a table is not a result, as with a section's return
tasks.receive('x')
-- any argument
tasks.send(t, 42)
-- text must be a string: a lua error
tasks.send('9.9', 'x')
-- an id string naming no task the caller owns:
-- task_not_owned
tasks.send(sys.taskid, 'x')
-- no chain owns itself: task_not_owned
tasks.join_any({ t })
-- t is idle, its reply was delivered, and nothing was
-- sent since: task_consumed

-- IDLE CAP. A run holds at most
-- RunLimits::max_idle_tasks idle tasks, 8 by default.
-- One more reply closes the task idle longest, other
-- than the one replying: it ends with its latest reply
-- as its result, as if it had returned it.
tasks.status(oldest).state
-- 'done', ok true; TaskSucceeded reports it
tasks.send(oldest, 'x')
-- false
tasks.join_any({ oldest })
-- delivers its latest reply if no wait has, and
-- otherwise raises task_consumed

-- WAITING AND STALLS
tasks.receive()
-- on the main chain with nothing else to run: the run
-- waits for RunControl::send or a cancel, even in a
-- run with no user
local t = tasks.spawn('## Worker')
-- Worker calls tasks.receive(), and nothing sends to it
tasks.join({ t })
-- the run ends with code stalled, naming t

-- THE TWO LOOPS
-- a conversation with the operator (uses: [user])
while true do
  messages:user(tasks.receive())
  pcall(function() return models.loop(compact) end)
end
-- a stop raises canceled inside the pcall; a message
-- sent meanwhile stays queued for tasks.receive()

-- a task that answers its owner and stays open for
-- follow-ups
while true do
  models.loop()
  messages:user(tasks.reply(messages[#messages].content))
end
```

The removed forms raise what any Pack or tool a run lacks raises today: `tools.offer` and `tools.offer_dynamic` raise a `lua` error (`crates/runvil-internal/lua/src/tools/offer.rs` lines 161 to 182), `tools.call` raises `unbound_tool` (`crates/runvil-internal/engine/src/error/tools.rs` lines 103 to 109), `tools.packs.get` and `tools.get` return nil (`crates/runvil-internal/lua/src/packs.rs` lines 160 to 168 and `crates/runvil-internal/lua/src/tools/objects.rs` lines 73 to 83), a prompt that declares a Pack no Host installs is refused with `missing required Pack` (`crates/runvil-internal/engine/src/execute/requirements.rs` line 124), and only a declared, usable Pack installs a prelude, so no Pack defines `input` (`crates/harness-internal/runner/src/packs.rs` lines 458 to 472). `tasks.reply` converts its value as a section's return does (`crates/runvil-internal/lua/src/section.rs` lines 24 to 49).

## Delivery rules

A message to a chain does exactly one of these, and never touches another chain:

- **Waiting reader:** the chain is parked in `tasks.receive()` or `tasks.reply`. The call returns the text.
- **Model request in flight:** the chain is in `models.loop`, and the model request it issued has no answer yet; the Host may still be streaming the reply. The Engine withdraws that request through `Step::Pending`'s `withdrawn` list, the loop appends the message as a user record, and it issues the request again. The withdrawn request leaves nothing in `messages`, and no error reaches Lua.
- **Tool batch in flight:** the chain is in `models.loop`, and tool calls it issued have no answer yet, whether the Host is running them or holding them for a free slot. Nothing is withdrawn: every call finishes and records its real result, and the loop appends the message before its next model request.
- **Otherwise:** the message waits in the chain's inbox. `models.loop` appends every queued message as a user record before each model request, and checks once more before it returns, so it never returns with a message unread. `tasks.receive()` takes the oldest.

`models.loop` prefixes each message it appends with one line saying it arrived during the work, so the model reads it as new input rather than as part of the task it started with:

- On the main chain: `This message arrived while you were working:`, a blank line, then the text.
- On a task: `The task that started you sent this message while you were working:`, a blank line, then the text.

A message a prompt reads itself, through `tasks.receive()` or `tasks.reply`, comes back exactly as sent.

The inbox never merges messages. Several messages that `models.loop` appends before one request stay separate user records, each with its arrival line, in `messages`, in the record, and as events. The request projection already joins consecutive user records into one provider message, with a blank line between them, so the model reads them as one message (`crates/runvil-internal/lua/src/projection.rs` lines 18 to 22 and 176 to 191).

A stop (`RunControl::stop_round`) and every withdrawal leave inboxes untouched, so a queued message is never dropped while its chain lives.

Two events report the inbox, each under the receiving chain's provenance:

- `Event::InboxMessage` reports each message that enters a chain's inbox. For a `tasks.send` message, the step that applies it emits the event. For a message the Host sends, `Run::deliver` returns the event, and the Harness records it at once, ahead of the step that applies the message.
- `Event::InboxWaiting` reports a chain parking in `tasks.receive()` or `tasks.reply` with its inbox empty, in the step where it parks.

Events are outputs, except one. A replay feeds the Engine only the run's inputs and compares the effects and events the Engine emits against the record (`crates/runvil-internal/types/src/ids.rs` lines 171 to 185; `crates/workshop/run-log/tests/it/fidelity.rs` lines 1 to 6). The inputs are the run's parameters, each answer, and each message the Host sent. An `InboxMessage` event on task `0` is always such a message, since `tasks.send` cannot reach the main chain, so a replay calls `Run::deliver` with its text when it reaches one and compares the event it returns with the record. The record needs no new kind: the Workshop's run log accepts only `effect`, `answer`, and `event` (`crates/workshop/run-log/src/schema.rs` line 41), and its recorder tee hands the conversation every `event` record once the run log has stored it (`crates/workshop/agents/src/tee.rs` lines 43 to 62).

Who fills which inbox:

- The main chain's inbox is filled only by `Run::deliver`, which the Harness calls for each `RunControl::send`.
- A task's inbox is filled only by its owner, through `tasks.send`. Lua can name any task by its id string (`crates/runvil-internal/lua/src/__impl_tasks.lua` lines 15 to 19), so the Engine's existing ownership check, `task_not_owned` (`crates/runvil-internal/engine/src/execute/tests/waits.rs` lines 187 to 223), keeps it to the owner.

## Interrupts

Today a dropped chat round or tool call raises `Canceled` at the `models.loop` call site, and a dropped tool batch leaves no record in `messages`, because the loop writes the assistant's `tool_calls` record only once every call has a result (`crates/runvil-internal/lua/src/models_loop/machine.rs` lines 194 to 233 and 333 to 364; `crates/runvil-internal/engine/src/execute/scheduler/apply.rs` lines 113 to 198). That stays the behavior of a stop.

A message is not a stop. It withdraws only a model request that has no answer yet, which the loop issues again with the message appended. The two never mix: an effect the Host drops for a stop and answers `Dropped` raises `Canceled` as today, while an effect the Engine withdrew for a message is issued again, and the Host's `Dropped` answer to it is discarded, as answers to withdrawn effects are today (`crates/runvil-internal/engine/src/execute/run.rs` lines 37 to 76). It never withdraws a tool call, so a message never leaves a call without its real result, and nothing needs a made-up tool result. A withdrawn request counts toward neither the round cap (`max_tool_iterations`) nor the chain's turns, since it produced no round. To cut a long tool, the Host stops the round and then sends. A Host that stops and then sends always gets the stop: the effect loop handles a stop before any send that follows it, and within one step the Engine applies answers before messages, so a dropped model request raises `Canceled` and the message waits for the next reader instead of withdrawing that request. In the built-in chat agent, whose `models.loop` runs inside `pcall`, the stop's `Canceled` is caught, and the next `tasks.receive()` returns the message.

## Unread messages

- A message stays in its inbox until a chain takes it; only the end of its chain or of the run removes it.
- When the run ends, by finishing, failing, or being cancelled, `Step::Done` lists the main chain's unread messages, oldest first. The Harness adds any message `RunControl` accepted but never delivered, and reports them all as `RunReport::unread`, so a Host can give them back to the person who sent them. `RunControl::send` returning true means the message was accepted, not read.
- A task that ends with messages in its inbox drops them. Its owner sent them from inside the run and sees the task's ending through `tasks.status`.

## Replies and idle tasks

- `tasks.reply(value)` publishes `value` as the task's latest result, the way a section's return value becomes a task's result, then parks the task on its inbox. The task is idle. `tasks.status` reads it as running and blocked on `inbox`, as a task waiting in `tasks.receive()`, with no `ok` and with the `section` it replied from. `tasks.ready(t)` is true while its latest reply waits undelivered, so a wait would return at once, and false once a wait took it. `tasks.pending()` lists idle tasks, since they have not ended.
- A wait on the task (`tasks.join_any`, `tasks.join`) delivers each reply once. A wait on an idle task whose latest reply was delivered, with no message sent since, raises `task_consumed`, as a delivered task does today.
- `tasks.send` to an idle task resumes its `tasks.reply` call with the text, and the task runs again.
- A task ends only by returning, by `tasks.cancel`, when its owner ends, or, while idle, when the idle cap closes it. An owner that ends with idle tasks abandons them without `make_tasks_live_error`; running tasks still raise it (`crates/runvil-internal/engine/src/execute/scheduler/task_end.rs` lines 77 to 103 and 144 to 169).
- A task waiting on its inbox gives back its concurrency slot, as a `tasks.join_any` wait does today (`crates/runvil-internal/engine/src/execute/scheduler/admission.rs` lines 215 to 256).
- A run holds at most `RunLimits::max_idle_tasks` idle tasks, 8 by default. When a reply would put it over the cap, the task idle longest, other than the one replying, closes: its chain ends and its latest reply stays its result, as if it had returned it. It reads `done` with `ok` true, `TaskSucceeded` reports it, and its owner's next `tasks.send` to it returns false. An idle task keeps its Lua VM, which cannot be rebuilt from its conversation, so the cap bounds memory.
- A wait on a task the cap closed delivers its latest reply when no wait has yet, and otherwise raises `task_consumed`, as for any task that returned.
- The cap never abandons a task. Abandonment stays what `AbandonReason` says it is, an owner that ended (`crates/runvil-internal/types/src/ids.rs` lines 233 to 261), and an abandoned task is still never delivered (`crates/runvil-internal/engine/src/execute/scheduler/waits.rs` lines 12 to 14).
- `tasks.reply()` and `tasks.reply(nil)` publish `""`, the result of a task whose walk returns nothing. A table raises, as a section's table return does.
- `tasks.reply` on the main chain raises a Lua error, since the main chain has no owner.
- Today's lifecycle is `Running` to `Done` to `Delivered`, or `Running` to `Canceled` or `Abandoned`, and `EntryState` holds a finished task's outcome until a wait takes it (`crates/runvil-internal/engine/src/execute/scheduler/tasks.rs` lines 59 to 75). Step 3 first separates that outcome from whether the backing chain is live, so a reply fills the outcome of a live entry and a wait takes it as it takes a finished task's. The idle cap's close then ends the chain and leaves the outcome where it is, which is exactly how a returned task looks.

## Stalls

- When the main chain waits on its inbox and nothing else can move, `Run::step` returns `Pending` with `waiting_for_input: true` and no effects. The Harness then waits for `RunControl::send` or a cancel, instead of ending the run with its `STALLED` error (`crates/harness-internal/runner/src/effect_loop.rs` lines 82 and 250).
- When no chain can move and the main chain is not waiting on its inbox, the run ends with `ErrorCode::Stalled`, naming the tasks waiting on their inboxes. This one rule replaces the scheduler's `Internal` error, "the scheduler stalled with no ready chain, no in-flight request, and no queued chain that can be admitted" (`crates/runvil-internal/engine/src/execute/scheduler/drive.rs` lines 70 to 78), since only the main chain's inbox can be filled from outside the Engine.
- In a run with no user, a main chain waiting on its inbox just waits, since a Host may still call `RunControl::send` from code.

## The user Pack

- `runvil/user` is a built-in beside `runvil/subagents` (`crates/runvil-plugin/src/registry.rs` lines 239 to 251), so no Plugin can take the `user` id. It offers no tools and has no prelude. A prompt that needs a human declares it as `uses: [user]`.
- A run holds it only when `RunSetup.has_user` is true. Otherwise it is unavailable with the reason "this run has no user", and a prompt that declares it is refused at the run's first step with `RequirementsUnmet` on its `uses:` line, through the existing unavailable-Pack path (`crates/runvil-internal/engine/src/execute/requirements.rs` lines 44 to 102).
- The main chain reads the operator's messages with `tasks.receive()`. A prompt that works with or without a user does not declare `user`; it checks `tools.packs.get('user')`, which is nil for a Pack the run cannot serve (`crates/runvil-internal/engine/src/execute/params.rs` lines 198 to 205).
- The `user-input` Pack's model-facing ask tool has no replacement, since a model reply already ends `models.loop` and the prompt goes back to `tasks.receive()`.

## The Workshop

- The built-in chat agent (`crates/workshop/agents/agents/chat.md`) declares `user` instead of `user-input` and reads the operator with `tasks.receive()`.
- A conversation sends the operator's text with `RunControl::send` as soon as the operator sends it. Before its run has its Harness there is no `RunControl` to send to, so `Conversation::send_input` returns false, and the UI keeps send disabled until the agent first waits or works. While the agent waits, the message answers it. While the agent works, it steers the running turn by the delivery rules: a reply still streaming is cut and asked again with the message, and running tools finish first. The Workshop's own queue of messages the operator can still edit or unsend, and a send-now action that stops the round before it sends, come later (see Decisions).
- `AgentEvent::from_event` builds the existing `user_message` agent event from `Event::InboxMessage` on the main chain, where today it builds it from the ask tool's `ToolResult`, and a new `awaiting_input` agent event from `Event::InboxWaiting` on the main chain (`crates/workshop/server/src/agents/wire.rs` lines 237 and 280 to 290). The UI keeps rendering `user` items from `user_message` (`crates/workshop/ui/src/services/agent-session.ts` lines 432 to 435) and stops showing the agent as working at `awaiting_input`, where today an `input_required` frame does that, and shows it working again at the next `user_message`. An `agent_message` no longer ends the working state (lines 445 to 447), since a reply no longer always ends the turn: a message that lands as the final request answers makes the loop go on. Attach replays a session's event log from its start (`crates/workshop/server/src/agents/wire.rs` lines 7 to 9), so a reattached client ends in the latest state.

## Decisions

- Sub-agents are tasks in the same run, not separate runs, so `RunControl` reaches only the main chain.
- `RunSetup.has_user: bool` replaces run services; nothing else used them, and the runner's one read of them hands them to the factories (`crates/harness-internal/runner/src/prepare.rs` line 122).
- The inbox functions live on `tasks`, beside `tasks.send` and `tasks.note`, which already acts on the caller's own task. The run gains no global.
- One loop form. A conversation with a user loops over `messages:user(tasks.receive())` and `models.loop()`; a resumable task loops over `models.loop()` and `messages:user(tasks.reply(messages[#messages].content))`. `models.loop` keeps returning nothing, since `messages[#messages].content` already holds its final reply. There is no `models.user_loop`.
- A message the Host sends is recorded as its `InboxMessage` event, which `Run::deliver` returns so the Harness records it where the input happened, and a replay delivers it again. Making each message the answer to a standing effect on the main chain would keep replay to answers alone, but that effect would have to be spared by stops, left out of stall detection, and reissued after every message.
- Hosts learn the main chain's input state from the events their recorder receives: `InboxWaiting` when it parks on its empty inbox, and `InboxMessage` when a message arrives. `RunControl` stays `send`, `stop_round`, and cancel.
- A message never withdraws a tool call. Harnesses that steer a running turn let running tools finish and cut at most the model's streaming reply, because a cut-off tool may already have changed something; a stop stays the way to cut tools. The source survey is in the sibling repository `runvil-design`, at `research/2026-11-08-harness-survey-input-steering.md`.
- `tasks.send` and `RunControl::send` deliver the text as given. The Engine adds only the arrival line, and only to messages `models.loop` appends. Labeling a model-written message and wrapping it as untrusted belongs to the future subagents Pack.
- `tasks.send` returns false for a task that has ended, and `RunControl::send` returns false once the run has ended, each dropping the text, as `tasks.cancel` does nothing for a task that has ended (`crates/runvil-internal/engine/src/execute/scheduler/waits.rs` line 26). Neither adds an error.
- One delivery mode in the Engine: a message always steers the loop that is running. A follow-up queue, which holds messages until the agent waits for one, belongs to the Host. The Workshop will keep one, so its operator can edit or unsend a message until it is handed over: it hands over one message per `InboxWaiting` event on the main chain. `RunControl` gains no unsend, since a message it accepted is committed. Until that queue exists, the Workshop sends each message at once, which steers a running turn.
- No `models.loop` option for replies: an author writes the reply loop with `tasks.reply`.
- A conversation keeps no message queue of its own. Its `RunControl` exists only once the run has its Harness, after the model catalog holds a chat model and the agent parses (`crates/workshop/server/src/agents.rs` lines 345 to 370), so until the agent first waits or works the UI keeps send disabled and `send_input` returns false. A message typed early waits in the Workshop's later follow-up queue, not in `Conversation`.
- The idle cap closes the task idle longest as `done`, its latest reply its result, rather than abandoning it: its owner is still alive and can still take that reply, while `abandoned` means the owner ended. The record does not name the cap as the reason; a debug trace line at the close does.
- Orphaning withdraws every effect, not only tool calls, so a message withdrawing a model request and a task's cancel or abandonment share one path, `abort_effect`, and a canceled task no longer pays for a model reply nobody reads.
- `tasks.receive()` raises when given any argument, as `input.ask` does today, and `tasks.send` raises a `lua` error when `text` is not a string, as `tasks.note` does (`crates/runvil-internal/lua/src/__impl_tasks.lua` lines 231 to 237).
- `ToolInfo::survives_stop` stays. After step 7 no shipped tool sets it, since reading the operator becomes an inbox wait, which a stop never reaches, where today the ask tool sets it (`crates/plugin-user-input/src/ask.rs` line 43). Any Plugin tool may still set it. A stop cannot cut such a tool, so under the tool batch rule a message to its chain waits until it finishes, and only a cancel ends it.
- Each `tasks` function this plan adds reaches the scheduler through one `Request` variant and resumes through one `Answer` variant of the Lua protocol (`crates/runvil-internal/lua/src/protocol/request.rs` and `answer.rs`), as `tasks.note` and `tasks.cancel` do: `Receive` in step 1, `Send` in step 2, and `Reply` in step 3. Step 2 adds one more pair for `models.loop`, `Arrivals`: the loop's read of its inbox before it returns, which takes every queued message and never waits, and whose answer also answers a `chat` request that queued messages cut short. Both enums are public to the Engine crate, so the step that adds a variant fences it.

## Out of scope

- The subagents Pack and its model-facing tools.
- An approvals primitive that bypasses the inbox. When it is designed, each approval is its own effect with an id, and a message arriving while an approval is pending closes it with a result saying the action did not run and nobody refused it.
- A report of when a chain reads a message, beside `Event::InboxMessage`, which reports delivery.
- Giving unread messages back to the person in the Workshop UI; the Workshop only logs how many a run left.
- Removing `ToolInfo::survives_stop`.
- Removing the UI's `ask` tool kind, which only `runvil/user-input/ask` produces (`crates/workshop/ui/src/parts/agent/transcript/tool-labels.ts` lines 11 and 60 to 61, with its rows in `transcript-model.ts` and its lines in `tool-line.ts`). Once the ask tool goes, no call takes it.
- The Workshop's queue of messages the operator can still edit or unsend, described under Decisions, and a send-now action that stops the round, cutting running tools, before it sends.
- Messages other than text. Images, files, audio, and video later change the message type that `RunControl::send`, `Run::deliver`, `tasks.send`, `tasks.receive`, `tasks.reply`, and `Event::InboxMessage` pass along, and add content parts beyond `text` and `image_url` (`crates/runvil-internal/lua/src/protocol/record.rs` lines 13 to 30).

### Open questions

None

</plan-context>

## Steps

<step-1>

1. [done] **Main-chain inbox in the Engine** (points: 10) - Give the main chain and each task an inbox in the scheduler, with each `call` child using its caller's. A chain whose inbox is empty parks on a new `Inbox` case of `Parked` (`crates/runvil-internal/engine/src/execute/scheduler/chain_record.rs` lines 194 to 235), reported as `Blocked::Inbox`, and reuses the task wait's slot handling: `Admission::park` gives its slots back as it does for a task wait, and the wake that delivers a message requeues it the same way (`crates/runvil-internal/engine/src/execute/scheduler/admission.rs` lines 215 to 256). Add `tasks.receive()`, `Run::deliver` for the main chain, which returns the message's `InboxMessage` event, `waiting_for_input`, and the main chain's unread messages on `Step::Done`. Update the `Step` destructuring sites and regenerate `crates/runvil/public-api.txt`. Tests: a delivered message resumes a waiting `tasks.receive()` with its text; messages delivered before the call return at once, oldest first; `Run::deliver` returns an `InboxMessage` event on task `0`, and the step that applies the message emits no second one; `waiting_for_input` is true exactly while the main chain is parked on its inbox; `Step::Done` lists the messages left in the main chain's inbox when the run ends, oldest first; a `tasks.receive()` in a `call` child of the main chain returns a message delivered to the main chain, and `waiting_for_input` is true while it waits; `tasks.receive('x')` raises.
   - Score: `Run::deliver` 1, `Step` 2 (`waiting_for_input` 1, `unread` 1), `Event` 1 (`InboxMessage`), `Blocked` 1 (`Inbox`), `tasks.receive` 1, with its wire, `Request` 0 (`Receive`) and `Answer` 0 (`Receive`). Other changes: the inbox queue and its `Parked` case 3, `call` children reading their caller's inbox 1, callers 0.

`crates/runvil-internal/engine/src/execute/run.rs`

```rust
impl Run {
    /// Queues `text` for the main chain's inbox and
    /// returns the `InboxMessage` event that reports
    /// it, which the caller records before its next
    /// `step`. The next `step` applies it. Call it
    /// only before `step` returns `Done`.
    pub fn deliver(&mut self, text: String) -> Event;
    // new
}

/// The outcome of one call to `Run::step`.
pub enum Step {
    Pending {
        effects: Vec<IssuedEffect>,
        events: Vec<Event>,
        withdrawn: Vec<EffectId>,
        /// The main chain waits on its inbox, so only
        /// `Run::deliver` or a cancel can move it.
        waiting_for_input: bool,
    },
    Done {
        result: Result<String, Error>,
        events: Vec<Event>,
        /// The main chain's unread messages, oldest
        /// first: still in its inbox when the run ended.
        unread: Vec<String>,
    },
}
// changed, was: Pending had no `waiting_for_input`,
//   Done had no `unread`
// callers: patterns and builders that name every
//   field; those ending in `..` need no change
// crates/runvil-internal/engine/src/execute/
//   scheduler/drive.rs (3)
// crates/harness-internal/runner/src/effect_loop.rs (2)
// crates/runvil/examples/greeter.rs (2)
// and 9 more files
```

`crates/runvil-internal/types/src/event.rs`

```rust
/// A message entered a chain's inbox, from
/// `Run::deliver` or `tasks.send`, under the receiving
/// chain's provenance. `Run::deliver` returns it; the
/// step that applies a `tasks.send` message emits it.
/// Serialized with kind `inbox_message`; `section`
/// and `provenance` come from `events!`, as on every
/// variant.
InboxMessage {
    text: String,
},
// new
```

`crates/runvil-internal/lua/src/protocol/answer.rs`

```rust
/// What a task's live backing chain is waiting on,
/// as `tasks.status` reports it in `blocked`.
pub enum Blocked {
    Chat,
    ToolCall,
    Store,
    Tasks,
    Call,
    Queued,
    /// Its own inbox, reported as `inbox`.
    Inbox,
}
// changed, was: no `Inbox`
// callers:
// crates/runvil-internal/engine/src/execute/scheduler/
//   chain_record.rs (6)

/// One dispatched request's successful outcome, one
/// variant per request kind.
pub enum Answer {
    // the other variants are unchanged
    /// The message a `receive` request took from the
    /// chain's inbox, as sent.
    Receive(String),
}
// changed, was: no `Receive`
// callers:
// crates/runvil-internal/lua/src/protocol/render.rs (15)
// crates/runvil-internal/engine/src/execute/
//   scheduler/tool_call.rs (20)
// and 13 more files
```

`crates/runvil-internal/lua/src/protocol/request.rs`

```rust
/// A validated suspending Engine call, parsed from the
/// yielded table.
pub enum Request {
    // the other variants are unchanged
    /// `tasks.receive()`: take the oldest message in
    /// the calling chain's inbox, or park the chain
    /// until one arrives. A `call` chain reads its
    /// caller's inbox.
    Receive,
}
// changed, was: no `Receive`
// callers:
// crates/runvil-internal/engine/src/execute/
//   scheduler/dispatch.rs (31)
// crates/runvil-internal/lua/src/protocol/parse.rs (3)
// and 20 more files
```

`crates/runvil-internal/lua/src/__impl_tasks.lua`

```lua
--- Waits for the calling chain's next inbox message
--- and returns its text; on the main chain, the
--- operator's. Raises when given any argument.
function tasks.receive() end
-- new
```

</step-1>

<step-2>

2. [done] **Task inboxes, tasks.send, and models.loop** (points: 10) - Add `tasks.send`, `Event::InboxWaiting` when a chain parks on its empty inbox, and the `Stalled` error, mapped to the `internal` kind in `kind_of` (`crates/runvil-internal/lua/src/error_value.rs` lines 238 to 308), since no Lua code catches it, and added to `ALL` and to the exhaustive `family` match in `crates/runvil-internal/types/src/error-tests.rs` (lines 90 to 160 and 162 to 231). A task waiting on its inbox gives back its slot through step 1's `Parked` case. Replace the scheduler's `Internal` stall error (`crates/runvil-internal/engine/src/execute/scheduler/drive.rs` lines 70 to 78) with the one rule from Stalls. Make `EffectTable::orphan` (`crates/runvil-internal/engine/src/execute/scheduler/effects.rs` lines 234 to 253) list every effect it orphans as withdrawn, not only tool calls, and reword `Step::Pending`'s `withdrawn` doc to match (`crates/runvil-internal/engine/src/execute/run.rs` lines 48 to 58): a step withdraws every effect whose chain stopped waiting on it, and the step that decides the run withdraws every effect still outstanding. Then implement the delivery rules for `models.loop` in `crates/runvil-internal/lua/src/models_loop/machine.rs` and the scheduler's chat arm: withdraw a model request that has no answer yet through `abort_effect` (`crates/runvil-internal/engine/src/execute/scheduler/chain.rs` lines 322 to 332) and answer it with `Answer::Arrivals`, so the loop appends the messages and sends it again, and answer a `chat` request the same way when messages already wait; leave a tool batch alone and append before the next request; prefix each appended message with its arrival line; check the inbox once more before returning, with `Request::Arrivals`; and apply a step's answers before its messages. Tests: the effect table lists every effect it orphans as withdrawn, a timer and a model round beside the tool calls, in id order, where `only_orphaned_tool_calls_are_withdrawn_in_id_order` pinned tool calls only; canceling a task whose model round has no answer withdraws that round, and its `Dropped` answer is discarded; `InboxWaiting` is emitted when the main chain parks on an empty inbox, and not when a message already waits; an owner's send returns true, resumes its task's `tasks.receive()`, and emits `InboxMessage` under the task's provenance; a non-owner sending by id string raises `task_not_owned`; a send to a task that returned returns false and queues nothing; a send whose text is not a string raises a `lua` error; `tasks.status(t).blocked` reads `inbox` while the task waits, and `InboxWaiting` is emitted under its provenance; a waiting task frees its slot, so a queued spawn starts; a main chain joining a task that waits on its inbox, with nothing else to run, ends with `stalled` naming that task; a message queued before the first request is appended before it, under the main chain's arrival line; a message while a model request has no answer withdraws it, raises nothing, leaves nothing of the withdrawn request in `messages`, and the reissued request holds the message, and the withdrawn one counts toward neither `max_tool_iterations` nor the chain's turns; a message while tool calls have no answer withdraws nothing, every call keeps its real result, and the message is appended before the next request; a message while the final request has no answer keeps the loop going; on a task the arrival line is the task's; a message to one task does not withdraw another chain's request; a tool call answered `Dropped` still raises `Canceled` at the call site and leaves the queued message in the inbox; a `Dropped` answer to the model request in flight and a message applied in the same step raise `Canceled` and leave the message queued; two messages queued before one request become two user records, each with its arrival line; the loop's last inbox read before it returns appends a message that reached the inbox after its final reply was answered, and the loop sends again.
   - Score: `tasks.send` 1, `Request` 0 (`Send` and `Arrivals`) and `Answer` 0 (`Send` and `Arrivals`), the wire for `tasks.send` and the loop's inbox reads, `Event` 1 (`InboxWaiting`), `ErrorCode` 1 (`Stalled`). Other changes: the slot give-back through step 1's `Parked` case 0, the one stall rule 1, the pre-request append and final check 1, the orphan path withdrawing every effect 1, reissuing an unanswered request 2, the arrival line 1, answers before messages 1.

`crates/runvil-internal/lua/src/__impl_tasks.lua`

```lua
--- Queues `text` in the inbox of `task`, a Task
--- handle or id, and returns true, or returns false,
--- dropping the text, once `task` has ended. Raises
--- a `lua` error when `text` is not a string, and
--- `task_not_owned` when the caller does not own
--- `task`.
function tasks.send(task, text) end
-- new
```

`crates/runvil-internal/lua/src/protocol/request.rs`

```rust
/// A validated suspending Engine call, parsed from the
/// yielded table.
pub enum Request {
    // the other variants are unchanged
    /// `tasks.send(task, text)`: queue `text` in the
    /// inbox of a task the caller owns, waking it when
    /// it waits there; a task that has ended drops it.
    Send {
        /// The task to send to.
        task: TaskId,
        /// The author-supplied message text.
        text: String,
    },
    /// The `models.loop` step's last read of its
    /// chain's inbox before the loop returns: takes
    /// every queued message at once and never waits.
    /// Shim-produced, so it is never refused.
    Arrivals,
}
// changed, was: no `Send` or `Arrivals`; a `chat`
//   request may now be answered with
//   `Answer::Arrivals`
// callers:
// crates/runvil-internal/engine/src/execute/
//   scheduler/dispatch.rs (37)
// crates/runvil-internal/lua/src/protocol/
//   tests/parse_tasks.rs (32)
// and 46 more files
```

`crates/runvil-internal/lua/src/protocol/answer.rs`

```rust
/// One dispatched request's successful outcome.
pub enum Answer {
    // the other variants are unchanged
    /// Whether a `send` request queued its text: false
    /// once the task has ended.
    Send(bool),
    /// The messages taken from the chain's inbox,
    /// oldest first, as sent: every one queued, maybe
    /// none, for an `arrivals` request; for a `chat`
    /// request, those that reached the inbox before
    /// its round was sent or answered, which the loop
    /// appends before it sends again.
    Arrivals(Vec<String>),
}
// changed, was: no `Send` or `Arrivals`
// callers:
// crates/runvil-internal/engine/src/execute/
//   scheduler/apply.rs (31)
// crates/runvil-internal/engine/src/execute/
//   scheduler/tool_call.rs (23)
// and 49 more files
```

`crates/runvil-internal/types/src/event.rs`

```rust
/// A chain parked in `tasks.receive()` or `tasks.reply`
/// with its inbox empty. Emitted under its provenance
/// in the step where it parks. Serialized with kind
/// `inbox_waiting`.
InboxWaiting {},
// new
```

`crates/runvil-internal/types/src/error.rs`

```rust
pub enum ErrorCode {
    // existing variants unchanged
    /// No chain can move and the main chain is not
    /// waiting on its inbox: the run deadlocked. Wire
    /// name `stalled`; names the tasks waiting on their
    /// inboxes. A run code, beside `Internal`.
    Stalled = 704,
}
// changed, was: no `Stalled`
// callers:
// crates/runvil-internal/lua/src/error_value.rs (1)
// crates/runvil-internal/types/src/error-tests.rs (2)
```

</step-2>

<step-3>

3. [done] **Replies and idle tasks** (points: 7) - First split `EntryState` (`crates/runvil-internal/engine/src/execute/scheduler/tasks.rs` lines 59 to 75) so a task entry's waiting outcome is separate from whether its backing chain is live: a finished task fills the outcome and ends, and waits take the outcome as they do today. Then add `tasks.reply`, which fills the outcome of a live entry and parks the task on its inbox, the idle rules from Replies and idle tasks, and `RunLimits::max_idle_tasks`. The cap closes a task through the same outcome a returned task leaves, so widen `Event::TaskSucceeded`'s doc (`crates/runvil-internal/types/src/event.rs` lines 290 to 295) to a task the run closed while idle, keeping its latest reply. The cap's close emits one `tracing::debug!` line naming the task it closed, which gives `runvil-engine` the workspace's `tracing` dependency (`Cargo.toml` line 119), a crate the Engine's dependency guard allows (`crates/build-xtask/src/engine_deps.rs` lines 37 to 45); refresh `Cargo.lock` so the `--locked` commands pass. Tests: a wait returns the reply; after a send, a second wait returns the next reply; a wait on an idle task whose reply was delivered, with no message since, raises `task_consumed`; an idle task's status reads `running`, `blocked` `inbox`, no `ok`, and its `section`; an idle task frees its slot; an owner that ends with an idle task keeps its own result; `tasks.cancel` on an idle task ends it canceled; `tasks.reply` on the main chain raises, and in a `call` child of a task replies for that task; with `max_idle_tasks` 1, a second task's reply closes the first: its status reads `done` with `ok` true, `TaskSucceeded` reports it under its own provenance, no `TaskAbandoned` fires, and the owner's next `tasks.send` to it returns false; a wait on the first delivers its undelivered reply once, and the next wait raises `task_consumed`; `tasks.ready` is true while a reply waits undelivered and false after a wait takes it; `tasks.pending()` lists an idle task; `tasks.reply()` publishes `""`.
   - Score: `tasks.reply` 1, with its wire, `Request` 0 (`Reply`) and `Answer` 0 (`Reply`), `RunLimits` 1 (`max_idle_tasks`). Other changes: separating an entry's outcome from its liveness 1, the idle lifecycle with owner end and the cap's close 3, delivery once per reply over the outcome 1, the cap's debug line 0.

`crates/runvil-internal/lua/src/__impl_tasks.lua`

```lua
--- Hands `value` to the task's owner as its latest
--- result, as a section's return value would be,
--- with `nil` handed over as `""`, then waits for the
--- next inbox message and returns its text. Raises
--- on the main chain.
function tasks.reply(value) end
-- new
```

`crates/runvil-internal/lua/src/protocol/request.rs`

```rust
/// A validated suspending Engine call, parsed from the
/// yielded table.
pub enum Request {
    // the other variants are unchanged
    /// `tasks.reply(value)`: publish `text` as the
    /// calling task's latest result, idle the task,
    /// and park it on its inbox until its owner's next
    /// message. A `call` chain replies for its
    /// caller's task.
    Reply {
        /// The reply, converted as a section's return
        /// value is, `""` for nil.
        text: String,
    },
}
// changed, was: no `Reply`
// callers:
// crates/runvil-internal/engine/src/execute/
//   scheduler/dispatch.rs (31)
// crates/runvil-internal/lua/src/protocol/parse.rs (3)
// and 20 more files
```

`crates/runvil-internal/lua/src/protocol/answer.rs`

```rust
/// One dispatched request's successful outcome.
pub enum Answer {
    // the other variants are unchanged
    /// The owner's next message, which resumes a
    /// `reply` request, as sent.
    Reply(String),
}
// changed, was: no `Reply`
// callers:
// crates/runvil-internal/lua/src/protocol/render.rs (15)
// crates/runvil-internal/engine/src/execute/
//   scheduler/tool_call.rs (20)
// and 13 more files
```

`crates/runvil-internal/engine/src/execute/params.rs`

```rust
pub struct RunLimits {
    // the other fields are unchanged
    /// The most tasks the run holds idle after a
    /// reply. One more reply closes the task idle
    /// longest, its latest reply its result.
    /// Defaults to 8.
    pub max_idle_tasks: NonZeroUsize,
}
// changed, was: no `max_idle_tasks`
// callers: only the `Default` impl names every
//   field; the rest use `..RunLimits::default()`
// crates/runvil-internal/engine/src/execute/params.rs
//   (2)
// crates/runvil-internal/engine/src/execute/context.rs
//   (1)
// and 3 more files
```

</step-3>

<step-4>

4. **RunControl and the user Pack** (points: 8) - Add `RunControl::send` and `RunReport::unread`, and give the effect loop a fourth wait arm: it calls `Run::deliver` for each sent message, records the event it returns, and steps again; it handles a stop before any send that follows it; and it waits for a send instead of ending `STALLED` when `waiting_for_input` is true. `RunReport::unread` is `Step::Done`'s `unread` followed by every message `RunControl` accepted but never delivered, oldest first. Add `RunSetup.has_user` and the built-in `runvil/user`. When `has_user` is false, the runner's `Packs::snapshot` marks `runvil/user` unavailable to the run with the reason "this run has no user" instead of making it, the way it treats a Pack whose factory refuses (`crates/harness-internal/runner/src/packs.rs` lines 123 to 202). Rewrite the built-in invariants above `BUILTIN` (`crates/runvil-plugin/src/registry.rs` lines 228 to 238), which say a built-in never refuses a run: `runvil/user` is unavailable in a run without a user. `user` is not a reserved Pack id (`crates/runvil-internal/types/src/packs.rs` line 28). A second built-in changes what a fresh registry lists and what every run's roster holds, so update the tests that assume `runvil/subagents` is the only one: the full lists in `crates/runvil-plugin/src/registry-tests.rs` (lines 159 to 167 and 192 to 202); the full rosters in `crates/harness-internal/runner/src/packs-tests.rs` (lines 312 to 320 and 588 to 600) and `packs-tests-pick.rs` (lines 138 to 153), which gain `user` as unavailable in a run with no user; and the install helpers that skip the built-in by its FQN, which must skip both, in `crates/plugin-std/tests/it/tools.rs` (lines 36 and 53), `crates/plugin-web/src/web-tests.rs` (line 70), `crates/plugin-mcp/tests/it/support.rs` (line 48), whose `pop` would otherwise take the `runvil/user` factory, and `crates/plugin-user-input/tests/it/ask.rs` (line 65). Update the docs that name `runvil/subagents` as the one built-in: `crates/harness-internal/runner/src/lib.rs` lines 42 to 46, `crates/runvil-plugin/src/lib.rs` lines 29 to 30, and `crates/runvil-plugin/src/registry.rs` lines 51 to 52. Tests: a send while waiting resumes the run; a send before the run starts is delivered before its first step; a send after the run ends returns false; the recorder holds a sent message's `InboxMessage` event ahead of the records of the step that applies it; a run waiting for input with nothing in flight does not end `STALLED`; a cancel while waiting ends the run canceled; a `stop_round` during a tool call leaves a message sent before it queued, and the next `tasks.receive()` returns it; a `stop_round` and then a `send` while a model request is in flight raise `Canceled` at `models.loop`, and the next `tasks.receive()` returns the message; a run that ends with messages sent and never read reports them in `RunReport::unread`; a prompt declaring `user` runs in a run with a user and is refused at the first step without one, naming "this run has no user"; `tools.packs.get('user')` returns a Pack object in a run with a user and nil in one without.
   - Score: `RunControl::send` 1, `RunReport` 1 (`unread`), `RunSetup` 1 (`has_user`), `BUILTIN` 1 (`runvil/user`). Other changes: the effect loop's fourth wait arm and its stall rule 3, the runner making `runvil/user` unavailable without a user 1, the tests and docs that assume one built-in 0.

`crates/harness-internal/runner/src/harness.rs`

```rust
impl RunControl {
    /// Sends `text` to the run's main-chain inbox and
    /// returns true, or returns false, dropping the
    /// text, once the run has ended. Text sent before
    /// the run starts is delivered before its first
    /// step. True means accepted, not read: text the
    /// run never reads comes back on
    /// `RunReport::unread`.
    #[must_use]
    pub fn send(&self, text: String) -> bool;
    // new
}

/// How one run ended, and what it left at its
/// declared output file.
pub struct RunReport {
    pub run_id: RunId,
    pub result: Result<String, Error>,
    pub output: Result<String, runvil::Error>,
    /// Messages sent to the main chain that the run
    /// never read, oldest first.
    pub unread: Vec<String>,
}
// changed, was: no `unread`
// callers:
// crates/harness-internal/runner/src/harness.rs (3)
```

`crates/harness-internal/runner/src/setup.rs`

```rust
pub struct RunSetup {
    // the other fields are unchanged
    /// A user answers the main chain's inbox, so the
    /// run holds `runvil/user`, which is unavailable
    /// otherwise.
    pub has_user: bool,
}
// changed, was: no `has_user`; `RunSetup::new`
//   (recorder, broker) leaves it false
// callers: none outside `RunSetup::new`, the one
//   literal that names every field; the other 29
//   literals, in 20 files, end in `..`
```

`crates/runvil-plugin/src/registry.rs`

```rust
/// The Packs every registry holds from the start:
/// `runvil/subagents`, and `runvil/user`, which marks
/// a prompt that needs a human. Neither offers tools
/// or has a prelude. A run holds `runvil/user` only
/// when its setup has a user.
const BUILTIN: [PackInfo; 2];
// changed, was: [PackInfo; 1], only subagents
// callers:
// crates/runvil-plugin/src/registry.rs (1)
// crates/runvil-plugin/src/registry-tests.rs (2)
```

</step-4>

<step-5>

5. **Guide: the user Pack and reading the operator** (points: 6) - In `runvil-docs`, replace the `user-input` Pack, `input.ask()`, its ask tool, the input broker, and host services with the `runvil/user` Pack and `tasks.receive()` on the main chain. In `src/language/05-lua-environment.md`: rewrite `## Asking the operator` (lines 2391 to 2539) for `uses: [user]`, where the main chain reads the operator's messages with `tasks.receive()`, queued messages return at once, oldest first, a stop leaves the wait open, a run without a user refuses the prompt at its first step with `this run has no user`, and a prompt that works either way checks `tools.packs.get('user')`; delete `### The input broker`, `### The ask tool`, `### Letting the model ask`, and the broker failures in `### When an ask fails`; move the `user-input` examples in `### Pack preludes` (lines 269 to 336) and `` ### The bound model in `sys.model` `` (lines 1892 to 1939) to other Packs and tools, with a made-up Pack for the prelude, since no shipped Pack has one once `user-input` goes; and drop `input.ask` from `### Suspending calls` (line 241) and from `### Passing Engine functions to pcall` (line 2806). In `src/language/04-how-a-prompt-runs.md`: replace `### Host services` (lines 371 to 386) with the `user` Pack's unavailable case, add `- user is unavailable: this run has no user` to `### Unavailable Packs` (line 388), rewrite the requirements notice example (lines 521 to 558) for `uses: [web, user]`, drop every clause about a Pack lacking a service (lines 245 to 258 and 344 to 608), and fix `### The work the Harness does` (lines 108 to 120), where a wait for the operator is no longer a tool call, and the `tools.packs.extras()` example (line 367). In `src/language/12-tools.md`: move every example that uses `user-input` as its sample Pack or `user-input/ask` as its sample tool to `web` or an adhoc tool, fix the prose that names the run's own services, the input broker, or `input.ask` (lines 52, 1154, 1925, 2494, and 2515), delete the `user-input_ask` wire-name row (line 296), and rewrite the example in `## Pack preludes` (lines 2625 to 2847) around a made-up Pack whose prelude helper calls its own Pack's tool, the pattern lines 2668, 2717, and 2773 describe. Fix the wording in `src/language/01-what-a-prompt-is.md` (lines 446 to 448 and 588), `02-file-structure.md` (line 564), `09-the-store.md` (lines 1924 and 2135), `13-web-fetch-and-search.md` (line 90), `15-tasks.md` (line 201, which says a task can call `input.ask()`, and lines 1338 and 1344, which say a wait for the operator is a `tool_call`), `16-limits-and-errors.md` (line 1525, which says a stop leaves a question open), and `17-quick-reference.md` (the `input.ask` row at line 75 and the `user-input/ask` row at line 258, which go, and the Pack tools intro at line 254). Tests: in `runvil-docs`, `rg -i -c "user-input|input-broker|input broker|host service" src` finds nothing, where it finds 111 lines in 7 files today, and `rg -c "input\.ask" src` finds nothing, where it finds 58 lines in 8 files today; `cargo xtask site --books-only` builds and passes its link check; `cargo test -p build-user-guide` passes.
   - Score: the `## Asking the operator` rewrite 3, chapter 04's Pack availability and notice examples 1, chapter 12's sample Pack and prelude section 1, removing `input.ask` from the other chapters 1.

</step-5>

<step-6>

6. **Guide: messages between tasks, replies, the loop, and stalls** (points: 9) - In `runvil-docs`, document `tasks.send`, `tasks.receive` in a task, `tasks.reply` and idle tasks, how `models.loop` reads the inbox, the `inbox_message` and `inbox_waiting` events, `stalled`, and `max_idle_tasks`. Line numbers here are the guide's before step 5. In `src/language/15-tasks.md`: add a `## Messages between tasks` section between `## Canceling and listing tasks` (line 739) and `## Ownership, leaked tasks, and abandonment` (line 884) for `tasks.send` and its false return, `tasks.receive`, `tasks.reply`, idle tasks, delivery once per reply, the idle cap, which closes the task idle longest as `done` with its latest reply, the `inbox_message` and `inbox_waiting` events, and the stall; then update `### The tasks namespace` (lines 49 to 93, twelve functions), the waits and `task_consumed` (lines 535 to 690), `### What tasks.cancel does` (lines 774 to 804), `### Listing live tasks with tasks.pending` (lines 829 to 841), ownership and abandonment (lines 884 to 1143, where an owner that ends with idle tasks abandons them without `tasks_live`, and the five abandon reasons stay five), the status tables (lines 1226 to 1346, where `blocked` gains `inbox` and an idle task reads `running`), `tasks.ready` (lines 1404 to 1416), admission and slots (lines 1945 to 1985), `### Terminal task events` (lines 2099 to 2123, where `TaskSucceeded` also reports a task the cap closed), and the argument and refusal tables (lines 2124 to 2184). In `src/language/11-conversations.md`: add a section after `### What each round sends` (line 1187) on how the loop reads the inbox: the arrival lines, the withdrawn and reissued request, the tool batch that finishes first, and the separate user records the request joins; and at line 235, where the Workshop chat prompt calls `messages:user(text)` with each chat turn's text, make it `messages:user(tasks.receive())`. In `src/language/16-limits-and-errors.md`: add `stalled` to the code family table and `### Run codes` (lines 880 to 1014), to the located-failure list in `### Where a run error's message points` (lines 692 to 720), and to `### Failures that are never retryable` (lines 1349 to 1368), add `max_idle_tasks` to the run limits (lines 33 to 149), and say that a stop leaves inboxes alone (lines 1445 to 1525). In `src/language/17-quick-reference.md`: change the rows whose first cell is `kind = "tasks_live"`, `status.blocked`, `tasks.join`, `tasks.join_any`, `tasks.ready`, and `Run codes (7xx)`, which an idle task, the `inbox` wait, or `stalled` makes wrong, and add rows for `tasks.receive`, `tasks.reply`, `tasks.send`, and `max_idle_tasks`. Add the rule that a `call` child uses its caller's inbox to `### How a called chain runs` in `src/language/08-jump-and-call.md` (line 1093). Tests: `rg -c "tasks\.receive" src/language` finds chapters 05, 11, 15, and 17, among others; `rg -c "stalled" src/language/16-limits-and-errors.md` finds the run code and its lists; `cargo xtask site --books-only` builds and passes its link check; `cargo test -p build-user-guide` passes.
   - Score: the messages-between-tasks section 3, threading it through chapter 15's tables and rules 1, the loop's inbox section and the two events 3, `stalled` and the limit in chapter 16 1, the quick reference 1, the `call` child rule in chapter 08 0.

</step-6>

<step-7>

7. **Workshop on RunControl, and the user-input Plugin removed** (points: 10) - Port the Workshop's conversations, agent socket, wire frames, and UI to `RunControl::send` and the inbox events, with `RunSetup.has_user` true for every conversation. Conversations stop providing run services. When `send_input` returns true, the socket pushes the status update a dispatched turn pushes today; when it returns false, the socket logs at debug, as it does today for a response to a dead wait (`crates/workshop/server/src/agents/socket.rs` lines 356 to 370). `AgentEvent::from_event` (`crates/workshop/server/src/agents/wire.rs` lines 237 and 280 to 290) builds `user_message` from `Event::InboxMessage` on the main chain instead of from the ask tool's `ToolResult`, and `awaiting_input` from `Event::InboxWaiting` on the main chain; the `AskTool` match goes. Delete `crates/workshop/agents/src/input.rs`, `input-tool.rs`, and `input-tests.rs`, with the `mod input` and `pub use input::...` lines in `crates/workshop/agents/src/lib.rs` (lines 42 and 52). In the chat agent (`crates/workshop/agents/agents/chat.md`), replace `user-input` with `user` in its `uses:` list (lines 5 to 8), read the operator with `messages:user(tasks.receive())` where it calls `input.ask()` (lines 56 to 57), and drop its `tools.offer("user-input")` (line 21). In the UI the chat box is always editable: while the agent waits, send answers it; while it works, an empty box shows stop and a non-empty box shows send, and the message steers the running turn, so a reply still streaming is cut and asked again with it, and running tools finish first. Until the agent first waits or works, send stays disabled, so no message reaches a conversation whose run has no Harness yet. The UI service stops showing the agent as working at `awaiting_input`, where today the `input_required` frame does that, and shows it working again at the next `user_message`, so a replayed log ends in the right state (`crates/workshop/ui/src/services/agent-session.ts` lines 272 to 296); an `agent_message` no longer ends the working state (lines 445 to 447), and `respond` and the `tool_call_update` case stop reading open waits (lines 360 to 378 and 481 to 490). The view no longer gates the box, the mic, Try again, or send on a pending wait (`crates/workshop/ui/src/parts/agent/agent-session-view.ts` lines 183, 212, 279 to 291, 399, 412, and 432). When a run ends, the conversation logs how many messages its `RunReport::unread` holds. Remove `AgentSessions::deliver_input_after_acceptance_for_test` (`crates/workshop/server/src/agents.rs` lines 426 to 437), since the inbox has no seam between a wait's acceptance and its resumption; its caller (`crates/workshop/server/tests/it/chat_gate/recovery.rs` lines 116 to 133) empties the catalog and then sends. `AskTool` (`crates/workshop/server/src/agents/wire.rs` lines 220 to 229) goes, with the ask value the launcher threads through `AgentSessions`, the socket, `socket_frames.rs`, and `AgentEventFrame::new`, as do `input_frame` (`socket_frames.rs` lines 15 to 22) and `wire-tests-ask.rs` with its `mod` lines (`wire-tests.rs` lines 18 to 19). Port the Workshop tests that wait on `input_required`, echo a token, or run `input.ask()` prompts to `awaiting_input` and an `input_response` with no token: the echo agent and the shared helpers in `crates/workshop/server/tests/it/agents.rs` (lines 31 to 57 and 139 to 222), the tests in `agents/lifecycle.rs`, `agents/turns.rs`, `agents/replacement.rs`, `agents/refusals.rs`, `agents/revoke.rs`, `chat_gate.rs`, `chat_gate/compaction.rs`, `chat_gate/canonical_sequence.rs`, and `chat_gate/recovery.rs` beside it, `crates/workshop/server/src/agents/tests.rs`, `wire-tests.rs`, and `socket_frames-tests.rs`, `crates/workshop/agents/tests/it/conversation.rs` and `support.rs`, `crates/workshop/protocol/tests/it/frames.rs`, the shared frame fixture `crates/workshop/server/tests/fixtures/agent-frames.json` (lines 61 to 63) with `crates/workshop/ui/test/agent-wire-fixtures.mjs`, and `agent-socket.mjs`, `agent-session-service.mjs`, `agent-session-view.mjs`, `agent-stt.mjs`, and `agent-stt-boot.mjs` in `crates/workshop/ui/test`. Then remove `RunSetup.services` and `new_pack`'s `run` parameter, porting every factory function and closure, the `Make` aliases (`crates/harness-internal/runner/tests/it/support.rs` line 60, and `crates/harness-internal/runner/src/packs-tests.rs` line 85, whose four-argument makers drop the run's services), and the three `PackFactory` impls, and rework `crates/runvil-plugin/tests/it/pack_info.rs`, whose `Greetings` factory takes a visitor from run services (lines 61 to 84 and 160 to 175). Delete `crates/plugin-user-input`, its `[workspace.dependencies]` entry (`Cargo.toml` line 40; the `crates/*` glob on line 3 makes it a member), the dependency lines in `crates/workshop/agents/Cargo.toml` (line 18) and `crates/workshop/server/Cargo.toml` (line 24), the Workshop's `install` call for it, the whole `let ask` statement (`crates/workshop/server/src/agents.rs` lines 159 to 171), the test install in `crates/workshop/agents/tests/it/support.rs` (line 68), and the crate's entry in `PLUGIN_DOC_DIRS` (`crates/workshop/ui/test/docs-claims.mjs` line 164), whose walk fails on a missing directory. Refresh `Cargo.lock` with `cargo update --workspace`, which drops the crate and upgrades nothing, so the `--locked` commands pass. Delete the test fixtures `crates/harness/tests/suite/asker.rs` and `crates/harness-internal/runner/tests/it/asker.rs`, which copy the ask tool, set `survives_stop`, and take a broker from run services, with their `mod asker;` lines (`crates/harness/tests/suite/main.rs` line 4 and `crates/harness-internal/runner/tests/it/main.rs` line 11). Delete `crates/harness-internal/runner/tests/it/prepare-input.rs`, whose tests cover the broker, run services, the ask tool, and `input.ask`, and `prepare-host-services.rs`, whose tests cover run services, with their `mod` lines in `prepare.rs` (lines 45 to 48) and that module doc's account of them (lines 9 to 13); step 4's refusal test covers the one that stays meaningful for the `user` Pack. Port the tests that use the fixtures, in `crates/harness/tests/suite/host.rs` and `vfs.rs` and in the runner's `scripted.rs`, `harness-stop.rs`, `harness-stop-cap.rs`, and `harness-stop-timing.rs`. A test that reads the operator moves to a `uses: [user]` prompt that reads `tasks.receive()`, answered through `RunControl::send`. A test that uses the ask tool as a call that survives stops uses the hold fixture with `survives_stop` set instead (`scripted.rs` lines 247 to 263), whose registry stops installing the asker (line 260). Sweep every doc and comment that describes the input broker, run services, the ask tool, wait tokens, or the `input` table. Outside the Workshop, these are: the runner's invariants (`crates/harness-internal/runner/src/lib.rs` lines 51 to 54); in the runner's `crates/harness-internal/runner/src/packs.rs`, its module doc (lines 14 to 15), the docs of `Packs::snapshot` (lines 124 to 126) and `Packs::make` (lines 175 to 183, which say a Host with no operator refuses `runvil/user-input` on every run), and the `new_pack` helper's doc (lines 280 to 281); the runner's `crates/harness-internal/runner/src/prepare.rs` (lines 11 and 114); `Harness::new`'s docs (`crates/harness-internal/runner/src/harness.rs` lines 105 to 108) and `RunControl::stop_round`'s (lines 270 to 272), which name the question to the operator as a call that survives a stop; `RunSetup::new`'s doc (`crates/harness-internal/runner/src/setup.rs` line 50); the harness facade docs (`crates/harness/src/lib.rs` lines 10 to 17); the plugin crate docs (`crates/runvil-plugin/src/lib.rs` lines 35 to 38 and 67 to 70); `Pack::call`'s and `new_pack`'s docs (`crates/runvil-plugin/src/pack.rs` lines 74 to 75 and 147 to 158); `TestCall`'s doc (`crates/runvil-plugin/src/testing.rs` lines 13 to 15); the services module docs (`crates/runvil-plugin/src/service.rs` lines 1 to 11); the Lua crate docs (`crates/runvil-internal/lua/src/lib.rs` lines 36 to 38); `ToolInfo::survives_stop`'s doc (`crates/runvil-internal/types/src/tools/info.rs` lines 58 to 59); and the `vendor` example in `crates/runvil-internal/types/src/metrics.rs` (line 39). In the Workshop, these are: the crate docs and invariants (`crates/workshop/agents/src/lib.rs` lines 3, 10, 24, and 34 to 35; `crates/workshop/server/src/lib.rs` lines 35 and 58 to 59; `crates/workshop/protocol/src/lib.rs` lines 46 to 56); `crates/workshop/server/src/agents.rs` (lines 9 to 19, 85, 147 to 153, 187, and 403), `crates/workshop/server/src/agents/socket.rs` (lines 9, 14, 134, 159, 364, 445, and 503), and `crates/workshop/server/src/agents/wire.rs` (line 14); and `crates/workshop/agents/src/conversation.rs` (lines 10, 224, and 234) and `conversation-run.rs` (lines 1 to 5). Tests: a message sent while the agent waits answers it and appears as a `user_message`; a message sent while the agent works appears as a `user_message`, and a tool call running when it was sent finishes before the model request that holds it; the main chain parking on its empty inbox appears as `awaiting_input`, and a reattached client sees it in the replayed log; a message sent before the run is handed its Harness returns false and reaches no run; the UI service sends `input_response` with no token, stops showing the agent as working at `awaiting_input`, shows it working again at the next `user_message`, and keeps showing it working through an `agent_message`; the chat box stays editable while the agent works, showing stop when empty and send when it holds text, and keeps send disabled until the agent first waits or works; a plain function of the Pack id alone is a `PackFactory`; a run makes its Packs with no run services; a stop while the main chain waits on its inbox leaves the wait, and a message sent after it reaches `tasks.receive()`.
   - Score: `Conversation::send_input` 1, `AgentEventKind` 1 (`AwaitingInput`), `InputResponse` 1, TypeScript `AgentEventKind` 1 (`awaiting_input`), TypeScript `InputResponseFrame` 0, `AgentSocket.respond` 0, `AgentSessionWire.respond` 0, removed `Conversation::unresolved_waits`, `Conversation::subscribe_waits`, `Conversation::run_services`, `WaitRegistry`, `WaitFrame`, `WaitError`, `complete_input_response`, `SessionInputBroker`, `AgentSessions::unresolved_waits`, `AgentSessions::deliver_input_after_acceptance_for_test`, `InputFrame`, `InputRequiredFrame`, `InputCanceledFrame`, `AgentSocket.onInputRequired`, `AgentSocket.onInputCanceled`, `AgentSessionWire.onInputRequired`, `AgentSessionWire.onInputCanceled`, `pendingInputToken`, and `onDidChangePendingInput` 0, `RunSetup` 1 (`services` removed), `PackFactory` and its blanket impl 1, removed `install`, `ASK`, `INPUT_BROKER`, and `InputBroker` 0. Other changes: the `AgentEvent::from_event` mapping 1, the chat box actions and the working state 1, porting the tests that used the ask fixtures 1, porting the Workshop's server, agents, protocol, and UI tests and their shared helpers 1, the chat agent, the unread count log, porting the factory functions, and the doc sweep 0.

`crates/workshop/agents/src/conversation.rs`

```rust
impl Conversation {
    /// Sends the operator's `text` to the run and
    /// returns true, or returns false, dropping the
    /// text, before the run is handed its Harness
    /// and once the run has ended.
    pub fn send_input(&self, text: String) -> bool;
    // changed, was: send_input(&self, token: &str,
    //   text: String, before_resume: impl FnOnce())
    //   -> Result<(), WaitError>
    // callers:
    // crates/workshop/server/src/agents/socket.rs (1)
    // crates/workshop/server/src/agents.rs (1)
    // crates/workshop/agents/tests/it/conversation.rs
    //   (1)

    pub fn unresolved_waits(&self) -> Vec<String>;
    // removed
    // callers:
    // crates/workshop/agents/tests/it/conversation.rs
    //   (2)
    // crates/workshop/server/src/agents.rs (1)
    // crates/workshop/server/src/agents/socket.rs (1)

    pub fn subscribe_waits(
        &self,
    ) -> broadcast::Receiver<WaitFrame>;
    // removed
    // callers:
    // crates/workshop/agents/tests/it/conversation.rs
    //   (2)
    // crates/workshop/server/src/agents/socket.rs (1)

    pub fn run_services(&self) -> HostServices;
    // removed
    // callers:
    // crates/workshop/agents/tests/it/support.rs (1)
    // crates/workshop/server/src/agents.rs (1)
    // crates/workshop/server/src/agents/tests.rs (1)
}
```

`crates/workshop/agents/src/input.rs`

```rust
pub struct WaitRegistry;
// removed
// callers:
// crates/workshop/agents/src/input-tests.rs (8)
// crates/workshop/agents/src/input-tool.rs (3)
// crates/workshop/agents/src/conversation.rs (2)

pub enum WaitFrame;
// removed
// callers:
// crates/workshop/agents/tests/it/conversation.rs
//   (5)
// crates/workshop/server/src/agents/socket.rs (5)
// crates/workshop/agents/src/input-tool.rs (5)
// and 4 more files

pub enum WaitError;
// removed
// callers:
// crates/workshop/agents/src/input-tests.rs (4)
// crates/workshop/agents/src/conversation.rs (1)
// crates/workshop/server/src/agents.rs (1)
// and 1 more file

pub fn complete_input_response(
    registry: &WaitRegistry,
    token: &str,
    text: String,
    before_completion: impl FnOnce(),
) -> Result<(), WaitError>;
// removed
// callers:
// crates/workshop/agents/src/input-tests.rs (2)
// crates/workshop/agents/src/conversation.rs (1)
```

`crates/workshop/agents/src/input-tool.rs`

```rust
pub struct SessionInputBroker;
// removed
// callers:
// crates/workshop/agents/src/input-tests.rs (2)
// crates/workshop/agents/src/conversation-run.rs (1)
```

`crates/workshop/server/src/agents.rs`

```rust
impl AgentSessions {
    pub fn unresolved_waits(
        &self,
        id: &str,
    ) -> Option<Vec<String>>;
    // removed
    // callers:
    // crates/workshop/server/tests/it/agents/
    //   lifecycle.rs (3)
    // crates/workshop/server/tests/it/agents/turns.rs
    //   (2)

    #[cfg(feature = "test-fixtures")]
    pub fn deliver_input_after_acceptance_for_test(
        &self,
        id: &str,
        response: InputResponse,
        after_acceptance: impl FnOnce(),
    ) -> Option<Result<(), WaitError>>;
    // removed: the inbox has no seam between a
    //   wait's acceptance and its resumption
    // callers:
    // crates/workshop/server/tests/it/chat_gate/
    //   recovery.rs (1)
}
```

`crates/workshop/server/src/agents/wire.rs`

```rust
/// The kind of one agent event, named as the Agent
/// Client Protocol's `sessionUpdate` kinds are.
enum AgentEventKind {
    // the other variants are unchanged
    /// The agent waits on its empty inbox for the
    /// operator's next message.
    #[serde(rename = "awaiting_input")]
    AwaitingInput,
}
// changed, was: no `AwaitingInput`
// callers:
// crates/workshop/server/src/agents/wire.rs (7)
```

`crates/workshop/protocol/src/input.rs`

```rust
pub enum InputFrame;
// removed
// callers:
// crates/workshop/server/src/agents/socket_frames.rs (3)
// crates/workshop/protocol/tests/it/frames.rs (2)
// crates/workshop/server/src/agents/wire-tests.rs (2)
// and 2 more files

/// The operator's message, sent at any time. Wire
/// type `input_response`.
pub struct InputResponse {
    pub text: String,
}
// changed, was: also `pub token: String`
// callers:
// crates/workshop/protocol/tests/it/frames.rs (1)
// crates/workshop/server/src/agents.rs (1)
// crates/workshop/server/src/agents/wire-tests.rs (1)
// and 2 more files
```

`crates/workshop/ui/src/services/protocol.ts`

```ts
/** The kind of one agent event. `awaiting_input`:
 *  the agent waits for the operator's message. */
export type AgentEventKind =
  | "agent_message"
  | "tool_call"
  | "tool_call_update"
  | "agent_thought"
  | "user_message"
  | "awaiting_input";
// changed, was: no "awaiting_input"
// callers:
// crates/workshop/ui/src/services/protocol.ts (1)

/** The operator's message, sent at any time. */
export interface InputResponseFrame {
  type: "input_response";
  text: string;
}
// changed, was: also `token: string`
// callers:
// crates/workshop/ui/src/services/agent-socket.ts (1)

export interface InputRequiredFrame;
// removed
export interface InputCanceledFrame;
// removed
// callers:
// crates/workshop/ui/src/services/protocol.ts (2)
```

`crates/workshop/ui/src/services/agent-socket.ts`

```ts
export class AgentSocket {
  /** Sends the operator's message; false when the
   *  socket is not open. */
  respond(text: string): boolean;
  // changed, was: respond(token: string,
  //   text: string)
  // callers:
  // crates/workshop/ui/src/services/agent-session.ts (1)
  // crates/workshop/ui/test/agent-wire-fixtures.mjs (1)
  // crates/workshop/ui/test/agent-socket.mjs (1)

  readonly onInputRequired: Event<string>;
  // removed
  readonly onInputCanceled: Event<string>;
  // removed
  // callers:
  // crates/workshop/ui/test/agent-wire-fixtures.mjs (2)
  // crates/workshop/ui/test/agent-socket.mjs (1)
}
```

`crates/workshop/ui/src/services/agent-session.ts`

```ts
export interface AgentSessionWire {
  respond(text: string): boolean;
  // changed, was: respond(token: string,
  //   text: string)
  // callers, and the test wires that implement it:
  // crates/workshop/ui/src/services/agent-session.ts (1)
  // crates/workshop/ui/test/agent-session-service.mjs
  //   (1)
  // crates/workshop/ui/test/agent-session-view.mjs (1)
  // and 1 more file

  readonly onInputRequired: Event<string>;
  // removed
  readonly onInputCanceled: Event<string>;
  // removed
  // callers:
  // crates/workshop/ui/src/services/agent-session.ts (2)
  // crates/workshop/ui/test/agent-session-service.mjs
  //   (2)
  // crates/workshop/ui/test/agent-session-view.mjs (2)
  // and 1 more file
}

export class AgentSessionService {
  get pendingInputToken(): string | null;
  // removed
  // callers:
  // crates/workshop/ui/test/agent-session-service.mjs (18)
  // crates/workshop/ui/src/parts/agent/
  //   agent-session-view.ts (5)
  // crates/workshop/ui/src/services/agent-session.ts (1)

  readonly onDidChangePendingInput: Event<
    string | null
  >;
  // removed
  // callers:
  // crates/workshop/ui/src/parts/agent/
  //   agent-session-view.ts (1)
  // crates/workshop/ui/test/agent-session-service.mjs (1)
}
```

`crates/harness-internal/runner/src/setup.rs`

```rust
pub struct RunSetup {
    // the other fields are unchanged
}
// changed, was: also `pub services: HostServices`
// callers:
// crates/harness-internal/runner/src/prepare.rs (1)
// crates/workshop/server/src/agents.rs (1)
// crates/harness/tests/suite/host.rs (1)
// and 9 more files
```

`crates/runvil-plugin/src/pack.rs`

```rust
/// Makes each run's instance of one installed Pack.
pub trait PackFactory: Send + Sync {
    /// Makes this run's instance named `id`. Runs
    /// inside the run's future, so it must not
    /// block, do I/O, or panic.
    fn new_pack(
        &self,
        id: &PackId,
    ) -> Result<Arc<dyn Pack>, Error>;
}
// changed, was: new_pack(&self, id: &PackId,
//   run: &HostServices)
// callers:
// crates/harness-internal/runner/src/packs.rs (1)
// crates/runvil-plugin/tests/it/pack_info.rs (2)
// crates/runvil-plugin/src/registry-tests.rs (1)
// and 5 more files
// impls, which drop `run` too:
// crates/plugin-web/src/web.rs (1)
// crates/plugin-mcp/src/lib.rs (1)
// crates/runvil-plugin/tests/it/pack_info.rs (1)

/// A plain function or closure with `new_pack`'s
/// parameters and return type is a factory.
impl<Func> PackFactory for Func
where
    Func: Fn(&PackId) -> Result<Arc<dyn Pack>, Error>
        + Send
        + Sync;
// changed, was: Fn(&PackId, &HostServices)
// callers: 20 factory functions and closures in
//   15 files, among
// crates/harness-internal/runner/src/packs-tests.rs (4)
// crates/harness-internal/runner/src/host-tests.rs (2)
// crates/harness-internal/runner/tests/it/prepare.rs
//   (2)
// and 12 more files
```

`crates/plugin-user-input/src/lib.rs`

```rust
pub fn install(
    packs: &mut PackRegistry,
    config: Value,
    services: &HostServices,
) -> Result<(), Error>;
// removed
pub const ASK: &str;
// removed
pub const INPUT_BROKER: ServiceKey<dyn InputBroker>;
// removed
pub trait InputBroker: Send + Sync;
// removed
// callers:
// crates/workshop/server/src/agents.rs (2)
// crates/workshop/agents/tests/it/support.rs (1)
// crates/workshop/agents/Cargo.toml (1)
// and 2 more files
```

</step-7>
