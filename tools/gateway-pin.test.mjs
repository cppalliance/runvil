import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  PIN_DIRECTORY,
  PLACEHOLDER,
  TARGETS,
  parsePin,
  placeholderFields,
  readPin,
  releaseAssetUrl,
  renderPin,
  requireFilledPin,
  specPath,
  targetEntry,
} from "./gateway-pin.mjs";

const WINDOWS = "x86_64-pc-windows-msvc";
const LINUX = "x86_64-unknown-linux-gnu";

/** A filled pin: every field a consumer reads names a real release. */
function filledPin() {
  const targets = {};
  for (const target of TARGETS) {
    targets[target] = {
      asset: `runvil-gateway-${target}.tar.gz`,
      sha256: "a".repeat(64),
      size: 1234,
    };
  }
  return {
    pin_format: 1,
    repository: "cppalliance/runvil-gateway",
    tag: "gateway-v0.4.0",
    version: "0.4.0",
    executable: "runvil-gateway",
    min_gateway: "0.4.0",
    spec: {
      openapi: { path: "gateway-pin/openapi.yaml", sha256: "b".repeat(64) },
      asyncapi: { path: "gateway-pin/asyncapi.yaml", sha256: "c".repeat(64) },
    },
    targets,
  };
}

/** The committed pin, read once: the repository's real file. */
function committedPin() {
  return readPin();
}

/** A temporary repository root holding `pin` at the pin path. */
function repositoryWith(pin) {
  const root = mkdtempSync(join(tmpdir(), "runvil-gateway-pin-"));
  mkdirSync(join(root, PIN_DIRECTORY), { recursive: true });
  writeFileSync(join(root, PIN_DIRECTORY, "pin.json"), renderPin(pin));
  return root;
}

test("the committed pin records all five release targets", () => {
  const pin = committedPin();
  assert.deepEqual(Object.keys(pin["targets"]).sort(), [...TARGETS].sort());
  assert.equal(pin["executable"], "runvil-gateway");
});

test("the committed pin is complete and names a published release", () => {
  const pin = committedPin();
  assert.deepEqual(placeholderFields(pin), []);
  assert.equal(requireFilledPin(pin, { source: "gateway-pin/pin.json" }), undefined);
});

test("a pin with no spec entry is complete, because the release publishes none", () => {
  const pin = filledPin();
  pin["spec"] = {};
  assert.deepEqual(placeholderFields(pin), []);
  assert.equal(requireFilledPin(pin, { source: "gateway-pin/pin.json" }), undefined);
});

test("a spec entry that still holds a placeholder is refused by name", () => {
  const pin = filledPin();
  pin["spec"]["openapi"]["sha256"] = PLACEHOLDER;
  assert.deepEqual(placeholderFields(pin), ["spec.openapi.sha256"]);
  assert.throws(() => requireFilledPin(pin, { source: "gateway-pin/pin.json" }), /spec\.openapi\.sha256/);
});

test("a placeholder pin is refused by name, not fetched", () => {
  const pin = filledPin();
  pin["tag"] = PLACEHOLDER;
  pin["min_gateway"] = PLACEHOLDER;
  pin["targets"][WINDOWS] = { asset: PLACEHOLDER, sha256: PLACEHOLDER, size: 0 };
  assert.throws(
    () => requireFilledPin(pin, { source: "gateway-pin/pin.json" }),
    /still holds placeholders/,
  );
});

test("a target-scoped refusal names only that target's unfilled fields", () => {
  const pin = filledPin();
  pin["targets"][WINDOWS]["sha256"] = PLACEHOLDER;
  assert.equal(requireFilledPin(pin, { target: LINUX }), undefined);
  assert.throws(() => requireFilledPin(pin, { target: WINDOWS }), /targets\.x86_64-pc-windows-msvc\.sha256/);
});

test("a filled pin passes the refusal", () => {
  assert.equal(requireFilledPin(filledPin(), { target: WINDOWS }), undefined);
  assert.deepEqual(placeholderFields(filledPin()), []);
});

test("parsing rejects an unknown pin format", () => {
  const text = renderPin({ ...filledPin(), pin_format: 2 });
  assert.throws(() => parsePin(text, "pin.json"), /pin_format must be 1/);
});

test("parsing rejects a missing target", () => {
  const pin = filledPin();
  delete pin["targets"][LINUX];
  assert.throws(() => parsePin(renderPin(pin), "pin.json"), /targets\.x86_64-unknown-linux-gnu/);
});

test("parsing rejects a target outside the five released triples", () => {
  const pin = filledPin();
  pin["targets"]["aarch64-pc-windows-msvc"] = {
    asset: "x",
    sha256: "d".repeat(64),
    size: 1,
  };
  assert.throws(() => parsePin(renderPin(pin), "pin.json"), /not one of the five release targets/);
});

test("parsing rejects a negative size rather than trusting it", () => {
  const pin = filledPin();
  pin["targets"][LINUX]["size"] = -1;
  assert.throws(() => parsePin(renderPin(pin), "pin.json"), /size must be a non-negative integer/);
});

test("an unknown target triple is refused by name", () => {
  assert.throws(() => targetEntry(filledPin(), "sparc-unknown-linux-gnu"), /records no target/);
});

test("rendering is canonical, so a rewrite only changes what changed", () => {
  const pin = filledPin();
  const rendered = renderPin(pin);
  assert.ok(rendered.endsWith("}\n"));
  assert.equal(renderPin(JSON.parse(rendered)), rendered);
});

test("the release URL names the pinned tag and asset", () => {
  assert.equal(
    releaseAssetUrl(filledPin(), "runvil-gateway-aarch64-apple-darwin.tar.gz"),
    "https://github.com/cppalliance/runvil-gateway/releases/download/gateway-v0.4.0/runvil-gateway-aarch64-apple-darwin.tar.gz",
  );
});

test("reading a repository's pin finds the file under gateway-pin", () => {
  const root = repositoryWith(filledPin());
  try {
    const pin = readPin(root);
    assert.equal(pin["tag"], "gateway-v0.4.0");
    assert.equal(specPath(root, "openapi.yaml"), join(root, "gateway-pin", "openapi.yaml"));
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});

test("a missing pin file reports the path it looked at", () => {
  const root = mkdtempSync(join(tmpdir(), "runvil-gateway-pin-"));
  try {
    assert.throws(() => readPin(root), /cannot read the Gateway pin at .*pin\.json/);
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});

test("the committed pin records no spec, because the release publishes none", () => {
  const pin = committedPin();
  assert.deepEqual(pin["spec"], {}, "the Gateway repository publishes no spec to record");
});