// Checks, on Linux, that an earlier release of Workshop offers the latest
// release, installs it, and then runs a prompt.
//
// Usage: node update.mjs <appimage> <version>
//   <appimage>  the AppImage of an earlier release, such as
//               Runvil_0.3.0_amd64.AppImage; the check works on a copy
//   <version>   the latest release, which Workshop must offer and install
//
// Environment:
//   CHEAPSEEK_BASE_URL  the base URL of a running CheapSeek, the chat model
//                       of the prompt's run
//
// A person runs this by hand once a release is published. Workshop reads
// updates from an address compiled into it, the `workshop-latest` release
// of cppalliance/runvil, so the check reads published files only and
// cannot run before a release.
//
// The check starts Workshop in a home folder of its own, on a display of
// its own (xvfb-run) and a session bus of its own (dbus-run-session), so
// the window stays off the screen of the person who runs it. It reads and
// clicks the window through tauri-driver, Tauri's WebDriver server.
//
// 1. The earlier Workshop shows "Runvil <version> is available".
// 2. A click on "Download Update" turns the AppImage into the file that
//    the release's SHA256SUMS lists, and that file prints <version>.
// 3. The updated Workshop, started again by this check, runs hello.md to an
//    end with text.
//
// Needs Node 22 or later, tauri-driver (`cargo install tauri-driver`),
// WebKitWebDriver, xvfb-run, dbus-run-session, and pkill.
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync, writeSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir, userInfo } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
// The release files of the processor this check runs on.
const ARCH = { x64: "amd64", arm64: "aarch64" }[process.arch];
// The process sessions this check started, each with everything in it.
const sessions = [];

function stop() {
  for (const session of sessions) {
    spawnSync("pkill", ["-TERM", "-s", String(session)]);
  }
}

// The line is written before the exit: a line queued on a pipe can be lost
// when the process exits.
function fail(message) {
  writeSync(2, `update: ${message}\n`);
  stop();
  process.exit(1);
}

// Whatever ends the check, the processes it started are stopped first: they
// run in sessions of their own, so Ctrl-C alone leaves them running.
process.on("uncaughtException", (error) => fail(`stopped by an error: ${error?.stack ?? error}`));
process.on("unhandledRejection", (reason) => fail(`stopped by an error: ${reason?.stack ?? reason}`));
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => fail(`stopped by ${signal}`));
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// Calls `probe` every half second until it returns a value that is not
// empty, and returns that value, or undefined after `seconds`.
async function until(seconds, probe) {
  const end = Date.now() + seconds * 1000;
  for (;;) {
    const value = await probe();
    if (value) return value;
    if (Date.now() > end) return undefined;
    await sleep(500);
  }
}

// The SHA-256 of a file, or "" while the file cannot be read. The updater
// moves the old AppImage away before it writes the new one, so for a moment
// the path names no file.
function sha256(path) {
  try {
    return createHash("sha256").update(readFileSync(path)).digest("hex");
  } catch {
    return "";
  }
}

function freePort() {
  return new Promise((resolve) => {
    const server = createServer();
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

const [given, version] = process.argv.slice(2);
if (!given || !version || process.argv.length !== 4) {
  fail(`usage: update.mjs <appimage> <version>; got ${process.argv.length - 2} arguments`);
}
if (!existsSync(given)) fail(`the AppImage ${given} does not exist`);
const cheapseek = process.env.CHEAPSEEK_BASE_URL;
if (!cheapseek) fail("CHEAPSEEK_BASE_URL must hold the base URL of a running CheapSeek; it is empty");
if (process.platform !== "linux" || !ARCH) {
  fail(`update.mjs runs on Linux x64 or ARM64; this is ${process.platform} on ${process.arch}`);
}
for (const tool of ["tauri-driver", "WebKitWebDriver", "xvfb-run", "dbus-run-session", "pkill"]) {
  if (spawnSync("sh", ["-c", `command -v ${tool}`]).status !== 0) fail(`${tool} must be on PATH; it is not`);
}

// The SHA-256 the release publishes for its AppImage.
const asset = `Runvil_${version}_${ARCH}.AppImage`;
const sums = `https://github.com/cppalliance/runvil/releases/download/v${version}/SHA256SUMS`;
const listing = await fetch(sums, { signal: AbortSignal.timeout(60_000) }).then(
  (response) => (response.ok ? response.text() : fail(`${sums} answered ${response.status}, required 200`)),
  (error) => fail(`${sums} did not answer: ${error.cause?.message ?? error.message}`),
);
const released = listing
  .split("\n")
  .map((line) => line.trim().split(/\s+\*?/))
  .find(([, name]) => name === asset)?.[0];
if (!released) fail(`${sums} must list ${asset}; it lists:\n${listing}`);

const work = mkdtempSync(join(tmpdir(), "runvil-update-"));
const home = join(work, "home");
const app = join(work, "Runvil.AppImage");
mkdirSync(join(home, ".runvil", "agents"), { recursive: true });
copyFileSync(given, app);
chmodSync(app, 0o755);
if (sha256(app) === released) fail(`${given} is release ${version} already; give the AppImage of an earlier release`);
console.log(`update: working in ${work}`);

// The Gateway the earlier Workshop starts reads this config. It holds
// CheapSeek as the one chat model and no speech model, so the Gateway
// downloads nothing.
writeFileSync(
  join(home, ".runvil", "gateway.toml"),
  `config-version = 0

[server]
bind = "127.0.0.1:0"
api_key = "update-check"
trust_loopback = true

[[endpoint]]
id = "cheapseek"
protocol = "openai"
base_url = "${cheapseek}"
api_key = ""

[[model]]
name = "cheapseek"
description = "CheapSeek, a test server that answers by rule"
context = 32768
upstream = "cheapseek"
endpoints = ["cheapseek"]

[[profile]]
name = "default"
models = []
`,
);
// A copy of prompts/hello.md in cppalliance/runvil-harness, the smallest
// prompt the Harness ships.
copyFileSync(join(here, "hello.md"), join(home, ".runvil", "agents", "hello.md"));

// Everything the check starts gets this environment and no other, so that
// nothing reaches the display, the session bus, or the home folder of the
// person who runs it.
const env = {
  PATH: process.env.PATH,
  HOME: home,
  USER: userInfo().username,
  LOGNAME: userInfo().username,
  LANG: "C.UTF-8",
  GDK_BACKEND: "x11",
};

// Starts tauri-driver in a process session of its own, has it open the
// AppImage's window, and returns a function that runs a script in the page.
// A script that cannot run, because the window is gone, returns undefined.
async function openWindow(name) {
  const [port, native] = [await freePort(), await freePort()];
  const log = openSync(join(work, `${name}.log`), "a");
  const driver = spawn(
    "dbus-run-session",
    ["--", "xvfb-run", "-a", "tauri-driver", "--port", String(port), "--native-port", String(native)],
    { env, cwd: work, detached: true, stdio: ["ignore", log, log] },
  );
  sessions.push(driver.pid);
  const command = async (path, body) => {
    try {
      const response = await fetch(`http://127.0.0.1:${port}${path}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(120_000),
      });
      const { value } = await response.json();
      return response.ok ? value : undefined;
    } catch {
      return undefined;
    }
  };
  const session = await until(120, async () => {
    const opened = await command("/session", {
      capabilities: { alwaysMatch: { browserName: "wry", "tauri:options": { application: app } } },
    });
    return opened?.sessionId;
  });
  if (!session) fail(`tauri-driver must open the window of ${app} within 120 seconds; see ${work}/${name}.log`);
  return (script) => command(`/session/${session}/execute/sync`, { script, args: [] });
}

// 1. The earlier Workshop offers the latest release. It checks for an
// update a few seconds after it starts.
const earlier = await openWindow("earlier");
const offered = await until(120, () =>
  earlier("var m = document.body.innerText.match(/Runvil ([0-9.]+) is available/); return m ? m[1] : '';"),
);
if (offered !== version) {
  fail(`the earlier Workshop must show "Runvil ${version} is available" within 120 seconds; it offered ${JSON.stringify(offered ?? "nothing")}`);
}
console.log(`update: the earlier Workshop offers Runvil ${offered}`);

// 2. Accepting the offer installs the release's own AppImage.
const clicked = await earlier(
  "var b = Array.from(document.querySelectorAll('button')).find(function (x) { return x.textContent.trim() === 'Download Update'; });" +
    " if (!b) { return false; } b.click(); return true;",
);
if (!clicked) fail('the offer must hold a button named "Download Update"; the window has none');
if (!(await until(900, () => sha256(app) === released))) {
  fail(`${app} must become ${asset} (SHA-256 ${released}) within 15 minutes of the click; its SHA-256 is ${sha256(app) || "unreadable"}`);
}
const printed = spawnSync(app, ["--version"], { env, encoding: "utf8" }).stdout?.trim();
if (printed !== `runvil-workshop ${version}`) {
  fail(`the updated AppImage must print "runvil-workshop ${version}" under --version; it printed ${JSON.stringify(printed)}`);
}
console.log(`update: the update installed ${asset}, which prints "${printed}"`);

// 3. The updated Workshop runs a prompt. The check starts it itself,
// because on Ubuntu 26.04 Workshop's own restart after an update fails:
// https://github.com/cppalliance/runvil-workshop/issues/36
// Only one Workshop runs at a time, so the check first stops the earlier
// one and the one the update may have started in its place. The pattern
// matches every stage of a starting AppImage (its runtime, its AppRun
// script, AppRun.wrapped, and Workshop) and leaves the Gateway running.
// The updated Workshop needs that Gateway when the AppImage it replaced
// held the only one, as the 0.3.0 AppImage did:
// https://github.com/cppalliance/runvil-workshop/issues/37
const workshop = ["-s", String(sessions[0]), "-f", "Runvil\\.AppImage|/AppRun|runvil-workshop"];
const gone = await until(60, () => {
  spawnSync("pkill", ["-TERM", ...workshop]);
  return spawnSync("pgrep", workshop).status === 1;
});
if (!gone) fail("the earlier Workshop must stop within 60 seconds of the update; it still runs");
const updated = await openWindow("updated");
const port = await until(120, () => updated("return document.title === 'Runvil' ? location.port : '';"));
if (!port) fail(`the updated Workshop must show its page within 120 seconds; see ${work}/updated.log`);
const run = spawnSync(process.execPath, [join(here, "run-prompt.mjs"), port, "hello"], { stdio: "inherit" });
if (run.status !== 0) fail("the updated Workshop must run hello.md to an end with text; run-prompt.mjs printed why it did not");

stop();
rmSync(app);
writeSync(1, `update: passed; the logs and the scratch home folder are in ${work}\n`);
process.exit(0);
