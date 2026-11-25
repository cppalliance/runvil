---
name: Chat and update checks for the installers
overview: The install cells of `installers.yml` end once each installed program serves a page. This plan adds a chat request to every cell that installs the Gateway, and a prompt run through Workshop to every cell that installs both. It also adds one check that a person runs after a release - an earlier Workshop offers the new release, installs it, and then runs a prompt. It changes the installer checks only.
todos:
  - id: plan-copy
    content: "Commit this plan alone on the branch qa-plan-tests"
    status: completed
  - id: cells
    content: "Add chat.mjs, run-prompt.mjs, and hello.md, call them from step 7 of cell.sh and cell.ps1, and start CheapSeek in the test job of installers.yml"
    status: completed
  - id: update
    content: "Add update.mjs"
    status: completed
  - id: local
    content: "Run both cells and the update check on Linux x64 against release 0.3.1, and run each failure case"
    status: completed
  - id: five-systems
    content: "Run every cell on the five runners with a dispatched run of installers.yml"
    status: pending
  - id: record
    content: "Record the results in this plan"
    status: completed
  - id: pr
    content: "Open one PR against cppalliance/runvil"
    status: completed
isProject: false
---

# Chat and update checks for the installers

## Terms

- **Cell**: one job of the test matrix in `installers.yml`. A cell installs one selection of components on a fresh machine and checks the result. `cell.sh` is the cell on macOS and Linux, and `cell.ps1` is the cell on Windows.
- **Selection**: the components a cell installs. `all` is Workshop and the Gateway, `workshop` is Workshop alone, and `gateway` is the Gateway alone. Windows has two more: `gateway-stt`, the Gateway with Speech to Text, and `rejections`, two component lists that the setup must refuse.
- **Workshop pin**: the commit of `cppalliance/runvil-workshop` that the root `Cargo.toml` names in `[workspace.metadata.workshop]`. The installers are built from that commit.
- **CheapSeek**: a test server from `cppalliance/cheapseek` that speaks the OpenAI chat API and computes each reply from fixed rules. It ships a GitHub action that starts it for a job.
- **Discovery file**: `~/.runvil/run/gateway.json`, the file a running Gateway writes its port and key to.
- **Agent**: Workshop's name for a prompt it can run. Each prompt file in Workshop's agents folder, `~/.runvil/agents`, is one agent, named after the file. Workshop's messages also call a section of a running prompt an agent: in "Model turn failed in agent `Greet`", `Greet` is a section of `hello.md`.
- **Agents socket**: the WebSocket `/agents/ws` of the local server that Workshop runs. Workshop's page launches an agent over it and reads the run's events from it.
- **`hello.md`**: the smallest prompt in `prompts/` of `cppalliance/runvil-harness`. Its run sends one request to the model and returns the reply.
- **`tauri-driver`**: Tauri's WebDriver server. Through it a script reads and clicks the window of a Tauri application such as Workshop.

<product-contract>

## Product Requirements

`installers.yml` builds the five installers and runs cells on five kinds of machine: Windows x64, macOS ARM, macOS Intel, Linux x64, and Linux ARM64. A cell ends once each installed program serves a page: Workshop its own page, and the Gateway its Settings page. The Gateway's first start writes a config that holds speech models only, so every cell passes with a Gateway that cannot answer a chat request.

Workshop also updates itself. It reads `latest.json`, the file that names the newest release and its files, offers that release in its window, downloads it, and replaces its own files. Workshop's tests run the update screen against a stand-in for the updater. The cells on Windows run the Windows installer a second time, in its update mode: there the installer replaces the files, and Workshop's own updater takes no part. The real update, in which a released Workshop updates itself to the next release, runs for the first time when a person tries it by hand after the release, and what that person looks at is up to them.

- Problem and users:
  - A release can ship installers whose Gateway and Workshop start and cannot run a prompt.
  - A release can ship a `latest.json` from which an earlier Workshop fails to offer the release or to install it.
  - The users are the people who install Runvil, and the engineers who change the installers, `installers.yml`, or the Workshop pin.
- Goals:
  - Every cell that installs the Gateway sends it one chat request, which CheapSeek answers, and requires a reply with text. Any text that is not blank passes, because CheapSeek's wording can change between its releases.
  - Every cell that installs both Workshop and the Gateway runs `hello.md` through Workshop, and requires that the model's reply holds text and that the run then ends.
  - One command, `update.mjs`, replaces that trial by hand: it runs the same steps every time and ends in a pass or a fail. On Linux, it checks that an earlier Workshop offers the new release, installs it, and then runs `hello.md` as the cells do.
- Non-goals:
  - Any change to Workshop, the Gateway, the installers, or `crates/build-installer`.
  - The update check on Windows and macOS, and the update check in CI. The Decision Record gives the reasons.
  - A fix for the two faults that the update check found in Workshop. The check works around both, and they are filed as issues 36 and 37 of `cppalliance/runvil-workshop`.
- Success criteria:
  - On Linux x64, `cell.sh` passes for `all` and for `gateway` with the installer of release 0.3.1.
  - A dispatched run of `installers.yml` passes every cell on the five runners.
  - On Linux x64, `update.mjs` passes from the 0.3.0 AppImage to release 0.3.1 in every one of 25 runs. The Testing Plan says why 25.
  - Each failure case in the Testing Plan fails with a message that names what was required and what was found.
- Constraints:
  - `cell.sh` and `cell.ps1` number their steps 1 to 6 today, and those steps keep every check they make. The two new checks run as a step 7, after them.
  - The chat check adds CheapSeek to the Gateway's config. Step 6 of `cell.ps1` reads which speech models the Gateway's first start wrote, so the config stays as the first start wrote it until that step has run.
- Open questions: None

## Functional Specification

A cell that installs the Gateway gets one more step, step 7. A person gets one new command, `node .github/installers/update.mjs`.

- Actors and workflows:
  - The test job of `installers.yml` installs Node 22, checks out CheapSeek, starts it, and runs the cell. Step 7 of the cell runs `chat.mjs`, and for the selection `all` it then copies `hello.md` into Workshop's agents folder, `~/.runvil/agents`, and runs `run-prompt.mjs`.
  - A person who has just published a release starts CheapSeek, downloads the AppImage of an earlier release, and runs `update.mjs` with that file and the new version.
  - `CHEAPSEEK_BASE_URL` holds CheapSeek's address for the cells and for `update.mjs`. CheapSeek's action sets it in the test job, and the person sets it for a local run. A cell passes it to `chat.mjs` as `<cheapseek-url>`.
- Inputs and outputs:
  - `chat.mjs <discovery-file> <cheapseek-url>` reads the Gateway's port and key from the discovery file. It reads the Gateway's config with `GET /admin/config`, adds CheapSeek as an endpoint with one model named `cheapseek`, saves the config with `PUT /admin/config`, and applies it with `POST /admin/config-apply`. These are the routes the Gateway's Settings page saves with. It then requires that `GET /v1/models` lists an entry whose `id` is `cheapseek` and whose `kind`, a field the Gateway adds to each entry, is `chat`, sends `POST /v1/chat/completions` with the message "Hello.", and requires text in `choices[0].message.content`.
  - `run-prompt.mjs <workshop-port> <agent>` connects to the agents socket on `<workshop-port>`, the loopback port that Workshop serves its page on, which step 4 of the cell finds. It requires that the list of agents Workshop sends holds `<agent>`, launches it, and reads the run's events. It requires an `agent_message` event with text and then a `run_ended` event. An `error` frame, a closed socket, or 120 seconds without an end fails it.
  - `.github/installers/hello.md` is a copy of `prompts/hello.md` at commit `7147e709f` of `cppalliance/runvil-harness`, the last commit that changed it.
  - `update.mjs <appimage> <version>` copies the AppImage into a new temporary folder and works on the copy. In a new home folder inside that folder, it writes `.runvil/gateway.toml`, a Gateway config that names CheapSeek as the one chat model, and copies `hello.md` into the agents folder. The earlier Workshop starts a Gateway, and that Gateway reads the config. For 0.3.0 it is the Gateway program inside the AppImage. `update.mjs` starts the copy through `tauri-driver` and requires that the window shows "Runvil `<version>` is available" within 120 seconds. It clicks "Download Update" and requires that the copy's SHA-256 turns into the one that the `SHA256SUMS` of release `v<version>` lists for the release's AppImage, within 15 minutes. That file is `Runvil_<version>_amd64.AppImage` on x64 and `Runvil_<version>_aarch64.AppImage` on ARM64. It requires that the updated file prints `runvil-workshop <version>` under `--version`. It then stops Workshop, starts the updated file again through `tauri-driver`, reads the port of Workshop's local server from the window, and runs `run-prompt.mjs` for `hello` on that port. Only the x64 path has been run.
  - Each script prints one line for each thing it confirmed, and exits 0.
- States and validation:
  - A cell whose selection installs the Gateway fails before it installs anything when `CHEAPSEEK_BASE_URL` is empty or `node` is missing from `PATH`.
  - `update.mjs` fails before it starts Workshop when the AppImage it was given does not exist, when `CHEAPSEEK_BASE_URL` is empty, when one of `tauri-driver`, `WebKitWebDriver`, `xvfb-run`, `dbus-run-session`, and `pkill` is missing from `PATH`, when the release's `SHA256SUMS` does not list the AppImage, or when the AppImage it was given is the new release already.
- Errors and recovery:
  - Every failure prints one line that says what failed, with what was required and what was found where the script knows both, and the script exits 1. Two examples: `chat: POST http://127.0.0.1:35285/v1/chat/completions answered 404, required 200: ...` and `run-prompt: the run of hello failed: Model turn failed in agent `Greet``.
  - `update.mjs` stops every process it started before it exits: when it passes, when a requirement fails, when an error it did not expect ends it, and when a person stops it with Ctrl-C. Those processes run in sessions of their own, so Ctrl-C alone would leave them running. It prints the folder that holds its logs.
- Security and privacy behavior:
  - CheapSeek listens on `127.0.0.1` only. `chat.mjs` sends the Gateway's key only to the Gateway's loopback port.
  - The test job checks CheapSeek out at the commit of its release `v1.1.0`. A CheapSeek release holds executables for Linux x64, macOS ARM, and Windows x64. On those three platforms its action downloads the executable and checks it against the release's `SHA256SUMS`. On macOS Intel and Linux ARM64 the test job builds CheapSeek from the checkout.
  - `update.mjs` starts `tauri-driver`, and through it Workshop, with a home folder of its own, on a display of its own (`xvfb-run`) and a session bus of its own (`dbus-run-session`). It gives those processes an environment of six variables: `PATH`, `HOME`, `USER`, `LOGNAME`, `LANG`, and `GDK_BACKEND`. `xvfb-run` and `dbus-run-session` add the display and the address of the bus. The window stays off the screen of the person who runs the check, and their `~/.runvil` is left as it was.
- Acceptance criteria:
  - `chat.mjs` fails when the address it was given for CheapSeek answers the Gateway's request with status 404, and `run-prompt.mjs`, run by hand on the Workshop of that Gateway, fails too.
  - `run-prompt.mjs` fails for an agent that Workshop does not list, and for a prompt that does not parse.
  - A cell that installs the Gateway passes only when `chat.mjs` passes, and a cell for `all` only when `run-prompt.mjs` passes too.
  - `update.mjs` passes only when all of its requirements hold: the offer in the window, the SHA-256 and the printed version of the updated file, and the run of `hello.md`.

</product-contract>
<implementation-contract>

## Technical Design

The change adds three Node scripts and one prompt file to `.github/installers/`, beside the cells. The scripts use what Node 22 has built in, `fetch` and `WebSocket`, so they run with Node alone, as `tools/docs-claims.mjs` does. `cell.sh` and `cell.ps1` call the same `chat.mjs` and `run-prompt.mjs`, so each check is written once.

- Architecture:
  - Each cell script numbers its steps. Steps 1 to 6 exist today: install, check which components are present, check the versions, start Workshop, check the Gateway's Settings page, and one check that differs by platform. Step 7 is new, and only the cells that install the Gateway have it: the selections `all`, `gateway`, and `gateway-stt`. It runs `chat.mjs`, and in a cell for `all` it then runs `run-prompt.mjs`.
  - `chat.mjs` adds CheapSeek to the Gateway's config through the Gateway's own routes while the Gateway runs. The config that the Gateway wrote at its first start stays on disk, unchanged, until step 7 runs, so steps 1 to 6 read what they read today.
  - The Gateway applies a config change after the work of its first start. On macOS and Linux the first start always sets up speech and begins to download its files, and on Windows it does so when Speech to Text was selected. In one run on Linux x64 that download was 608 MB, and the Gateway applied the change 43 seconds after `chat.mjs` asked. A slower connection takes longer, so each request of `chat.mjs` has a limit of 10 minutes, inside the 45 minutes a cell may take.
  - Workshop reads its agents folder each time a client connects to the agents socket, so the cell copies `hello.md` into that folder while Workshop is running.
  - The test job checks CheapSeek out once, at commit `54d18ebefc024fb2bfe86a3556792303829bcd56`, and runs the action from that checkout with `uses: ./cheapseek`. A CheapSeek release holds executables for Linux x64, macOS ARM, and Windows x64. The cells on macOS Intel and Linux ARM64 build it with `cargo build --release --locked` and pass the path of the result to the action's `executable` input. They build a copy of the checkout in the runner's temporary folder, because Cargo takes a package under this repository's checkout for a member of this workspace and stops. That build took between 2 minutes 51 seconds and 3 minutes 51 seconds in CheapSeek's own release run.
  - `update.mjs` is a command that a person runs once a release is published, and the cells leave it alone. It starts an earlier Workshop through `tauri-driver`, accepts the update that the window offers, and runs `hello.md` on the updated Workshop. CheapSeek is the chat model: `update.mjs` writes it into the Gateway's config before Workshop starts the Gateway, so it works without `chat.mjs`.
  - `run-prompt.mjs` needs the port of Workshop's local server. In a cell, step 4 has already found it. `update.mjs` reads it from the window, as `location.port`, because Workshop's window shows the page of that server.
- Modules and interfaces: None
- File changes:
  - `.github/installers/chat.mjs`, `.github/installers/run-prompt.mjs`, and `.github/installers/update.mjs` are new. So is `.github/installers/hello.md`, a copy of `prompts/hello.md` of `cppalliance/runvil-harness`.
  - `.github/installers/cell.sh` and `.github/installers/cell.ps1` gain step 7 at their end. At their start, in a cell that installs the Gateway, they now check that `CHEAPSEEK_BASE_URL` is set and `node` is on `PATH`.
  - `.github/workflows/installers.yml` gains four steps in its test job, which run in the cells that have step 7: install Node 22, check out CheapSeek, build it on the two platforms that need the build, and start it.
  - `vibe/2026-11-24-1-installer-chat-and-update-checks.md` is this plan.
- Data, persistence, failure, security, and privacy constraints: None

</implementation-contract>
<verification-contract>

## Testing Plan

The change adds checks, so the test of the change is that each new check passes on a working install, and fails when its input is wrong or the model does not answer.

- Unit: None
- Integration and end-to-end:
  - On Linux x64, in a temporary home folder: `cell.sh <setup.run> all 0.3.1 0.3.1` and `cell.sh <setup.run> gateway 0.3.1 0.3.1`, with the installer of release 0.3.1. The two versions are the ones that Workshop and the Gateway must print. `CELL_ROOT` names a temporary folder to install into, and `CHEAPSEEK_BASE_URL` a running CheapSeek.
  - On Linux x64: `update.mjs Runvil_0.3.0_amd64.AppImage 0.3.1`, where the arguments are the AppImage of the earlier release and the version it must update to. It runs 25 times, and every run must pass. A check that drives a window can pass 3 runs of 3 and then fail 1 run in 6, as a window test in `cppalliance/runvil-workshop` did. A check that fails that often passes 25 runs in a row about 1 time in 100.
  - On the five runners: one dispatched run of `installers.yml`, which runs every cell.
- Failure cases:
  - `cell.sh` for `gateway` with `CHEAPSEEK_BASE_URL` empty.
  - `chat.mjs` with a CheapSeek address that answers 404, and then `run-prompt.mjs` for `hello` on the Workshop that uses that Gateway. Both must fail.
  - `run-prompt.mjs` for `hello` before the Gateway has been given a chat model, for an agent that Workshop does not list, and for a prompt that does not parse.
  - `update.mjs` with a version that was never released, with the new release's own AppImage, and with `CHEAPSEEK_BASE_URL` empty.
  - `run-prompt.mjs` on a port where nothing listens, and `chat.mjs` without its arguments.
- Exit criteria:
  - `bash -n .github/installers/cell.sh` and `node --check` on the three scripts.
  - `cargo test -p build-xtask`
  - `node --test tools/docs-claims.mjs`
  - CI passes on the PR. The change is shell, PowerShell, JavaScript, and YAML, so the other commands of `AGENTS.md` "Verification" run in CI only.

</verification-contract>
<decision-record>

## Decision Record

- Decisions:
  - The checks go into the existing cells. The cells already install each selection on each of the five machines, so the new step costs one chat request in each cell that installs the Gateway, and one prompt run in each cell for `all`. A separate set of cells would install everything a second time.
  - Step 7 is required in every cell that installs the Gateway. A cell that skipped it when `CHEAPSEEK_BASE_URL` was empty would pass with the check gone, so the cell fails instead.
  - `chat.mjs`, `run-prompt.mjs`, and `update.mjs` are Node scripts. Node 22 has an HTTP client and a WebSocket client built in, and the repository already runs `tools/docs-claims.mjs` with Node alone.
  - `.github/installers/hello.md` is a copy of `prompts/hello.md` from `cppalliance/runvil-harness`. The alternative is to download the file at the Harness commit that Workshop's `Cargo.toml` names at the Workshop pin, which adds a checkout and a lookup to every cell. Workshop's own test of the same run, in `crates/workshop/server/tests/it/live.rs` of `cppalliance/runvil-workshop`, holds a copy too. When the prompt language changes and the copy stays behind, step 7 fails in the pull request that moves the Workshop pin.
  - `update.mjs` runs by hand and stays out of `installers.yml`. The address Workshop reads updates from is compiled into it, so the check can only read a release that is already published. A job that ran the check at the end of a release run would be new code in the release workflow whose first real run is the next release, so a fault in the job would first show in the middle of a release.
  - `update.mjs` runs on Linux only. `tauri-driver` works on Linux and Windows, which leaves macOS out. On Windows the update runs the Windows installer, so the check needs other steps after the click on "Download Update", and they wait for a later change. On Linux, Workshop updates itself only when it runs from an AppImage, so the check takes the AppImage of an earlier release as its first argument.
  - `update.mjs` starts the updated Workshop itself and leaves the Gateway of the earlier Workshop running. Both follow from faults the first runs of the check found in Workshop, which are filed as issues. On Ubuntu 26.04, Workshop exits after an update and its own restart fails ([runvil-workshop#36](https://github.com/cppalliance/runvil-workshop/issues/36)). The 0.3.0 AppImage holds the Gateway program and the 0.3.1 AppImage holds Workshop only, so the update replaces the AppImage and with it the only Gateway program on the machine ([runvil-workshop#37](https://github.com/cppalliance/runvil-workshop/issues/37)). The Gateway process that the earlier Workshop started keeps running after its file is replaced, and the updated Workshop attaches to it.
- Rejected alternatives:
  - Writing CheapSeek into `gateway.toml` before the Gateway's first start. Step 2 of both cells requires that the install leaves that file to the first start, and step 6 of `cell.ps1` reads what the first start wrote.
  - Stopping Workshop and the Gateway, editing `gateway.toml`, and starting both again. It adds a stop and a start to both cell scripts, for a Gateway that Workshop started and for one that the cell started. The Gateway's `/admin/config` routes, which its Settings page saves with and `chat.mjs` uses, work on a running Gateway.
  - The update check as a test in `cppalliance/runvil-workshop`, beside its window tests. It checks released files, `latest.json` among them, and this repository publishes those.
- Assumptions, risks, and notes:
  - The CheapSeek build lengthens the cells for `all` on macOS Intel and Linux ARM64, by the times that the second run below gives. A CheapSeek release with executables for those two platforms would remove the build.
  - `update.mjs` has run from the 0.3.0 AppImage only, which holds its own Gateway. At the release after 0.3.1, the earlier AppImage holds Workshop alone, so the check will need to put a Gateway program beside its copy of the AppImage, in the place where the 0.3.1 installer puts one. That case has not run.
  - This plan serves four tests of the team's QA plan of attack, a list of 40 tests that the reviewers have seen: "On each of the five systems, install both parts and send `hello.md`. The run ends with text.", "Install the Gateway alone and send it one chat request. It answers.", "Install the previous release and start Workshop. It offers the new version.", and "Accept that update, then send `hello.md`. The run ends with text."
  - Result of the cells on Linux x64, on Ubuntu 26.04.1 with the installer of release 0.3.1 and CheapSeek 1.1.0: `cell.sh` passed for `all` in 63 seconds and for `gateway` in 42 seconds. `chat.mjs` got the reply "Hi there — what do you need?", and the run of `hello.md` ended with "What do you need? I'll take it from there."
  - Result of `update.mjs` from the 0.3.0 AppImage, which was published in the pre-release `nightly`, to 0.3.1: it passed every one of 25 runs in a row. A run took 11.3 to 21.6 seconds, and 14.3 seconds on average.
  - Result of the failure cases: all 11 failed, each with a line of its own.
    - `cell.sh` for `gateway` with `CHEAPSEEK_BASE_URL` empty: `cell: the selection 'gateway' installs the Gateway, so CHEAPSEEK_BASE_URL must hold the base URL of a running CheapSeek; it is empty`
    - `chat.mjs` with a CheapSeek address that answers 404: `chat: POST http://127.0.0.1:45899/v1/chat/completions answered 404, required 200: {"error":{"code":"upstream_client_error","message":"upstream returned 404","type":"invalid_request_error"}}`
    - `run-prompt.mjs` for `hello`, after `chat.mjs` had failed that way: ``run-prompt: the run of hello failed: Model turn failed in agent `Greet` ``
    - `run-prompt.mjs` for `hello` before the Gateway has been given a chat model: `run-prompt: the run of hello must end within 120 seconds; frames so far: [agents, agent_session]`
    - `run-prompt.mjs` for an agent that Workshop does not list: `run-prompt: Workshop must list the agent nope; it lists ["bad","chat","hello"]`
    - `run-prompt.mjs` for a prompt that does not parse: `run-prompt: the run of bad failed: file must begin with a --- frontmatter delimiter (line 1, column 1)`
    - `run-prompt.mjs` on a port where nothing listens: `run-prompt: ws://127.0.0.1:9/agents/ws failed: the connection was refused or dropped`
    - `chat.mjs` without its arguments: `chat: usage: chat.mjs <discovery-file> <cheapseek-url>; got 0 arguments`
    - `update.mjs` with a version that was never released: `update: https://github.com/cppalliance/runvil/releases/download/v0.3.2/SHA256SUMS answered 404, required 200`
    - `update.mjs` with the new release's own AppImage: `update: <path>/Runvil_0.3.1_amd64.AppImage is release 0.3.1 already; give the AppImage of an earlier release`
    - `update.mjs` with `CHEAPSEEK_BASE_URL` empty: `update: CHEAPSEEK_BASE_URL must hold the base URL of a running CheapSeek; it is empty`
  - One branch of `update.mjs` has not run: the failure when Workshop offers another version than the one asked for. Release 0.3.1 is the only one that Workshop offers today.
  - Result on the five runners: two runs of `installers.yml` for pull request 221. A pull request runs the cells for `all` and `workshop`. In every cell for `all` that got as far as step 7, both checks printed the same replies as on my machine.
    - First run, at commit `5d1a63a24`: the cells for `all` passed on Linux x64, macOS ARM, and Windows x64, in 64 to 91 seconds a job. On macOS Intel and Linux ARM64 the job failed before the cell started, at the CheapSeek build: Cargo took the checkout inside this repository for a member of this workspace. The build now runs on a copy outside the checkout.
    - Second run, at commit `31f37957f`: the cells for `all` passed on Linux x64, Windows x64, macOS Intel, and Linux ARM64. The CheapSeek build took 7 minutes 53 seconds on macOS Intel and 2 minutes 29 seconds on Linux ARM64.
    - In the second run the cell for `all` on macOS ARM failed at step 4, which is older than this change and runs before step 7. Workshop gives a Gateway that it starts 30 seconds to answer its health probe, and the Gateway's log shows that it finished loading its speech runtime 37.6 seconds after it started. A rerun of that one job passed, in 89 seconds.
    - The ten jobs for `workshop`, five in each run, passed. They install Workshop alone, so the job skips the CheapSeek steps and the cell skips step 7.
  - The cell for `gateway` has run on Linux x64 only, on my machine, and the Windows cell for `gateway-stt` has not run. The other four systems first run those cells in a dispatched run of `installers.yml` or in a release run, so the todo `five-systems` stays open.
  - Result of the review: Greg Kal found that `update.mjs` could end without stopping what it had started. Tauri's updater moves the old AppImage away before it writes the new one (`install_appimage` in `tauri-plugin-updater` 2.11.0), so the read that computes the SHA-256 can find the path empty, and that error ended the script before it stopped `tauri-driver`, the display, Workshop, and the Gateway. Ctrl-C had the same effect. The read now counts as "not yet updated", and the script stops its processes on an unexpected error, on SIGINT, and on SIGTERM. Tested: a run that got SIGINT 6 seconds after its start, and one that got SIGTERM, each had 7 processes running before the signal and all 7 stopped after it. Each of the 28 runs before the change read the file outside that moment, and passed.
  - Result of the exit criteria: `bash -n`, the three `node --check` runs, `cargo test -p build-xtask` (99 passed), and `node --test tools/docs-claims.mjs` (2 passed) passed on my machine.
  - `vibelight`, the team's tool that outlines Rust crates for review, was not run: this change is shell, PowerShell, JavaScript, and YAML.

### Deferred and Out of Scope

- Deferred: the update check on Windows. Revisit once a person has run the Linux check after one release.
- Deferred: the update check in CI. Revisit if Workshop gains a setting for the address it reads updates from.

</decision-record>
<project-survey>

## Project Survey

None

</project-survey>
<execution-plan>

## Execution Instructions

- Work items:
  - Commit this plan alone on the branch `qa-plan-tests`, made from `origin/master`.
  - Add step 7 to the two cell scripts, add `chat.mjs`, `run-prompt.mjs`, and `hello.md`, add the four steps to the test job of `installers.yml` (Node 22, the CheapSeek checkout, its build on two platforms, and its start), and commit them together.
  - Add `update.mjs` and commit it.
  - Run the Testing Plan, and record the results in this plan in a last commit.
- Dependencies and verification:
  - A local run needs a running CheapSeek (`cheapseek --port <port>` prints its base URL), Node 22 or later, and for `update.mjs` also `tauri-driver`, `WebKitWebDriver`, `xvfb-run`, and `dbus-run-session`.
  - For a local run of a cell, the person sets `HOME` and `CELL_ROOT`, the folder the cell installs into, to temporary folders. The cell then leaves the person's own Runvil and `~/.runvil` as they were.

</execution-plan>
