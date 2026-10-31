import assert from "node:assert/strict";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  gatewayBinaryName,
  gatewaySidecarName,
  gatewaySidecarPath,
  markGatewaySidecarPinned,
  readGatewaySidecarPin,
  removeGatewaySidecar,
  stageGatewaySidecar,
} from "./stage-gateway-sidecar.mjs";

test("maps the Windows target to Tauri's suffixed executable name", () => {
  assert.equal(
    gatewaySidecarName("x86_64-pc-windows-msvc"),
    "runvil-gateway-x86_64-pc-windows-msvc.exe",
  );
  assert.equal(
    gatewayBinaryName("x86_64-pc-windows-msvc"),
    "runvil-gateway.exe",
  );
});

test("maps the Linux target to Tauri's suffix without an extension", () => {
  assert.equal(
    gatewaySidecarName("x86_64-unknown-linux-gnu"),
    "runvil-gateway-x86_64-unknown-linux-gnu",
  );
  assert.equal(
    gatewayBinaryName("x86_64-unknown-linux-gnu"),
    "runvil-gateway",
  );
});

test("maps every valid non-Windows target without an extension", () => {
  for (const target of [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
  ]) {
    assert.equal(gatewayBinaryName(target), "runvil-gateway");
    assert.equal(
      gatewaySidecarName(target),
      `runvil-gateway-${target}`,
    );
  }
});

test("maps other valid Windows targets with an executable extension", () => {
  assert.equal(
    gatewaySidecarName("x86_64-pc-windows-gnu"),
    "runvil-gateway-x86_64-pc-windows-gnu.exe",
  );
});

test("rejects a malformed target triple", () => {
  assert.throws(() => gatewaySidecarName("../outside"), /invalid target triple/);
});

test("reports the one path tauri.conf.json's externalBin resolves", () => {
  const root = join("repo");
  assert.equal(
    gatewaySidecarPath({ root, target: "x86_64-pc-windows-msvc" }),
    join(
      root,
      "crates",
      "workshop",
      "desktop",
      "binaries",
      "runvil-gateway-x86_64-pc-windows-msvc.exe",
    ),
  );
  assert.equal(
    gatewaySidecarPath({ root, target: "aarch64-apple-darwin" }),
    join(
      root,
      "crates",
      "workshop",
      "desktop",
      "binaries",
      "runvil-gateway-aarch64-apple-darwin",
    ),
  );
});

test("rejects a missing source binary", () => {
  const root = mkdtempSync(join(tmpdir(), "runvil-sidecar-"));
  try {
    assert.throws(
      () =>
        stageGatewaySidecar({
          root,
          target: "x86_64-pc-windows-msvc",
          source: join(root, "target", "debug", "runvil-gateway.exe"),
        }),
      /source binary does not exist/,
    );
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});

test("rejects a source binary whose platform name mismatches the target", () => {
  const root = mkdtempSync(join(tmpdir(), "runvil-sidecar-"));
  const source = join(root, "target", "debug", "runvil-gateway");
  try {
    mkdirSync(join(root, "target", "debug"), { recursive: true });
    writeFileSync(source, "linux gateway");
    assert.throws(
      () =>
        stageGatewaySidecar({
          root,
          target: "x86_64-pc-windows-msvc",
          source,
        }),
      /source binary must be named runvil-gateway\.exe/,
    );
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});

test("stages and removes the real source file under Tauri's target name", () => {
  const root = mkdtempSync(join(tmpdir(), "runvil-sidecar-"));
  const source = join(root, "target", "debug", "runvil-gateway.exe");
  try {
    mkdirSync(join(root, "target", "debug"), { recursive: true });
    writeFileSync(source, "compiled gateway");

    const staged = stageGatewaySidecar({
      root,
      target: "x86_64-pc-windows-msvc",
      source,
    });
    assert.equal(
      staged,
      join(
        root,
        "crates",
        "workshop",
        "desktop",
        "binaries",
        "runvil-gateway-x86_64-pc-windows-msvc.exe",
      ),
    );
    assert.equal(readFileSync(staged, "utf8"), "compiled gateway");

    assert.equal(
      removeGatewaySidecar({
        root,
        target: "x86_64-pc-windows-msvc",
      }),
      staged,
    );
    assert.throws(() => readFileSync(staged), /ENOENT/);
    assert.throws(() => readFileSync(`${staged}.pinned`), /ENOENT/);
    assert.equal(
      removeGatewaySidecar({
        root,
        target: "x86_64-pc-windows-msvc",
      }),
      staged,
    );
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});

test("the staged sidecar keeps an executable mode for the Linux bundlers", () => {
  // `copyFileSync` writes the destination with the default mode, so without
  // an explicit `chmodSync` the staged Gateway is 0644 and Tauri's deb and
  // AppImage bundlers carry that through to `usr/bin`. The assertion is
  // skipped on Windows, where `chmodSync` only toggles the read-only flag and
  // `statSync().mode` reports 0666 for any writable file. CI runs these tools'
  // tests on Linux and Windows, so the assertion is live on the platform that
  // ships the deb.
  const root = mkdtempSync(join(tmpdir(), "runvil-sidecar-"));
  const source = join(root, "target", "debug", "runvil-gateway");
  const target = "x86_64-unknown-linux-gnu";
  try {
    mkdirSync(join(root, "target", "debug"), { recursive: true });
    writeFileSync(source, "downloaded gateway");
    // Start from a mode with no execute bits at all, so a passing assertion
    // cannot come from the umask or from the source file's own mode.
    chmodSync(source, 0o644);

    const staged = stageGatewaySidecar({ root, target, source });

    if (process.platform === "win32") {
      assert.equal(
        statSync(staged).mode & 0o777,
        0o666,
        "Windows reports 0666 for any writable file, so the execute bit cannot be asserted here",
      );
      return;
    }
    assert.equal(statSync(staged).mode & 0o777, 0o755);
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});

test("marking a staged sidecar pinned records which kind it was", () => {
  const root = mkdtempSync(join(tmpdir(), "runvil-sidecar-"));
  const source = join(root, "target", "debug", "runvil-gateway");
  try {
    mkdirSync(join(root, "target", "debug"), { recursive: true });
    writeFileSync(source, "downloaded gateway");
    const target = "x86_64-unknown-linux-gnu";
    const staged = stageGatewaySidecar({ root, target, source });
    const marker = markGatewaySidecarPinned({ root, target, kind: "release", tag: "gateway-v0.4.0" });
    assert.equal(marker, `${staged}.pinned`);
    assert.deepEqual(readGatewaySidecarPin({ root, target }), {
      kind: "release",
      tag: "gateway-v0.4.0",
    });
    // Removal takes the marker with the sidecar, so a later build is free
    // to refresh from the workspace again.
    removeGatewaySidecar({ root, target });
    assert.throws(() => readFileSync(marker), /ENOENT/);
    assert.equal(readGatewaySidecarPin({ root, target }), null);
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});

test("a stub marker is distinguishable from a release marker", () => {
  const root = mkdtempSync(join(tmpdir(), "runvil-sidecar-"));
  const source = join(root, "target", "debug", "runvil-gateway");
  const target = "x86_64-apple-darwin";
  try {
    mkdirSync(join(root, "target", "debug"), { recursive: true });
    writeFileSync(source, "placeholder");
    stageGatewaySidecar({ root, target, source });
    markGatewaySidecarPinned({ root, target, kind: "stub" });
    assert.equal(readGatewaySidecarPin({ root, target }).kind, "stub");
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});

test("a hand-written or corrupt marker reads as unknown, not as a release", () => {
  const root = mkdtempSync(join(tmpdir(), "runvil-sidecar-"));
  const source = join(root, "target", "debug", "runvil-gateway");
  const target = "x86_64-apple-darwin";
  try {
    mkdirSync(join(root, "target", "debug"), { recursive: true });
    writeFileSync(source, "placeholder");
    const staged = stageGatewaySidecar({ root, target, source });
    writeFileSync(`${staged}.pinned`, "this sidecar came from somewhere\n");
    assert.equal(readGatewaySidecarPin({ root, target }).kind, "unknown");
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});
