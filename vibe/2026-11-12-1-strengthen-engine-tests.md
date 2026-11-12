---
name: Strengthen Engine tests
overview: Make existing Engine tests fail when the code they run is broken. Mutation testing found 89 changes to production code, outside `runvil-vfs`, that an existing test runs and every test still passes. Checks and input cases added to 45 existing tests now catch 63 of them. No test is added and no production code changes.
todos:
  - id: strengthen-tests
    content: "Strengthen the existing tests in `runvil-lua`, `runvil-runtime`, `runvil-prompt`, `runvil-types`, `runvil-plugin`, and `runvil-model-client`, in one commit with this plan"
    status: completed
isProject: false
---

# Strengthen Engine tests

<product-contract>

## Product Requirements

A test that runs a line does not always fail when the line is wrong. Mutation testing finds those cases: it makes one small change to the production code at a time and reports each change that every test still passes. Per-test coverage then names the tests that run the changed line and pass anyway. This plan strengthens those tests, and changes nothing else.

- Problem and users:
  - On 2026-11-11 at `fa9936f64`, `cargo-mutants` 27.1.0 made 4,178 single changes to the Engine's production code, each judged by every Engine test. Outside `runvil-vfs`, 145 passed every test.
  - On 2026-11-12 at `2d40bafa2` the 145 were tested again. Two fail a test there. Of the other 143, 89 are on a line that at least one of the 2,638 Engine tests runs, and 54 are on a line no test runs.
  - The 89 are tests that pass with the code they run broken: 42 in `runvil-lua`, 23 in `runvil-runtime`, 14 in `runvil-prompt`, 7 in `runvil-types`, 2 in `runvil-plugin`, and 1 in `runvil-model-client`.
  - The users are the maintainers and reviewers, who rely on these tests to catch a regression.
- Goals:
  - Each of the 89 that alters behavior fails an existing test.
- Non-goals:
  - New tests.
  - Any change to production code.
  - `runvil-vfs`.
  - The 54 changes on lines no test runs.
  - Mutation testing in CI.
- Success criteria:
  - Testing the 89 again on the changed tree leaves only the ones the Decision Record lists.
  - The Engine has the same 2,638 tests before and after.
- Constraints:
  - Only existing test functions change: a check is added on a value the test already produces, or an input case is added to the test.
  - A test's name stays true to what it checks.
- Open questions: None

## Functional Specification

No product behavior changes. Each added check states a behavior the code already has, and each failed for at least one of the 89 changes when that change was applied.

- Acceptance criteria:

  | Crate | Run by a test and passing | Now fail a test | Alter no behavior | No fitting test |
  |---|---|---|---|---|
  | `runvil-lua` | 42 | 24 | 14 | 4 |
  | `runvil-runtime` | 23 | 20 | 3 | 0 |
  | `runvil-prompt` | 14 | 13 | 1 | 0 |
  | `runvil-types` | 7 | 4 | 1 | 2 |
  | `runvil-plugin` | 2 | 1 | 1 | 0 |
  | `runvil-model-client` | 1 | 1 | 0 | 0 |
  | Total | 89 | 63 | 20 | 6 |

  - The 63 are caught by 45 strengthened tests: 19 in `runvil-lua`, 12 in `runvil-runtime`, 8 in `runvil-prompt`, 4 in `runvil-types`, and 1 each in `runvil-plugin` and `runvil-model-client`.
  - The Decision Record names the 20 that alter no behavior and the 6 with no fitting test.

</product-contract>
<implementation-contract>

## Technical Design

Test code only: 37 files in six crates, 423 lines added and 64 removed. No public item, production line, or dependency changes.

- File and public API changes:
  - `crates/runvil-internal/lua/src`: 18 files, among them the test modules of `coro.rs` and `program.rs`.
  - `crates/runvil-internal/runtime/src`: 9 files under `execute/`, and `subst-tests.rs`.
  - `crates/runvil-internal/prompt/src`: `tests/section_fences.rs`, `contract/tests.rs`, and the test module of `body_text.rs`.
  - `crates/runvil-internal/types/src`: `error-tests.rs`, `names-tests.rs`, and the test modules of `models.rs` and `untrusted_inventory.rs`.
  - `crates/runvil-plugin/src/service.rs` and `crates/runvil-internal/model-client/src/client-tests.rs`: one test each.

</implementation-contract>
<verification-contract>

## Testing Plan

The measurement that found the 89 is also the check that they are closed. Each was applied to the tree before and after this change and judged by every Engine test.

- Regression, security, and performance:

  | Tree | Tested | Fail a test | Pass every test |
  |---|---|---|---|
  | `2d40bafa2` | 89 | 0 | 89 |
  | `2d40bafa2` with this change | 89 | 63 | 26 |

  How the tests were found: every Engine test ran once under coverage with its own profile, which gives the tests that run each line. For each change, the test chosen is one whose subject is the changed code, in that code's own test module where there is one.

- Exit criteria:
  - `cargo nextest run --locked --all-features` over the nine Engine packages: 2,638 tests, the count on `2d40bafa2`.
  - `CARGO_BUILD_WARNINGS=deny cargo clippy --locked --all-targets --all-features` over the six changed packages.
  - `cargo fmt --all --check`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - The diff against `2d40bafa2` adds no file, adds or removes no `#[test]`, and touches no line outside test code.

</verification-contract>
<decision-record>

## Decision Record

- Decisions:
  - Only existing tests change. Rationale: the purpose is to repair tests that pass when the code they run is broken. A change on a line no test runs is missing coverage, which is a different problem with a different fix.
  - The test that gets the check is the one whose subject is the changed code, not every test that passes through the line. Rationale: one line is run by a median of 25 tests and by as many as 1,550, nearly all on their way to checking something else.
  - A new input case in an existing test counts as strengthening it. Rationale: several tests ran the line but never with an input on which the change makes a difference, such as two `lua` fences with no line between them.
  - One change for six crates. Rationale: it is one kind of edit throughout and 37 files.
  - No test for the 20 changes that alter no behavior:
    - `runvil-lua`, 14: the seven changes to the library flags of the temporary VM in `compiler` (`program.rs:23` and `:24`), which only parses and dumps a chunk; `dispatch.rs:207`, whose guard is true for every error that can reach it; `json/encode.rs:425`, which zero never reaches; `models_loop/machine.rs:384` and `:387`, where the guard is already implied; `projection.rs:86`, where an empty batch accepts and rejects what no batch does; and two of the three changes at `tools/offer.rs:212`, which differ only for a negative length, where every form gives 0.
    - `runvil-runtime`, 3: `Admission::clear` (`admission.rs:284`), which runs only as a run ends; the guard at `chain_record.rs:535`, which its one caller has already made true; and the guard at `step.rs:255`, where `finish` does the same things in the same order.
    - `runvil-prompt`, 1: `parse.rs:148`, where the extra heading let through is the H1, which the next line filters out.
    - `runvil-types`, 1: `seed.rs:106`, `|` as `^` on two values with no bit in common.
    - `runvil-plugin`, 1: `registry.rs:274`, `Vec::new()` as `vec![]`.
  - No check for the 6 changes with no fitting test. Rationale: the tests that reach these lines are about something else, so a check there would make the test's name untrue.
    - `runvil-lua`, 4: `route_store_to_shims` (`engine_globals_store.rs:391`) and `call_optional_line` (`protocol/parse/store.rs:17`), which only other crates' tests reach; `clear_jump` (`vm/run.rs:36`) and the guard at `vm/run.rs:282`, which differ only when a lock is poisoned.
    - `runvil-types`, 2: `attribute_len` (`untrusted_inventory.rs:419`), which the tests reach without ever passing an attribute opener; and `Debug for ToolCatalog` (`tools/registry.rs:32`), reached only by a `runvil-runtime` test of another type's `Debug`.
- Rejected alternatives:
  - New tests for the 54 changes on lines no test runs and for the 6 with no fitting test. Reason: they are missing coverage, not weak tests. Revisit: as separate work on coverage.
  - A check in every test that runs a changed line. Reason: it would put unrelated assertions in hundreds of tests. Revisit: never.
- Assumptions, risks, and notes:
  - The measurement makes simple changes only: a swapped operator, a deleted `match` arm or negation, a function body replaced by a default value. A clean result does not show that the tests catch a subtler fault.
  - The list of changes comes from `fa9936f64`. Code added between it and `2d40bafa2` was measured only in `runvil-prompt`, and `runvil-markdown`, which is newer than the list, was not measured.
  - The measurement ran on Linux only.
  - One of the 89, the guard at `execute/scheduler/effects.rs:290` in `runvil-runtime`, failed a test on unchanged `2d40bafa2` when the machine was heavily loaded and passed every test when it was not. With this change it fails either way.

### Deferred and Out of Scope

- Deferred: `runvil-vfs`, which held 56 such changes at `fa9936f64`, until the maintainers say whether more tests there are wanted.
- Deferred: the 54 changes on lines no test runs, and the 6 with no fitting test, as coverage work.
- Deferred: `runvil-markdown`, until it is measured.
- Out of scope: mutation testing in CI.

</decision-record>
<project-survey>

## Project Survey

None

</project-survey>
<execution-plan>

## Execution Instructions

<step-1>

### Step 1: Strengthen the tests [completed]

- Component: `strengthen-tests`
- Tests:
  - The exit criteria in the Testing Plan

Build: for each of the 89 changes, add a check or an input case to the existing test whose subject is the changed code. Leave the 26 the Decision Record lists. Test the 89 again and record both rows in the Testing Plan.

Commit: one commit. Suggested subject: `Make existing Engine tests fail when the code they run is broken`.

</step-1>

</execution-plan>
