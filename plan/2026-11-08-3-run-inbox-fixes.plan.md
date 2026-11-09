---
name: Run Inbox Fixes
overview: "Fix the problems the Run Inbox and User Pack change left: record order and task inbox events, the idle cap, the spent round cap, replied fanout arms, the empty stall, the Harness stop race and reissued rounds, the Workshop's end of run, refused messages, and steering in the transcript, plus the Rust docs and the language guide."
todos: []
isProject: false
---

<plan-context>

# Run Inbox Review Fixes

The Run Inbox and User Pack change works, but it left problems at its edges and in its docs. This plan fixes them. In the Engine:
- A message is recorded after the model reply that came before it.
- A task's inbox events wait for the task to start.
- The idle cap stops ending another task's work when it can avoid it.
- A spent round cap leaves a late message queued.
- A fanout arm that replies no longer outlives its call.
- A stall only the Engine can cause is `internal` again.

The Harness acts on a stop before every step, and it drops a round reissued for a message once it sees a stop. The Workshop:
- Frames the end of a run.
- Refuses messages out loud.
- Keeps tool results with their calls in a steered conversation.
- Drops a reply that a message cut off.
- Keeps Stop working after a queued message starts.

The language guide and the Rust docs say what the code does.

## Notes

- Repository: `c:\Users\Vinnie\cursor\runvil`, a Rust workspace on Windows (PowerShell) with a TypeScript UI under `crates/workshop`. Every path in this plan is relative to its root, except the guide paths in step 4 (see Docs).
- Setup: `npm ci --prefix crates/workshop` (Node 22) before cargo commands that build the Workshop (`.github/workflows/ci.yml`).
- Build: `cargo build --locked -p workshop` (`.github/workflows/ci.yml` line 198).
- Package tests: `cargo nextest run --locked -p <package>`, for example `-p runvil-engine`, `-p runvil-lua`, `-p harness-runner`, `-p workshop-server`, `-p workshop-agents` (`plan/2026-11-08-1-per-run-packs.plan.md`, Notes). UI tests: one file with `node --test test/<file>.mjs` from `crates/workshop/ui`, which is how the `test` script in `crates/workshop/ui/package.json` runs the suite.
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
- Rustdoc: run `$env:RUSTDOCFLAGS = '-D warnings'` first, then:
  - `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `cargo doc -p runvil --no-deps`
  - `cargo doc -p harness --no-deps`
  - `cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `cargo doc --locked --no-deps -p workshop-server --document-private-items`

  These come from `.github/workflows/ci.yml` lines 97 to 108 and 154.
- API surface: run `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue`, then `cargo +nightly-2026-09-05 xtask api --check` (toolchain pinned in `crates/build-xtask/src/api/toolchain.rs` line 19). A step that changes the `runvil` facade regenerates `crates/runvil/public-api.txt` with `cargo +nightly-2026-09-05 xtask api --bless`; a plain `xtask api` only prints the difference (`crates/build-xtask/src/api.rs` lines 31 to 35).
- Docs: the language guide is the sibling repository `runvil-docs` (`c:\Users\Vinnie\cursor\runvil-docs`), and the `src/language/` paths in step 4 are relative to its root, with line numbers as they stand at its HEAD `1b19d20`.
  - Build it from the `runvil` root: run `$env:RUNVIL_DOCS = 'c:\Users\Vinnie\cursor\runvil-docs'` and put `$env:USERPROFILE\.cargo\bin` on `PATH` (or set `$env:MDBOOK`).
  - Then run `cargo xtask site --books-only`, which stages the chapters, builds them with mdbook, and checks links, and then `cargo test -p build-user-guide`. Sources: `runvil-docs` files `CONTRIBUTING.md` line 7 and `plans/build-language-guide.md` line 149.
  - House rules: no em dash and no double dash, every code fence opens with four backticks, and each paragraph stays on one line (`runvil-docs` file `CONTRIBUTING.md` lines 14 to 18).
  - Write each rule as steps 1 to 3 built it, checked against their tests.
  - Step 4 comes before steps 5 and 6 so that the final step builds and tests `runvil` as a whole.
- Guide step: step 4 changes files only in `runvil-docs`, so for that step `runvil-docs` is the repository: run its git commands there and take its commit hashes from it.
  - The plan copy stays in `runvil`, outside `runvil-docs`, so commit it apart from the guide: right after the step's first commit in `runvil-docs`, commit the plan copy alone in `runvil` with the same message.
  - From then on, an amend that stages the plan copy amends that `runvil` commit, and an amend of guide files amends the `runvil-docs` commit.
  - On resume, take step 4's commit from `runvil-docs`'s HEAD.
  - Before step 4 makes its first change, check that `runvil-docs` has a clean worktree; when it does not, stop and ask the user.
- Known failure: `cargo test -p build-xtask` fails because step 1's `queued_for_test` hook in `crates/runvil-internal/engine/src/execute/scheduler/test_hooks.rs` put `Scheduler` at 83 inherent methods, past the 82 its entry in `crates/build-xtask/src/type_size.rs` records.

## Terms

- The **main chain** is a run's H1 pass and section walk, task `0`. Every other chain is a task or a `call` child (`ChainKind` in `crates/runvil-internal/engine/src/execute/scheduler/chain_record.rs` lines 43 to 61).
- A task's **owner** is the chain that spawned it (`TaskEntry.owner`, `crates/runvil-internal/engine/src/execute/scheduler/tasks.rs` line 143). A task **owns** the live tasks that its own chains spawned. The arena keeps no owner index; lookups scan the entries, as `live_all` does (`tasks.rs` lines 283 to 293).
- A task is **idle** once it has replied with `tasks.reply` and waits on its inbox.
- A **round** is one model request of `models.loop`. A message **withdraws** a round that has no answer yet, and the loop sends it again with the message appended. The **reissued** round has a new round id (`crates/runvil-internal/engine/src/execute/run/effect.rs` lines 352 to 362).
- A **steering message** is one the operator sends while the agent works.

## Record order and task inbox events

- `Run::deliver` returns the events that the answers resumed since the last step produced, in order, followed by the message's `InboxMessage`.
- Why: today it returns only the `InboxMessage`. `Run::resume` stamps the events an answer produces at once and leaves them in the run's shared event buffer (`EventBuffer`, `crates/runvil-internal/types/src/emitter.rs` lines 53 to 56) for the next `step` to hand out (`crates/runvil-internal/engine/src/execute/scheduler/drive.rs` line 91). The Harness answers landed effects and then delivers (`crates/harness-internal/runner/src/effect_loop.rs` lines 326 to 332). So its record holds the message ahead of the model reply that came before it. The Workshop transcript follows record order (`crates/workshop/agents/src/tee.rs` lines 43 to 58), so it shows the same wrong order.
- A message sent to a task still waiting for admission is queued without an event. Right after the task's `TaskStarted` (`crates/runvil-internal/engine/src/execute/scheduler/admission.rs` lines 375 to 384), one `InboxMessage` follows per queued message, oldest first, under the task's provenance. A task canceled before admission reports neither `TaskStarted` nor its messages.
- A task's inbox queue goes away when the task stops being live: it returns, fails, is canceled, is abandoned, or is closed by the idle cap. A long run then keeps no messages that no chain can read.

## Stops

- The Harness acts on a raised stop right before each step, as well as after it. Take a stop raised while the Harness records answers and delivers a message:
  - The Harness drops the round in flight before the step that applies the message.
  - The Engine applies the `Dropped` answer before the message, so `models.loop` raises `Canceled`.
  - The message stays queued for the next read.
- When the Harness first sees a raised stop after a step that withdrew a model round, it also drops every model round that step issued for the same task, unstarted. To the Host, that reissue is the request it stopped.
- So a Host that stops and then sends always gets the stop, as `RunControl::send` already says (`crates/harness-internal/runner/src/harness.rs` lines 299 to 310).

## The idle cap

- Past `max_idle_tasks`, a reply closes one task: among the idle tasks other than the replier, the one idle longest that owns no live tasks.
- When every other idle task owns live tasks, the reply closes the one idle longest, and that task's live tasks are abandoned `owner_returned`, as if it had returned.
  - The closed task stays `done` with its latest reply, and `TaskSucceeded` reports it.
  - No `tasks_live` error arises.
  - This can abandon the replier when the closed task owns it; the cap's bound wins then.
- Today the cap closes the task idle longest whatever it owns, through the cancel path, so its tasks, the replier included, are abandoned `owner_aborted` (`crates/runvil-internal/engine/src/execute/scheduler/task_end.rs` lines 84 to 105, `crates/runvil-internal/engine/src/execute/scheduler/chain.rs` line 293).

## The round cap

- Once a loop's rounds have reached `max_tool_iterations`, the loop returns after its final reply without its last inbox read. A message that arrives then stays queued for the next `tasks.receive()`, `tasks.reply`, or loop.
- Today the last read takes the message, and the next round raises `tool_loop_exhausted` after a good reply. The message is then neither answered nor unread (`crates/runvil-internal/lua/src/models_loop/machine.rs` lines 237 to 241 and 279 to 284).

## Fanout arms that reply

- The fanout shim cancels each arm right after it takes the arm's result.
  - An arm that returned has already ended, so the cancel does nothing and reports nothing.
  - An arm that answered with `tasks.reply` and idles ends canceled and reports `TaskCanceled`.
- No arm outlives `tasks.fanout`, as the shim's comment says (`crates/runvil-internal/lua/src/__impl_fanout.lua` lines 81 to 83).

## Stalls

- A run that stalls with no task waiting on its inbox ends with the `Internal` error again, under the message the scheduler used before the inbox existed: "the scheduler stalled with no ready chain, no in-flight request, and no queued chain that can be admitted". Only an Engine fault gets there.
- A stall with tasks waiting on their inboxes still ends `stalled` and names them.

## The Workshop's end of a run

- The main chain's `RunSucceeded` or `RunFailed` event (`crates/runvil-internal/types/src/event.rs` lines 155 to 158) frames as a durable agent event of kind `run_ended` with empty content, so a replay shows it too. At `run_ended` the UI turns off the working state and `started`, which turns send off.
- The socket answers a message the conversation refuses with an error frame instead of a debug log:
  - Once the conversation is `Closed` (`SessionState::Closed`, `crates/workshop/agents/src/conversation.rs` line 136) or its run has an id (`Conversation::run_id`, line 144): "The agent's run has ended, so your message was not sent." A run that has started takes every message until it ends, so a refusal after its start means it ended, even before the conversation is `Closed`.
  - Before its run starts: "The agent is not taking messages yet, so your message was not sent."

  An error frame already ends the UI's working state.
- Try again is enabled only while the run takes messages (`started`), so it turns off at `run_ended`.
- On a same-session reattach, when the last send has no echoed `user_message` yet, the UI returns to the working state the last durable event left. An echo the replay brings turns it on again.
- The status bar goes idle when the main chain waits on its inbox (`InboxWaiting`) instead of at every assistant reply, since a reply no longer always ends a turn. An assistant reply still resets the reconnect backoff (`crates/workshop/server/src/agents/status.rs` lines 79 to 89).

## The Workshop transcript and working state

- A tool result joins the turn that holds its call, even when a steering message started a new turn in between. While the agent works, every call of the open batch that has no result shows as loading, whichever turn holds it, and the tail reads that call's label instead of "Planning next moves".
- A main-chain `user_message` that lands while a round streams drops that round's pending text and thought, and raises the late-chunk floor to that round, so its late chunks cannot reopen it. Each settle also drops the pending items of every earlier round.
- After a sent cancel, a live chunk or a durable model event (`agent_thought`, `agent_message`, `tool_call`) from a round newer than any the UI saw before the cancel means the agent went on with a queued message. The UI shows it working again, with Stop.
- A new session discards a dictation take only when it replaces an earlier session, not at a view's first acknowledgment.

## Decisions

- **Engine API:** it changes in one place: `Run::deliver` returns `Vec<Event>`. No new `Run` method drains the buffer, and `Run::resume` keeps its signature.
  - Swapping the Harness to deliver before it answers would put the stamps in order. But the record would still show the message ahead of the reply while `messages` holds the reply first.
- **Idle cap:** it prefers a task that owns no live tasks and falls back to the oldest with `owner_returned`. The alternatives were to never close a task that owns live tasks, which would leave the run over its cap, or to keep `owner_aborted`.
- **Round cap:** at a spent cap, the loop leaves messages queued rather than running an uncounted round or keeping the raise. The earlier promise that the loop never returns with a message unread gives way at a spent cap.
- **Fanout arms:** the shim cancels every delivered arm without first asking whether it is live, since a cancel of an ended task is free and silent. Refusing `tasks.reply` in an arm would need an Engine marker for arms that no other rule needs.
- **Empty stall:** it goes back to `internal`, so Engine faults keep their "retrying will not help" signal.
- **End of a run:** the Workshop frames it from the run's own events rather than from a frame the socket makes when the conversation closes, so a reattach replays it in order.
- **Try again:** it follows `started` rather than a new "waiting" member of `AgentSessionService`, so the service's public surface does not grow.

## Out of scope

- Placing a steering message where the agent read it instead of where it was delivered.
- Restoring the text of a message lost when the socket dropped mid-send.
- A replay driver for run logs.
- Deltas from tasks' model rounds; the shipped chat streams only the main chain's.
- Older plan copies under `plan/`, which stay as written.

### Open questions

None

</plan-context>

## Steps

<step-1>

1. [done] **Messages in record order, task inbox events, and the spent round cap** (points: 6) - Make `Run::deliver` drain the run's event buffer before it stamps the message: `deliver_message` (`crates/runvil-internal/engine/src/execute/scheduler/inbox.rs` line 111) takes the events `Run::resume` left in the buffer (`ChainState::take_events`, `crates/runvil-internal/engine/src/execute/context.rs` lines 281 to 283) and returns them in order, followed by its `InboxMessage`. Rewrite `Run::deliver`'s doc (`crates/runvil-internal/engine/src/execute/run.rs` lines 206 to 217) as the fence below says, naming steering. The Harness's `deliver` wrapper (`crates/harness-internal/runner/src/effect_loop.rs` lines 489 to 495) records each returned event in order. In `dispatch_send` (`inbox.rs` lines 256 to 294), queue a message for a task still waiting for admission (`self.admission.is_queued(chain)`) without emitting its `InboxMessage`. In `admit_chain` (`crates/runvil-internal/engine/src/execute/scheduler/admission.rs` lines 375 to 385), right after `TaskStarted`, emit one `InboxMessage` per message already queued for that task, oldest first, under the task's provenance; `Inboxes::queues` is private to `inbox.rs` (lines 61 to 68), so add a helper there. Drop a task's inbox queue when the task stops being live, in `complete_task` (`crates/runvil-internal/engine/src/execute/scheduler/task_end.rs` line 45) and `end_backing` (line 219). In the `models.loop` machine, make `check` (`crates/runvil-internal/lua/src/models_loop/machine.rs` lines 336 to 339) return without the inbox read once `rounds` has reached `max_rounds`, and update the capped half of `the_final_check_appends_late_messages_after_the_reply_and_starts_a_counted_round` (`crates/runvil-internal/lua/src/models_loop/machine-tests.rs` lines 484 to 506). Rewrite the admission docs that say only a task wait gives slots back (`admission.rs` lines 4 to 8, 114 to 126, and 215 to 245) to name the inbox wait too, and the docs of `Step`'s `events` fields (`run.rs`), which now leave out the events `deliver` returned. Update the `Run::deliver` callers, including the tests that look for a resumed answer's events in the next step (`crates/runvil-internal/engine/src/execute/run/tests-steer.rs` lines 288 to 323), and regenerate `crates/runvil/public-api.txt`. Tests:
   - After `Run::resume` and then `Run::deliver`, the events `deliver` returns hold the resumed round's `model_turn_completed` and `assistant_reply` events ahead of the `inbox_message`, and task `0`'s events in record order are in `seq` order.
   - Through the Harness, a message sent as the final reply lands is recorded after that reply's `assistant_reply` (beside `a_send_once_a_rounds_answer_has_landed_keeps_that_answer_and_asks_again_with_the_message` in `crates/harness-internal/runner/tests/it/harness-inbox.rs` line 403).
   - `tasks.spawn` followed at once by `tasks.send` records the task's `TaskStarted` before its `InboxMessage`, and a task canceled before admission reports neither.
   - A send to the internal timer of a timed `tasks.join_any` raises `task_not_owned`. The timed-wait setups are in `crates/runvil-internal/engine/src/execute/tests/timeouts.rs` lines 21 and 72.
   - At `max_tool_iterations` 1, a message delivered as the final reply lands leaves the loop returning the reply, with the message in `Step::Done`'s `unread`. Under `pcall`, the next `tasks.receive()` returns it at once.
   - Score: `Run::deliver` 1. Other changes: the deferred `InboxMessage` at admission and its drop on a cancel before admission 3, dropping an ended task's queue 1, the spent-cap return 1, the Harness recording each returned event and the callers 0, the docs 0.

`crates/runvil-internal/engine/src/execute/run.rs`

```rust
impl Run {
    /// Queues `text` for the main chain's inbox and
    /// returns the events to record before the next
    /// `step`, in order: those the answers resumed
    /// since the last step produced, then the
    /// `InboxMessage` that reports the message. The
    /// next `step` applies it: a waiting
    /// `tasks.receive()` resumes with it; a
    /// `models.loop` request with no answer yet is
    /// withdrawn, listed in `withdrawn`, and sent
    /// again with it; otherwise it waits, and a
    /// running loop takes it before its next request
    /// or before it returns, unless its round cap is
    /// spent. A message the run never reads comes
    /// back in `Done`'s `unread`. Call it only before
    /// `step` returns `Done`.
    pub fn deliver(&mut self, text: String) -> Vec<Event>;
    // changed, was: deliver(&mut self, text: String)
    //   -> Event
    // callers:
    // crates/runvil-internal/engine/src/execute/run/
    //   tests-inbox.rs (18)
    // crates/runvil-internal/engine/src/execute/run/
    //   tests-steer.rs (13)
    // crates/runvil-internal/engine/src/execute/run/
    //   tests-send.rs (2)
    // and 1 more file
}
```

</step-1>

<step-2>

2. [done] **The idle cap's choice, replied fanout arms, and the empty stall** (points: 6) - Change which idle task the cap closes. The code is `Scheduler::close_past_idle_cap` (`crates/runvil-internal/engine/src/execute/scheduler/task_end.rs` lines 84 to 105) with `TaskArena::idle_longest_past` (`crates/runvil-internal/engine/src/execute/scheduler/tasks.rs` lines 383 to 399). Among the idle tasks other than the replier, close the one idle longest that owns no live tasks. When every candidate owns live tasks, close the one idle longest and abandon its live tasks as `owner_returned`. Don't go through the cancel path's `owner_aborted` (`abort_subtree`, `crates/runvil-internal/engine/src/execute/scheduler/chain.rs` line 293), so the closed task stays `done` with its latest reply and no `tasks_live` arises. Whichever path the close takes, it keeps the `tracing::debug!` line naming the closed task (`task_end.rs` lines 92 to 97) and drops the closed task's inbox queue. Widen `AbandonReason::OwnerReturned`'s doc (`crates/runvil-internal/types/src/ids.rs` lines 243 to 261) to an owner the idle cap closed. Name the cap's close in the docs of `end_backing` (`task_end.rs` lines 211 to 218) and `abort_subtree` (`chain.rs` lines 282 to 285). In the stall rule (`crates/runvil-internal/engine/src/execute/scheduler/drive.rs` lines 82 to 89), end the run with `make_internal_error("the scheduler stalled with no ready chain, no in-flight request, and no queued chain that can be admitted")` when `inbox_waiters()` is empty, and keep `Stalled` otherwise; drop `make_stalled_error`'s branch for an empty list (`crates/runvil-internal/engine/src/error.rs` lines 127 to 141). In the fanout shim (`crates/runvil-internal/lua/src/__impl_fanout.lua` lines 165 to 187), yield a `cancel` for each arm right after the `join_any` that delivers it. An ended arm's cancel does nothing and emits nothing (`cancel_task`, `crates/runvil-internal/engine/src/execute/scheduler/waits.rs` lines 235 to 247). An arm idle after `tasks.reply` ends canceled. Tests, using `run_with_idle_cap` (`crates/runvil-internal/engine/src/execute/run/tests-reply.rs` line 30) for the cap:
   - With `max_idle_tasks` 1 and two idle tasks, where the one idle longer owns a live task and the other owns none, a third task's reply closes the one that owns none.
   - When task A spawns B, replies, and idles, and B then replies, A closes `done` with `TaskSucceeded`, and B is abandoned `owner_returned`.
   - When the closed task owns a running worker, the worker is abandoned `owner_returned` and its effect is withdrawn, while the closed task reads `done` and `ok` with no `tasks_live`.
   - An idle task whose live task was spawned from one of its `call` children counts as owning it, so the cap passes it over for an idle task that owns none.
   - The queued task that can never be admitted (`crates/runvil-internal/engine/src/execute/tests/scheduler/concurrency-stepped.rs` lines 47 to 83) ends `internal` again, and the inbox-waiter stall (`crates/runvil-internal/engine/src/execute/run/tests-send.rs` lines 232 to 252) still ends `stalled` naming `0.0`.
   - A fanout arm that answers with `tasks.reply` hands the call its reply as its result and ends with one `TaskCanceled`, and `tasks.pending()` after the call lists nothing.
   - The fanout tests that pin event lists still pass: `crates/runvil-internal/engine/src/execute/tests/fanout_acceptance.rs`, `crates/runvil-internal/engine/src/execute/tests/scheduler/fanout/failure_semantics.rs`, and `crates/runvil-internal/engine/src/execute/tests/serial_driver.rs`.
   - Score: `TaskArena::idle_longest_past` 0 (`task_of`, scored with the cap's choice). Other changes: the cap's choice of task 3, its `owner_returned` fallback 1, the empty-stall `Internal` error 1, canceling each delivered arm 1, the docs 0.

`crates/runvil-internal/engine/src/execute/scheduler/tasks.rs`

```rust
impl TaskArena {
    /// The task the idle cap closes once more than
    /// `cap` tasks are idle: among the idle tasks
    /// other than `except`, the one idle longest that
    /// owns no live task, or the one idle longest when
    /// each of them owns one. `task_of` names the task
    /// an owner chain runs inside, so a task owns what
    /// its `call` children spawned too. `None` while
    /// the run is within the cap.
    pub(super) fn idle_longest_past(
        &self,
        cap: NonZeroUsize,
        except: &TaskId,
        task_of: impl Fn(ChainIndex) -> TaskId,
    ) -> Option<TaskId>;
    // changed, was: idle_longest_past(&self,
    //   cap: NonZeroUsize, except: &TaskId)
    //   -> Option<TaskId>
    // callers:
    // crates/runvil-internal/engine/src/execute/
    //   scheduler/task_end.rs (1)
}
```

</step-2>

<step-3>

3. [done] **Stops before each step, reissued rounds, and the Harness's send guard** (points: 5) - In the effect loop's `drive` (`crates/harness-internal/runner/src/effect_loop.rs` lines 206 to 302), act on a raised stop right before each `self.run.step()` (line 210), as well as in the Pending arm (lines 245 to 248). A stop raised while the `Woken::Sent` arm (lines 326 to 332) records answers and delivers a message then drops the round in flight before the step that applies the message, and `models.loop` raises `Canceled` with the message still queued. Record in `Flights` (`InFlight`, lines 576 to 587) which in-flight effects are model rounds. When the Pending arm finds a stop raised after a step that withdrew a model round, also answer `Dropped`, unperformed, every model round that step issued for the same task. Make the send queue's end guard (`EndOnDrop`, `crates/harness-internal/runner/src/harness.rs` lines 259 to 267, made at lines 190 to 192) a field that `Harness::new` (line 117) builds. Then dropping the Harness, or dropping the future `run` returns before its first poll, also ends the queue. Docs:
   - The `RunControl::stop_round` doc (`harness.rs` lines 316 to 334) names the reissued round.
   - The `Reach::Withdrawn` doc (`effect_loop.rs` lines 559 to 560) says "the effects a step withdrew".
   - `crates/harness-internal/runner/README.md` line 3 drops "services" and names `send`.
   - "the built-in" becomes "the built-ins" in `crates/harness/examples/run-prompt.rs` line 2, `crates/harness/tests/suite/support.rs` line 50, `crates/harness-internal/runner/tests/it/prepare.rs` line 80, and `crates/harness-internal/runner/tests/it/harness.rs` line 75.
   - The `ToolContext` doc (`crates/runvil-plugin/src/context.rs` lines 21 to 24) says a Plugin's `install` keeps the Host-wide services its Pack needs in the Pack's `PackFactory`, which hands them to each run's instance it makes at `new_pack`.

   Tests go in `crates/harness-internal/runner/tests/it/harness-inbox.rs`, using `held_broker` (`crates/harness-internal/runner/tests/it/scripted.rs` line 208) and `run_beside`, `until`, `answers_to`, and `is_event` (`crates/harness-internal/runner/tests/it/harness.rs`):
   - A recorder that raises the stop and sends B while it records A's `InboxMessage`, during a held round, gets `Canceled` at the loop. The next `tasks.receive()` calls read A and then B, and no reissued round starts.
   - A stop first seen right after the step that reissued a round drops the reissued round unstarted.
   - A send while a held round streams aborts it (`chat.dropped()` is 1), and two rounds start. The first is answered `Dropped` in the record. The second request's last message ends with "This message arrived while you were working:" followed by the text, because the model client merges it into the previous user message.
   - `send` returns false once `run` returns a recorder refusal (`FailingRecorder`, `crates/harness-internal/runner/tests/it/support.rs` line 316), and once the Harness or its unpolled `run` future is dropped.
   - Score: Other changes: the stop check before each step 1, dropping a reissued round on a stop 3, the send guard as a field 1, the docs 0.

</step-3>

<step-4>

4. **Guide: the fixed rules and the stale passages** (points: 10) - In `runvil-docs`, describe the rules steps 1 to 3 built, and fix the passages that are false at HEAD `1b19d20`.

   In `src/language/15-tasks.md`:
   - Rewrite the cancel paragraph at line 799. A cancel withdraws every effect the task still has unanswered: a model round, a render, a store call, a timed wait's timer, and every tool call. The Harness stops the work already running and drops the rest unstarted. A store call the task made in the cancel's own step is dropped unperformed, and a write the Harness already performed stays.
   - At line 932, say a running `models.loop` also takes the task's queued messages. At line 942, say it does so once more before it returns, unless its round cap is spent.
   - Rewrite the idle cap at line 1036 for the new choice and its `owner_returned` fallback, and check its other mentions (lines 944, 1018, 1038, 1044, 1342, 1515, 1522, and 2317) and the abandon reasons.
   - At line 1050, say the record holds the events of answers that landed before a message ahead of its `inbox_message`. Where the chapter describes the `inbox_message` event, say a message sent to a task that has not started yet is reported right after the task's `task_started`, and not at all when the task is canceled before it starts.
   - After line 981, add the resumable-task loop: `models.loop()` then `messages:user(tasks.reply(messages[#messages].content))`.
   - Say "still running and not idle" for `tasks_live` at lines 1232, 1818, and 2407.
   - At line 2319, say an arm that replied ends canceled.
   - At line 2415, say a `tasks.send` can make a task's record depend on answer order.

   In `src/language/14-fanout.md`, say at line 77 that the call returns once every arm has finished or answered with `tasks.reply`, and after line 81 that a replied arm hands the call its reply as its result and is then canceled.

   In `src/language/16-limits-and-errors.md`:
   - At line 124, the Harness passes the limits set for the run, which are the defaults unless the Host sets others, and Workshop sets none (`crates/harness-internal/runner/src/prepare.rs` line 122).
   - At line 149, the idle cap's new choice.
   - At lines 713, 894, and 947, the `tasks_live` wording.
   - At line 998, drop the Pack that needs a service.
   - At line 1028, a stall with no task waiting on an inbox ends `internal`.

   In `src/language/17-quick-reference.md`, fix the idle cap row at line 278 and the `tasks_live` wording at line 317.

   In `src/language/11-conversations.md`:
   - At lines 235 and 2245, a message appended after a compaction stays when a second overflow restores the list.
   - At line 1204, at a spent cap the loop returns and the message waits for the next reader, and keep `### Reaching the round cap` (lines 1609 to 1619) consistent with that.

   In `src/language/05-lua-environment.md`:
   - At line 208, define a suspending call as one that pauses the section until its answer comes, from the Harness or from an inbox.
   - After line 2471, add that a `models.loop` on the main chain takes every queued message before each request and once more before it returns. Link it to `### Messages that arrive while the loop runs`.
   - At line 2493, say a reply ends the loop only once the loop finds the inbox empty.

   In `src/language/04-how-a-prompt-runs.md`:
   - At line 388, count four cases, adding a factory that refuses or panics for one run.
   - At lines 400 to 402, quote the log lines `crates/harness-internal/runner/src/packs.rs` writes (lines 122, 301, and 310) instead of `Pack construct failed; it is built as unavailable`.
   - At line 1729, a repeatable run also needs the same messages, each reaching the run at the same point.

   In `src/language/01-what-a-prompt-is.md`, line 456 needs the same repeatable-run change.

   Tests, in `runvil-docs`:
   - `rg -n "can still take effect|passes none|needs a service|Pack construct failed" src/language` finds nothing.
   - `rg -n "tasks still live" src/language` finds only the Engine's quoted message at `15-tasks.md` line 1305.
   - `rg -n "tasks.reply\(messages\[#messages\]\.content\)" src/language/15-tasks.md` finds the resumable-task loop.
   - `cargo xtask site --books-only` builds and passes its link check.
   - `cargo test -p build-user-guide` passes.
   - Score: the cancel paragraph 1, the idle cap through chapters 15, 16, and 17 1, the fanout reply 1, `models.loop` reading the inbox in chapters 05 and 15 1, the round cap and compaction in chapter 11 1, repeatable runs and answer order in chapters 01, 04, and 15 1, the resumable-task loop 1, chapter 04's unavailable Packs 1, the `tasks_live` wording 1, the stall, limits, services, suspending-call, and task-start event fixes 1.

</step-4>

<step-5>

5. **Workshop: the end of a run and refused messages** (points: 6) - Add `AgentEventKind::RunEnded` (`crates/workshop/server/src/agents/wire.rs` lines 156 to 193, with its doc table at lines 160 to 167). Map the main chain's `Event::RunSucceeded` and `Event::RunFailed` to it in `AgentEvent::from_event` (lines 241 to 299, with `on_main_chain` at line 229), with empty content. Add `"run_ended"` to the TypeScript `AgentEventKind` (`crates/workshop/ui/src/services/protocol.ts` lines 119 to 125). Add an `agent_event_run_ended` case to the shared fixture `crates/workshop/server/tests/fixtures/agent-frames.json` and to both readers: `crates/workshop/server/src/agents/wire-tests.rs` (`agent_fixture`, line 20), and the `CASES` list in `crates/workshop/ui/test/agent-wire-fixtures.mjs` (lines 67 to 83). In `answer_input` (`crates/workshop/server/src/agents/socket.rs` lines 303 to 336), answer a refused message with an error frame through `send_error` (`crates/workshop/server/src/websocket.rs` lines 108 to 114), instead of the debug log. The text is "The agent's run has ended, so your message was not sent." once `conversation.state()` is `SessionState::Closed` or `conversation.run_id()` is `Some`, and "The agent is not taking messages yet, so your message was not sent." otherwise. In `AgentSessionService` (`crates/workshop/ui/src/services/agent-session.ts`):
   - `run_ended` closes thinking, clears the stop, and turns `generating` and `started` off.
   - A same-session `acknowledge` (lines 371 to 385) returns to the working state the last durable event left when the last `respond` has no echoed `user_message` yet.
   - Update the `started` doc (lines 286 to 291) and the `respond` doc (lines 329 to 331).

   In the view (`crates/workshop/ui/src/parts/agent/agent-session-view.ts` lines 390 to 392), enable Try again only while `started` is on, and re-render it when `started` changes. In `on_event` (`crates/workshop/server/src/agents/status.rs` lines 79 to 89), push idle at the main chain's `InboxWaiting`, and let `AssistantReply` only reset the backoff. Remove the `InputResponse` re-export (`crates/workshop/server/src/lib.rs` line 87). Replace the two assertions that cannot fail, at `crates/workshop/server/tests/it/chat_gate/canonical_sequence.rs` line 59 and `crates/workshop/server/tests/it/chat_gate/overload.rs` lines 36 to 39, with checks that can, such as the conversation's run id staying the same across the stop. Tests:
   - The main chain's `RunSucceeded` and `RunFailed` frame as `run_ended` with empty content, and the fixture case round-trips in both suites.
   - After an agent's program returns, the client gets `run_ended`, and a later `input_response` gets the ended error frame.
   - The service turns `generating` and `started` off at `run_ended`.
   - After a run-ending error and `run_ended`, the view's Try again is disabled and send is idle.
   - A same-session reattach after an unechoed send returns to waiting when the last durable event was `awaiting_input`, and a replayed echo turns working on again.
   - The status bar stays busy across an `AssistantReply` while the loop goes on, and goes idle at the main chain's `InboxWaiting`.
   - The two replaced assertions fail on a wrong implementation.
   - Score: `AgentEventKind` 1 (`RunEnded`), TypeScript `AgentEventKind` 0 (`run_ended`), removed `InputResponse` re-export 0. Other changes: the refused-message error frame 1, the UI at `run_ended` 1, Try again following `started` 1, the reattach rule 1, the status bar 1, the replaced assertions 0.

`crates/workshop/server/src/agents/wire.rs`

```rust
enum AgentEventKind {
    // the other variants are unchanged
    /// The agent's run ended, succeeded or failed,
    /// from the run's `RunSucceeded` or `RunFailed`
    /// event: it takes no more messages.
    #[serde(rename = "run_ended")]
    RunEnded,
}
// changed, was: no `RunEnded`
// callers: none outside this file
```

`crates/workshop/ui/src/services/protocol.ts`

```ts
/** The kinds of durable agent event the server
 * frames; `run_ended` marks the run's end. */
export type AgentEventKind =
  | "agent_message"
  | "tool_call"
  | "tool_call_update"
  | "agent_thought"
  | "user_message"
  | "awaiting_input"
  | "run_ended";
// changed, was: no "run_ended"
// callers: none outside this file
```

`crates/workshop/server/src/lib.rs`

```rust
/// The `input_response` frame, re-exported from the
/// protocol crate.
pub use workshop_protocol::InputResponse;
// removed
// callers: none
```

</step-5>

<step-6>

6. **Workshop UI: steering in the transcript and the working state** (points: 8) - Keep the exported `buildTranscript` and `isToolLoading` (`crates/workshop/ui/src/parts/agent/transcript/tool-labels.ts` lines 79 to 87) and its `TurnState` as they are. In `crates/workshop/ui/src/parts/agent/transcript/transcript-model.ts`:
   - Have `splitTurns` (lines 261 to 272) route a `tool-result` to the turn that holds its call, so `draftTurn`'s `waiting` map (lines 279 to 280 and 351 to 359) pairs it.
   - The open batch is the transcript's latest `tool-call` item. Pass `{ last: true, generating }` for each of its calls that has no result, whichever turn holds it.
   - Let `tailStatus` (lines 491 to 535) read the open batch's loading call when the last turn has none.
   - Update the header rule (lines 12 to 13).

   In `AgentSessionService` (`crates/workshop/ui/src/services/agent-session.ts`):
   - Let `answersModelCall` (lines 604 to 618) look past user items while the call has no result.
   - At a `user_message`, drop the pending reply and reasoning items of rounds above `settled` and raise `settled` to the highest of them. `foldDelta` (lines 489 to 525) then ignores their late chunks. Have `settle` (lines 543 to 561) also drop the pending items of every earlier round.
   - Remember the newest round seen when `cancelTurn` (lines 351 to 362) runs. After that, a delta or an `agent_thought`, `agent_message`, or `tool_call` from a newer round clears `stopped` and turns `generating` on, and a reasoning chunk of that round opens with its start time.

   Fix the docs:
   - The `AgentCancelFrame` doc (`crates/workshop/ui/src/services/protocol.ts` lines 273 to 278), and the cancel and grace paragraphs of the `wire.rs` module doc (`crates/workshop/server/src/agents/wire.rs` lines 14 to 20): a stop lets the agent go on with a message already queued.
   - The `AgentDeltaFrame` doc (`wire.rs` lines 359 to 368): a withdrawn round ends with no superseding event.
   - The Stop comment at `crates/workshop/ui/src/parts/agent/agent-session-view.ts` lines 156 to 157.

   In the view's session handler (`agent-session-view.ts` lines 176 to 184), discard a dictation take only when `this.sessionId` was already set, and update the header (lines 27 to 30). Tests:
   - In `crates/workshop/ui/test/transcript-model.mjs`, the items user, tool-call `c1`, user, tool-result `c1` attach the result to `c1`.
   - With user, tool-call `c1`, user while generating, `c1` shows loading and the tail reads its label.
   - In `crates/workshop/ui/test/agent-session-service.mjs`, take round 1's deltas, a `user_message`, then round 2's deltas and `agent_message`. No pending item remains, a late chunk of round 1 opens nothing, and a replay of the durable log gives the same items.
   - After a steer and a failed call's error frame, the batch's later results turn `generating` back on.
   - After `user_message`, `tool_call`, `user_message`, and `cancelTurn`, a newer round's delta turns `generating` on, with the thought keeping its start, and the next `awaiting_input` ends it.
   - A late chunk from a round issued just before the Stop turns Stop back on until `awaiting_input`; the test pins that.
   - In `crates/workshop/ui/test/agent-session-view.mjs`, Stop shows while that newer round streams.
   - In `crates/workshop/ui/test/agent-stt.mjs` (lines 724 to 736), a take started before the first acknowledgment survives it, and a real session replacement discards it.
   - Score: Other changes: pairing a result with its open call across turns, loading, and the tail 3, dropping a cut round and raising the floor 1, restarting the working state after Stop on a newer round 3, the dictation discard 1, the docs 0.

</step-6>
