import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  MANIFEST_ASSET,
  SPEC_ASSETS,
  bumpGateway,
  buildPin,
  downloadRelease,
  renderDiff,
  validateManifest,
} from "./bump-gateway.mjs";
import { PLACEHOLDER, TARGETS, pinPath, readPin, specPath } from "./gateway-pin.mjs";

const TAG = "gateway-v0.4.0";
const OPENAPI = "openapi: 3.1.0\ninfo:\n  title: Runvil Gateway\n";
const ASYNCAPI = "asyncapi: 3.0.0\ninfo:\n  title: Runvil Gateway realtime\n";

function sha256(text) {
  return createHash("sha256").update(text).digest("hex");
}

/** A manifest shaped the way the Gateway repository publishes one. */
function manifest() {
  const targets = {};
  for (const target of TARGETS) {
    targets[target] = {
      asset: `runvil-gateway-${target}.tar.gz`,
      sha256: "e".repeat(64),
      size: 4096,
    };
  }
  return {
    version: "0.4.0",
    min_gateway: "0.4.0",
    executable: "runvil-gateway",
    spec: {
      openapi: { sha256: sha256(OPENAPI) },
      asyncapi: { sha256: sha256(ASYNCAPI) },
    },
    targets,
  };
}

/** A sandbox repository root with a placeholder pin and placeholder specs. */
function sandbox() {
  const base = mkdtempSync(join(tmpdir(), "runvil-bump-gateway-"));
  const root = join(base, "repo");
  mkdirSync(join(root, "gateway-pin"), { recursive: true });
  writeFileSync(
    pinPath(root),
    `${JSON.stringify(
      {
        pin_format: 1,
        repository: "cppalliance/runvil-gateway",
        tag: PLACEHOLDER,
        version: PLACEHOLDER,
        executable: "runvil-gateway",
        min_gateway: PLACEHOLDER,
        spec: {
          openapi: { path: "gateway-pin/openapi.yaml", sha256: PLACEHOLDER },
          asyncapi: { path: "gateway-pin/asyncapi.yaml", sha256: PLACEHOLDER },
        },
        targets: Object.fromEntries(
          TARGETS.map((target) => [target, { asset: PLACEHOLDER, sha256: PLACEHOLDER, size: 0 }]),
        ),
      },
      null,
      2,
    )}\n`,
  );
  for (const { name } of SPEC_ASSETS) {
    writeFileSync(specPath(root, name), "# PLACEHOLDER\n");
  }
  return { base, root };
}

/**
 * A `gh` stand-in: `release view` succeeds for `foundTag`, and `release
 * download` materializes the requested patterns into `--dir`.
 */
function fakeGh({ foundTag = TAG, body = manifest(), specs = { "openapi.yaml": OPENAPI, "asyncapi.yaml": ASYNCAPI }, calls = [] } = {}) {
  return (args) => {
    calls.push(args);
    if (args[0] === "release" && args[1] === "view") {
      if (args[2] !== foundTag) {
        throw new Error(`gh release view ${args[2]} failed: release not found`);
      }
      return "{}";
    }
    if (args[0] === "release" && args[1] === "download") {
      const dir = args[args.indexOf("--dir") + 1];
      const patterns = args.flatMap((arg, index) => (arg === "--pattern" ? [args[index + 1]] : []));
      for (const pattern of patterns) {
        if (pattern === MANIFEST_ASSET) {
          writeFileSync(join(dir, MANIFEST_ASSET), JSON.stringify(body, null, 2));
        } else if (specs[pattern] !== undefined) {
          writeFileSync(join(dir, pattern), specs[pattern]);
        } else {
          throw new Error(`no asset named ${pattern}`);
        }
      }
      return "";
    }
    throw new Error(`unexpected gh call: ${args.join(" ")}`);
  };
}

test("a bump rewrites the pin and both spec copies", () => {
  const { base, root } = sandbox();
  try {
    const result = bumpGateway({ root, tag: TAG, gh: fakeGh() });
    assert.equal(result.written, true);
    const pin = readPin(root);
    assert.equal(pin["tag"], TAG);
    assert.equal(pin["version"], "0.4.0");
    assert.equal(pin["min_gateway"], "0.4.0");
    assert.equal(pin["spec"]["openapi"]["sha256"], sha256(OPENAPI));
    assert.equal(readFileSync(specPath(root, "openapi.yaml"), "utf8"), OPENAPI);
    assert.equal(readFileSync(specPath(root, "asyncapi.yaml"), "utf8"), ASYNCAPI);
    assert.equal(pin["targets"]["aarch64-apple-darwin"]["size"], 4096);
  } finally {
    rmSync(base, { force: true, recursive: true });
  }
});

test("a tag that cannot be found writes nothing at all", () => {
  const { base, root } = sandbox();
  try {
    const before = readFileSync(pinPath(root), "utf8");
    assert.throws(
      () => bumpGateway({ root, tag: "gateway-v9.9.9", gh: fakeGh() }),
      /Gateway release gateway-v9\.9\.9 was not found/,
    );
    assert.equal(readFileSync(pinPath(root), "utf8"), before, "the pin is untouched");
    assert.equal(readFileSync(specPath(root, "openapi.yaml"), "utf8"), "# PLACEHOLDER\n");
    assert.deepEqual(
      readdirSync(join(root, "gateway-pin")).sort(),
      ["asyncapi.yaml", "openapi.yaml", "pin.json"],
      "no temp file is left behind",
    );
  } finally {
    rmSync(base, { force: true, recursive: true });
  }
});

test("a spec that disagrees with the manifest hash fails before any write", () => {
  const { base, root } = sandbox();
  try {
    const before = readFileSync(pinPath(root), "utf8");
    assert.throws(
      () =>
        bumpGateway({
          root,
          tag: TAG,
          gh: fakeGh({ specs: { "openapi.yaml": "tampered", "asyncapi.yaml": ASYNCAPI } }),
        }),
      /openapi\.yaml for gateway-v0\.4\.0 has sha256 [0-9a-f]{64}, but sidecar\.json records [0-9a-f]{64}/,
    );
    assert.equal(readFileSync(pinPath(root), "utf8"), before);
    assert.equal(readFileSync(specPath(root, "openapi.yaml"), "utf8"), "# PLACEHOLDER\n");
  } finally {
    rmSync(base, { force: true, recursive: true });
  }
});

test("a dry run prints the diff and writes nothing", () => {
  const { base, root } = sandbox();
  try {
    const before = readFileSync(pinPath(root), "utf8");
    const result = bumpGateway({ root, tag: TAG, gh: fakeGh(), dryRun: true });
    assert.equal(result.written, false);
    assert.equal(readFileSync(pinPath(root), "utf8"), before);
    assert.ok(result.diffs.some((diff) => diff.includes("+  \"tag\": \"gateway-v0.4.0\"")), result.diffs.join("\n"));
  } finally {
    rmSync(base, { force: true, recursive: true });
  }
});

test("a manifest missing a target is refused by field name", () => {
  const body = manifest();
  delete body["targets"]["aarch64-unknown-linux-gnu"];
  assert.throws(
    () => validateManifest(body, TAG),
    /targets\.aarch64-unknown-linux-gnu\.asset, targets\.aarch64-unknown-linux-gnu\.sha256, targets\.aarch64-unknown-linux-gnu\.size/,
  );
});

test("a manifest whose version disagrees with the tag is refused", () => {
  const body = manifest();
  body["version"] = "0.5.0";
  assert.throws(() => validateManifest(body, TAG), /version 0\.5\.0 does not match tag gateway-v0\.4\.0/);
});

test("a manifest naming a placeholder checksum is refused rather than pinned", () => {
  const body = manifest();
  body["targets"]["x86_64-pc-windows-msvc"]["sha256"] = PLACEHOLDER;
  assert.throws(() => downloadRelease({ tag: TAG, gh: fakeGh({ body }) }), /still names a placeholder checksum/);
});

test("the built pin carries every five target entries in release order", () => {
  const pin = buildPin(manifest(), TAG, "cppalliance/runvil-gateway");
  assert.deepEqual(Object.keys(pin["targets"]), TARGETS);
  assert.equal(pin["pin_format"], 1);
  assert.equal(pin["executable"], "runvil-gateway");
  assert.equal(pin["spec"]["asyncapi"]["path"], "gateway-pin/asyncapi.yaml");
});

test("the diff shows only the changed lines with a little context", () => {
  const before = "a\nb\nc\nd\n";
  const after = "a\nb\nCHANGED\nd\n";
  const diff = renderDiff("pin.json", before, after);
  assert.ok(diff.includes("-c"), diff);
  assert.ok(diff.includes("+CHANGED"), diff);
  assert.ok(diff.includes(" b"), diff);
  assert.equal(renderDiff("pin.json", before, before), "pin.json: unchanged");
});

test("a download leaves no scratch directory behind", () => {
  const before = readdirSync(tmpdir()).filter((name) => name.startsWith("runvil-bump-gateway-"));
  downloadRelease({ tag: TAG, gh: fakeGh() });
  const after = readdirSync(tmpdir()).filter((name) => name.startsWith("runvil-bump-gateway-"));
  assert.deepEqual(after, before);
});

test("a failed gh download propagates its message", () => {
  const gh = (args) => {
    if (args[1] === "view") {
      return "{}";
    }
    throw new Error("gh release download failed: HTTP 404");
  };
  assert.throws(() => downloadRelease({ tag: TAG, gh }), /HTTP 404/);
  assert.equal(existsSync(join(tmpdir(), "does-not-exist")), false);
});
