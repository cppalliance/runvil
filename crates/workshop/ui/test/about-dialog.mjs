// Unit test for Help > About (src/parts/chrome/chrome.contribution.ts and
// src/parts/chrome/about-dialog.ts): the action resolves the registered
// update service, so the desktop dialog offers the update check instead of
// "Desktop updates unavailable", follows the service's phases while open,
// and without a registered service it still opens with the button
// disabled. Bundles the modules with esbuild and
// drives them against jsdom with a stub-backend UpdateService.
// Run: node test/about-dialog.mjs
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";
import { JSDOM } from "jsdom";

const uiDir = path.dirname(fileURLToPath(import.meta.url));

const dom = new JSDOM("", { url: "http://127.0.0.1:7910/" });
const { window } = dom;
globalThis.window = window;
globalThis.document = window.document;
globalThis.HTMLElement = window.HTMLElement;
globalThis.Element = window.Element;
globalThis.Node = window.Node;
globalThis.KeyboardEvent = window.KeyboardEvent;

const bundle = await esbuild.build({
  stdin: {
    contents: `
      import "./src/parts/chrome/chrome.contribution.ts";
      export { Commands } from "@workshop/platform/command-registry";
      export { registerService } from "@workshop/platform/service-registry";
      export { UPDATES, UpdateService } from "./src/services/update-service.ts";
    `,
    resolveDir: path.join(uiDir, ".."),
    loader: "ts",
  },
  bundle: true,
  write: false,
  format: "esm",
  platform: "browser",
  target: "es2022",
  logLevel: "silent",
  loader: { ".css": "empty" },
});
const { Commands, registerService, UPDATES, UpdateService } = await import(
  `data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`
);

const failures = [];
function check(name, condition) {
  if (!condition) failures.push(name);
}

const ABOUT = "workbench.action.showAboutDialog";

function updateButton() {
  return window.document.querySelector(".ws-about-dialog__check");
}

// Dismisses through the Close button, as a user does, so the dialog's
// teardown runs and the next open is not a no-op.
function closeDialog(name) {
  window.document.querySelector(".ws-about-dialog__close")?.click();
  check(name, window.document.querySelector(".ws-about-dialog-overlay") === null);
}

// No service registered: the dialog still opens, with updates unavailable.
check("the About action is registered", await Commands.execute(ABOUT));
check(
  "without an update service the button reads unavailable",
  updateButton()?.textContent === "Desktop updates unavailable" && updateButton()?.disabled === true,
);
closeDialog("Close dismisses the dialog without an update service");

// Lets the service's awaits and the dialog's re-render run.
function settle() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function buttonReads(text, enabled) {
  const button = updateButton();
  return button?.textContent === text && button.disabled === !enabled;
}

// The desktop app's registered service, with each answer scripted per case.
let checks = 0;
let supported = true;
let answer = async () => null;
const updates = new UpdateService({
  desktop: true,
  supported: async () => supported,
  currentVersion: async () => "0.3.0",
  check: () => {
    checks += 1;
    return answer();
  },
  relaunch: async () => undefined,
});
const registration = registerService(UPDATES, () => updates);

await Commands.execute(ABOUT);
check(
  "with the desktop update service the button offers the check",
  buttonReads("Check for Updates...", true),
);

// A click runs one check; the dialog follows its phases while open.
let finishCheck;
answer = () =>
  new Promise((resolve) => {
    finishCheck = resolve;
  });
updateButton()?.click();
await settle();
check("the button runs the update check", checks === 1);
check("a running check disables the button", buttonReads("Checking for Updates...", false));
finishCheck(null);
await settle();
check("a finished check offers the check again", buttonReads("Check for Updates...", true));

// A failed check offers the retry, which runs the check again.
answer = async () => {
  throw new Error("offline");
};
updateButton()?.click();
await settle();
check("a failed check offers the retry", buttonReads("Retry update check", true));
answer = async () => null;
updateButton()?.click();
await settle();
check("the retry runs the check", checks === 3);
check("a passing retry offers the check", buttonReads("Check for Updates...", true));

// An available update, dismissed elsewhere, is offered again from About,
// and choosing it closes the dialog.
answer = async () => ({
  currentVersion: "0.3.0",
  version: "0.4.0",
  body: "",
  downloadAndInstall: async () => undefined,
  close: async () => undefined,
});
updateButton()?.click();
await settle();
check("an available update is offered", buttonReads("Download Update", true));
updates.remindLater();
check("a dismissed update is still offered", buttonReads("Download Update", true));
updateButton()?.click();
check("showing the update makes it available", updates.snapshot.phase === "available");
check(
  "showing the update closes the dialog",
  window.document.querySelector(".ws-about-dialog-overlay") === null,
);

// While an update downloads and installs, the button offers no new check:
// a check would close the update handle the install is using.
let finishInstall;
answer = async () => ({
  currentVersion: "0.3.0",
  version: "0.4.0",
  body: "",
  downloadAndInstall: () =>
    new Promise((resolve) => {
      finishInstall = resolve;
    }),
  close: async () => undefined,
});
await updates.checkNow();
const installing = updates.install();
await settle();
await Commands.execute(ABOUT);
check("a running install disables the button", buttonReads("Installing Update...", false));
closeDialog("Close dismisses the dialog during an install");
finishInstall();
await installing;

// A package-managed install leaves updates to the package manager.
supported = false;
await updates.checkNow();
await Commands.execute(ABOUT);
check(
  "a package-managed install disables the button",
  buttonReads("Updates are managed by your package manager", false),
);
closeDialog("Close dismisses the dialog with the update service");
registration.dispose();
updates.dispose();

if (failures.length > 0) {
  console.error(`about-dialog: ${failures.length} failure(s)`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("about-dialog: all assertions passed");
