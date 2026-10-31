import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { gzipSync } from "node:zlib";
import test from "node:test";

import {
  fetchGatewaySidecar,
  hostTarget,
  sha256Hex,
  verifyAsset,
} from "./fetch-gateway-sidecar.mjs";
import { PLACEHOLDER, TARGETS, parsePin, placeholderReport, renderPin } from "./gateway-pin.mjs";

const WINDOWS = "x86_64-pc-windows-msvc";
const LINUX = "x86_64-unknown-linux-gnu";
const BINARY = "runvil-gateway";
const EXECUTABLE_BYTES = "the gateway executable";

/** A gzipped tar holding one `runvil-gateway` under a release directory name. */
function releaseArchive() {
  const header = Buffer.alloc(512, 0);
  header.write("runvil-gateway-x86_64-unknown-linux-gnu/runvil-gateway", 0, 100, "utf8");
  header.write("0000644\0", 100, "utf8");
  header.write("0000000\0", 108, "utf8");
  header.write("0000000\0", 116, "utf8");
  const size = Buffer.byteLength(EXECUTABLE_BYTES);
  header.write(`${size.toString(8).padStart(11, "0")}\0`, 124, "utf8");
  header.write("00000000000\0", 136, "utf8");
  header.write("        ", 148, "utf8");
  header.write("0", 156, "utf8");
  header.write("ustar\0", 257, "utf8");
  header.write("00", 263, "utf8");
  const body = Buffer.alloc(512, 0);
  body.write(EXECUTABLE_BYTES, 0, "utf8");
  return gzipSync(Buffer.concat([header, body, Buffer.alloc(1024, 0)]));
}

/** A pin whose Linux target matches `bytes`. */
function pinFor(bytes) {
  const targets = {};
  for (const target of TARGETS) {
    targets[target] = { asset: "PLACEHOLDER", sha256: PLACEHOLDER, size: 0 };
  }
  targets[LINUX] = {
    asset: "runvil-gateway-x86_64-unknown-linux-gnu.tar.gz",
    sha256: sha256Hex(bytes),
    size: bytes.length,
  };
  return {
    pin_format: 1,
    repository: "cppalliance/runvil-gateway",
    tag: "gateway-v0.4.0",
    version: "0.4.0",
    executable: "runvil-gateway",
    min_gateway: "0.4.0",
    spec: {
      openapi: { path: "gateway-pin/openapi.yaml", sha256: "a".repeat(64) },
      asyncapi: { path: "gateway-pin/asyncapi.yaml", sha256: "b".repeat(64) },
    },
    targets,
  };
}

/** A pin whose every field is still a placeholder. */
function placeholderPin() {
  return {
    pin_format: 1,
    repository: "cppalliance/runvil-gateway",
    tag: PLACEHOLDER,
    version: PLACEHOLDER,
    executable: BINARY,
    min_gateway: PLACEHOLDER,
    spec: {
      openapi: { path: "gateway-pin/openapi.yaml", sha256: PLACEHOLDER },
      asyncapi: { path: "gateway-pin/asyncapi.yaml", sha256: PLACEHOLDER },
    },
    targets: Object.fromEntries(
      TARGETS.map((target) => [target, { asset: PLACEHOLDER, sha256: PLACEHOLDER, size: 0 }]),
    ),
  };
}

/** A pin whose every field is filled, so a consumer may use it. */
function filledPin() {
  const pin = pinFor(Buffer.from("unused"));
  for (const target of TARGETS) {
    pin["targets"][target] = {
      asset: `runvil-gateway-${target}.tar.gz`,
      sha256: "d".repeat(64),
      size: 10,
    };
  }
  return pin;
}

/** A temporary repository root holding `pin`, with a download cache beside it. */
function sandbox(pin = pinFor(releaseArchive())) {
  const base = mkdtempSync(join(tmpdir(), "runvil-fetch-gateway-"));
  const root = join(base, "repo");
  mkdirSync(join(root, "gateway-pin"), { recursive: true });
  writeFileSync(join(root, "gateway-pin", "pin.json"), renderPin(pin));
  return { base, root, cache: join(base, "cache") };
}

function cleanup(base) {
  rmSync(base, { force: true, recursive: true });
}

test("stub mode stages a file at Tauri's path without downloading", async () => {
  const { base, root, cache } = sandbox();
  try {
    let downloads = 0;
    const result = await fetchGatewaySidecar({
      root,
      target: LINUX,
      cacheDir: cache,
      stub: true,
      download: async () => {
        downloads += 1;
        return Buffer.alloc(0);
      },
    });
    assert.equal(downloads, 0, "the stub path downloads nothing");
    assert.equal(
      result.staged,
      join(root, "crates", "workshop", "desktop", "binaries", "runvil-gateway-x86_64-unknown-linux-gnu"),
    );
    assert.ok(existsSync(result.staged));
    assert.match(readFileSync(result.staged, "utf8"), /not an executable/);
    assert.equal(result.stub, true);
    assert.ok(
      existsSync(`${result.staged}.pinned`),
      "a stub is marked so the desktop build script leaves it in place",
    );
  } finally {
    cleanup(base);
  }
});

test("stub mode needs no pin, so a fresh checkout can compile", async () => {
  const base = mkdtempSync(join(tmpdir(), "runvil-fetch-gateway-"));
  try {
    const result = await fetchGatewaySidecar({
      root: base,
      target: WINDOWS,
      cacheDir: join(base, "cache"),
      stub: true,
    });
    assert.equal(
      result.staged,
      join(base, "crates", "workshop", "desktop", "binaries", "runvil-gateway-x86_64-pc-windows-msvc.exe"),
    );
  } finally {
    cleanup(base);
  }
});

test("a real fetch verifies the archive, extracts it, and stages it", async () => {
  const archive = releaseArchive();
  const { base, root, cache } = sandbox(pinFor(archive));
  try {
    const result = await fetchGatewaySidecar({
      root,
      target: LINUX,
      cacheDir: cache,
      download: async () => archive,
    });
    assert.equal(result.downloaded, true);
    assert.equal(result.tag, "gateway-v0.4.0");
    assert.equal(readFileSync(result.staged, "utf8"), EXECUTABLE_BYTES);
    assert.ok(
      existsSync(`${result.staged}.pinned`),
      "a verified download is marked so the desktop build script keeps it",
    );
    // The archive is cached under the tag, so the next run refetches nothing.
    assert.ok(existsSync(join(cache, "gateway-v0.4.0", "runvil-gateway-x86_64-unknown-linux-gnu.tar.gz")));
    let downloads = 0;
    const second = await fetchGatewaySidecar({
      root,
      target: LINUX,
      cacheDir: cache,
      download: async () => {
        downloads += 1;
        return archive;
      },
    });
    assert.equal(downloads, 0, "the cached archive is reused");
    assert.equal(second.downloaded, false);
  } finally {
    cleanup(base);
  }
});

test("a checksum mismatch fails loudly and reports both hashes", async () => {
  const archive = releaseArchive();
  const pin = pinFor(archive);
  pin["targets"][LINUX]["sha256"] = createHash("sha256").update("other").digest("hex");
  const { base, root, cache } = sandbox(pin);
  try {
    await assert.rejects(
      () => fetchGatewaySidecar({ root, target: LINUX, cacheDir: cache, download: async () => archive }),
      (error) => {
        assert.match(error.message, /sha256 is [0-9a-f]{64}, but the pin records [0-9a-f]{64}/);
        assert.ok(error.message.includes(sha256Hex(archive)), error.message);
        assert.ok(error.message.includes(pin["targets"][LINUX]["sha256"]), error.message);
        return true;
      },
    );
    assert.equal(
      existsSync(join(root, "crates", "workshop", "desktop", "binaries", "runvil-gateway-x86_64-unknown-linux-gnu")),
      false,
      "a mismatch stages nothing",
    );
  } finally {
    cleanup(base);
  }
});

test("a size mismatch fails before the hash is reported as the problem", () => {
  const bytes = Buffer.from("payload");
  assert.throws(
    () => verifyAsset(bytes, { sha256: sha256Hex(bytes), size: 99 }, "asset.tar.gz"),
    /is 7 bytes, but the pin records 99 bytes/,
  );
});

test("a placeholder pin refuses to fetch and names the fields to fill", async () => {
  const { base, root, cache } = sandbox(parsePin(renderPin(placeholderPin())));
  try {
    await assert.rejects(
      () => fetchGatewaySidecar({ root, target: LINUX, cacheDir: cache, download: async () => Buffer.alloc(0) }),
      /still holds placeholders/,
    );
  } finally {
    cleanup(base);
  }
});

test("an archive missing the executable names what it expected", async () => {
  const header = Buffer.alloc(512, 0);
  header.write("pkg/README.md", 0, 100, "utf8");
  header.write("0000644\0", 100, "utf8");
  header.write("0000000\0", 108, "utf8");
  header.write("0000000\0", 116, "utf8");
  header.write("00000000003\0", 124, "utf8");
  header.write("00000000000\0", 136, "utf8");
  header.write("        ", 148, "utf8");
  header.write("0", 156, "utf8");
  const body = Buffer.alloc(512, 0);
  body.write("abc", 0, "utf8");
  const archive = gzipSync(Buffer.concat([header, body, Buffer.alloc(1024, 0)]));
  const { base, root, cache } = sandbox(pinFor(archive));
  try {
    await assert.rejects(
      () => fetchGatewaySidecar({ root, target: LINUX, cacheDir: cache, download: async () => archive }),
      /holds no runvil-gateway/,
    );
  } finally {
    cleanup(base);
  }
});

test("attestation verification calls gh release verify-asset once", async () => {
  const archive = releaseArchive();
  const { base, root, cache } = sandbox(pinFor(archive));
  try {
    const calls = [];
    await fetchGatewaySidecar({
      root,
      target: LINUX,
      cacheDir: cache,
      verifyAttestation: true,
      download: async () => archive,
      gh: (args) => {
        calls.push(args);
        return "";
      },
    });
    assert.equal(calls.length, 1);
    assert.deepEqual(calls[0].slice(0, 2), ["release", "verify-asset"]);
    assert.ok(calls[0].includes("gateway-v0.4.0"));
    assert.ok(calls[0].includes("cppalliance/runvil-gateway"));
  } finally {
    cleanup(base);
  }
});

test("attestation is skipped unless asked for", async () => {
  const archive = releaseArchive();
  const { base, root, cache } = sandbox(pinFor(archive));
  try {
    let calls = 0;
    await fetchGatewaySidecar({
      root,
      target: LINUX,
      cacheDir: cache,
      download: async () => archive,
      gh: () => {
        calls += 1;
        return "";
      },
    });
    assert.equal(calls, 0);
  } finally {
    cleanup(base);
  }
});

test("the placeholder report names every unfilled field and the complete case", () => {
  const report = placeholderReport(placeholderPin(), { source: "gateway-pin/pin.json" });
  assert.match(report, /gateway-pin\/pin\.json still has 20 unfilled field/);
  for (const field of [
    "tag",
    "version",
    "min_gateway",
    "spec.openapi.sha256",
    "spec.asyncapi.sha256",
    "targets.aarch64-unknown-linux-gnu.sha256",
    "targets.x86_64-pc-windows-msvc.size",
  ]) {
    assert.ok(report.includes(field), `${field} missing from:\n${report}`);
  }
  assert.equal(placeholderReport(filledPin()), "the Gateway pin is complete");
});

test("host triple is one of the five released targets or a named refusal", () => {
  const triple = hostTarget();
  assert.ok(TARGETS.includes(triple) || triple === "aarch64-pc-windows-msvc", triple);
});

test("a target the pin does not record is refused before any download", async () => {
  const { base, root, cache } = sandbox();
  try {
    let downloads = 0;
    await assert.rejects(
      () =>
        fetchGatewaySidecar({
          root,
          target: "sparc-unknown-linux-gnu",
          cacheDir: cache,
          download: async () => {
            downloads += 1;
            return Buffer.alloc(0);
          },
        }),
      /unsupported Gateway target sparc-unknown-linux-gnu/,
    );
    assert.equal(downloads, 0);
  } finally {
    cleanup(base);
  }
});
