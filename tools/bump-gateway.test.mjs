import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  CHECKSUM_ASSET,
  DIST_MANIFEST_ASSET,
  SPEC_ASSETS,
  bumpGateway,
  buildPin,
  downloadRelease,
  executableName,
  parseChecksumSum,
  releaseAssetNames,
  releaseVersion,
  renderDiff,
  selectTargets,
  versionFromTag,
} from "./bump-gateway.mjs";
import { PLACEHOLDER, TARGETS, pinPath, readPin, specPath } from "./gateway-pin.mjs";

const TAG = "gateway-v0.4.0";
const REPOSITORY = "cppalliance/runvil-gateway";
const OPENAPI = "openapi: 3.1.0\ninfo:\n  title: Runvil Gateway\n";
const ASYNCAPI = "asyncapi: 3.0.0\ninfo:\n  title: Runvil Gateway realtime\n";

function sha256(text) {
  return createHash("sha256").update(text).digest("hex");
}

/** The asset name cargo-dist publishes for `target`. */
function assetFor(target) {
  const extension = target.includes("windows") ? ".zip" : ".tar.xz";
  return `gateway-${target}${extension}`;
}

/** A synthetic digest per target, so a mismatch is visible when one moves. */
function digestFor(target) {
  return createHash("sha256").update(`archive ${target}`).digest("hex");
}

/** The size the release reports for each archive, keyed by asset name. */
function sizesFor() {
  const sizes = new Map();
  for (const [index, target] of TARGETS.entries()) {
    sizes.set(assetFor(target), 4000 + index);
  }
  return sizes;
}

/** A `dist-manifest.json` shaped the way cargo-dist writes one. */
function distManifest({ specs = true } = {}) {
  const artifacts = {};
  const sums = [];
  for (const target of TARGETS) {
    const asset = assetFor(target);
    const digest = digestFor(target);
    artifacts[asset] = {
      name: asset,
      kind: "executable-zip",
      target_triples: [target],
      assets: [
        { name: "LICENSE", path: "LICENSE", kind: "license" },
        { name: "runvil-gateway", path: "runvil-gateway", kind: "executable" },
      ],
      checksum: `${asset}.sha256`,
      checksums: { sha256: digest },
    };
    sums.push(`${digest} *${asset}`);
  }
  artifacts["source.tar.gz"] = {
    name: "source.tar.gz",
    kind: "source-tarball",
    target_triples: [],
    assets: [],
    checksums: { sha256: "f".repeat(64) },
  };
  if (specs) {
    artifacts["openapi.yaml"] = {
      name: "openapi.yaml",
      kind: "extra-artifact",
      target_triples: [],
      assets: [],
      checksums: { sha256: sha256(OPENAPI) },
    };
    artifacts["asyncapi.yaml"] = {
      name: "asyncapi.yaml",
      kind: "extra-artifact",
      target_triples: [],
      assets: [],
      checksums: { sha256: sha256(ASYNCAPI) },
    };
    sums.push(`${sha256(OPENAPI)} *openapi.yaml`, `${sha256(ASYNCAPI)} *asyncapi.yaml`);
  }
  return {
    dist_version: "0.32.0",
    announcement_tag: TAG,
    releases: [{ app_name: "gateway", app_version: "0.4.0", artifacts: Object.keys(artifacts) }],
    artifacts,
    checksumSum: sums,
  };
}

/** The `sha256.sum` text matching `distManifest`. */
function checksumSum(manifest) {
  return `${manifest.checksumSum.join("\n")}\n`;
}

/**
 * The `gh release view` answer: each asset's name, size, and digest. A
 * `sizes` value of `null` models a release that states no size for that
 * asset.
 */
function releaseView({ manifest, sizes = sizesFor(), extras = {}, omitDigest = false }) {
  const assets = [];
  const push = (name, size, digest) => {
    const asset = { name, digest: `sha256:${digest}`, state: "uploaded" };
    if (Number.isInteger(size)) {
      asset.size = size;
    }
    if (omitDigest) {
      delete asset.digest;
    }
    assets.push(asset);
  };
  for (const [name, entry] of Object.entries(manifest.artifacts)) {
    push(name, sizes.has(name) ? sizes.get(name) : 1000, entry.checksums.sha256);
  }
  push(CHECKSUM_ASSET, 598, sha256(checksumSum(manifest)));
  push(DIST_MANIFEST_ASSET, 23099, sha256("manifest"));
  for (const [name, text] of Object.entries(extras)) {
    push(name, Buffer.byteLength(text), sha256(text));
  }
  return JSON.stringify({ tagName: TAG, name: TAG, assets });
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
 * A `gh` stand-in. `release view` answers with each asset's size and digest;
 * `release download` materializes the requested patterns into `--dir`, and
 * refuses a pattern the release does not publish, as `gh` does.
 */
function fakeGh({
  foundTag = TAG,
  manifest = distManifest(),
  specs = { "openapi.yaml": OPENAPI, "asyncapi.yaml": ASYNCAPI },
  sizes = sizesFor(),
  omitDigest = false,
  view = null,
  calls = [],
} = {}) {
  const listed = releaseView({ manifest, sizes, extras: specs, omitDigest });
  return (args) => {
    calls.push(args);
    if (args[0] === "release" && args[1] === "view") {
      if (args[2] !== foundTag) {
        throw new Error(`gh release view ${args[2]} failed: release not found`);
      }
      return view === null ? listed : view;
    }
    if (args[0] === "release" && args[1] === "download") {
      const dir = args[args.indexOf("--dir") + 1];
      const patterns = args.flatMap((arg, index) => (arg === "--pattern" ? [args[index + 1]] : []));
      const viewNames = releaseAssetNames(JSON.parse(listed));
      for (const pattern of patterns) {
        if (!viewNames.includes(pattern)) {
          throw new Error(`no asset named ${pattern}`);
        }
        if (pattern === CHECKSUM_ASSET) {
          writeFileSync(join(dir, CHECKSUM_ASSET), checksumSum(manifest));
        } else if (pattern === DIST_MANIFEST_ASSET) {
          writeFileSync(join(dir, DIST_MANIFEST_ASSET), JSON.stringify(manifest, null, 2));
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

test("a bump rewrites the pin and both spec copies when the release publishes them", () => {
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
    assert.equal(pin["targets"]["aarch64-apple-darwin"]["size"], 4001);
    assert.equal(pin["targets"]["aarch64-apple-darwin"]["sha256"], digestFor("aarch64-apple-darwin"));
  } finally {
    rmSync(base, { force: true, recursive: true });
  }
});

test("a release that publishes no spec is pinned with no spec entry and says so", () => {
  const { base, root } = sandbox();
  try {
    const printed = [];
    const result = bumpGateway({
      root,
      tag: TAG,
      gh: fakeGh({ manifest: distManifest({ specs: false }), specs: {} }),
      log: (line) => printed.push(line),
    });
    assert.equal(result.written, true);
    const pin = readPin(root);
    assert.deepEqual(pin["spec"], {}, "an absent spec is recorded as absent");
    assert.equal(pin["targets"]["x86_64-pc-windows-msvc"]["asset"], "gateway-x86_64-pc-windows-msvc.zip");
    assert.equal(
      printed.join("\n").includes("publishes no openapi.yaml and asyncapi.yaml"),
      true,
      printed.join("\n"),
    );
    assert.equal(
      readFileSync(specPath(root, "openapi.yaml"), "utf8"),
      "# PLACEHOLDER\n",
      "the old copy is left alone rather than invented",
    );
  } finally {
    rmSync(base, { force: true, recursive: true });
  }
});

test("a release without a spec still yields a complete pin", async () => {
  const { placeholderFields } = await import("./gateway-pin.mjs");
  const { base, root } = sandbox();
  try {
    const result = bumpGateway({
      root,
      tag: TAG,
      gh: fakeGh({ manifest: distManifest({ specs: false }), specs: {} }),
      log: () => {},
    });
    assert.deepEqual(placeholderFields(result.pin), []);
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

test("a spec that disagrees with the checksum list fails before any write", () => {
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
      /openapi\.yaml for gateway-v0\.4\.0 has sha256 [0-9a-f]{64}, but sha256\.sum records [0-9a-f]{64}/,
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
    assert.ok(result.diffs.some((diff) => diff.includes('+  "tag": "gateway-v0.4.0"')), result.diffs.join("\n"));
  } finally {
    rmSync(base, { force: true, recursive: true });
  }
});

test("a missing sha256.sum is named rather than silently skipped", () => {
  const manifest = distManifest();
  const view = releaseView({ manifest });
  const withoutSum = JSON.stringify({
    ...JSON.parse(view),
    assets: JSON.parse(view)["assets"].filter((asset) => asset["name"] !== CHECKSUM_ASSET),
  });
  assert.throws(
    () => downloadRelease({ tag: TAG, gh: fakeGh({ view: withoutSum }) }),
    /publishes no sha256\.sum asset/,
  );
});

test("a missing dist-manifest.json is named rather than silently skipped", () => {
  const manifest = distManifest();
  const view = releaseView({ manifest });
  const withoutManifest = JSON.stringify({
    ...JSON.parse(view),
    assets: JSON.parse(view)["assets"].filter((asset) => asset["name"] !== DIST_MANIFEST_ASSET),
  });
  assert.throws(
    () => downloadRelease({ tag: TAG, gh: fakeGh({ view: withoutManifest }) }),
    /publishes no dist-manifest\.json asset/,
  );
});

test("a target missing from the manifest is refused by pin field", () => {
  const manifest = distManifest();
  delete manifest.artifacts["gateway-aarch64-unknown-linux-gnu.tar.xz"];
  assert.throws(
    () => downloadRelease({ tag: TAG, gh: fakeGh({ manifest }) }),
    /targets\.aarch64-unknown-linux-gnu\.asset is absent from dist-manifest\.json/,
  );
});

test("a checksum that disagrees between the two sources is refused, naming both", () => {
  const manifest = distManifest();
  manifest.checksumSum[0] = `${"0".repeat(64)} *gateway-x86_64-pc-windows-msvc.zip`;
  assert.throws(
    () => downloadRelease({ tag: TAG, gh: fakeGh({ manifest }) }),
    /targets\.x86_64-pc-windows-msvc\.sha256 is [0-9a-f]{64} in dist-manifest\.json but [0-9a-f]{64} in sha256\.sum/,
  );
});

test("a release digest that disagrees with the manifest is refused", () => {
  const manifest = distManifest();
  const view = JSON.parse(releaseView({ manifest }));
  const windows = view["assets"].find((asset) => asset["name"] === assetFor("x86_64-pc-windows-msvc"));
  windows["digest"] = `sha256:${"1".repeat(64)}`;
  assert.throws(
    () => downloadRelease({ tag: TAG, gh: fakeGh({ manifest, view: JSON.stringify(view) }) }),
    /but the release states [0-9a-f]{64}/,
  );
});

test("a release that states no digest still pins from the two files", () => {
  const result = downloadRelease({ tag: TAG, gh: fakeGh({ omitDigest: true }) });
  assert.equal(result.pin["targets"]["aarch64-apple-darwin"]["sha256"], digestFor("aarch64-apple-darwin"));
});

test("a target with no published size is refused rather than pinned as zero", () => {
  const manifest = distManifest();
  const sizes = sizesFor();
  sizes.set(assetFor("x86_64-apple-darwin"), null);
  assert.throws(
    () => downloadRelease({ tag: TAG, gh: fakeGh({ manifest, sizes }) }),
    /targets\.x86_64-apple-darwin\.size is absent from the release's asset list/,
  );
});

test("a manifest whose version disagrees with the tag is refused", () => {
  const manifest = distManifest();
  manifest.releases[0].app_version = "0.5.0";
  assert.throws(
    () => releaseVersion(manifest, TAG),
    /records app_version 0\.5\.0, so the tag does not name the release it points at/,
  );
});

test("the version comes from the tag, so min_gateway is always derivable", () => {
  assert.equal(versionFromTag("gateway-v0.3.0"), "0.3.0");
  assert.equal(versionFromTag("gateway-v1.2.3-rc.1"), "1.2.3-rc.1");
  assert.throws(() => versionFromTag("gateway"), /does not end in a version/);
  assert.equal(releaseVersion({}, TAG), "0.4.0", "a manifest with no version still pins the tag's");
});

test("a manifest naming two executables is refused", () => {
  const manifest = distManifest();
  manifest.artifacts[assetFor("x86_64-unknown-linux-gnu")].assets[1].name = "other-gateway";
  assert.throws(() => executableName(manifest), /names runvil-gateway and other-gateway executables/);
});

test("a manifest naming no executable is refused", () => {
  const manifest = distManifest();
  for (const artifact of Object.values(manifest.artifacts)) {
    if (artifact.kind === "executable-zip") {
      artifact.assets = artifact.assets.filter((asset) => asset.kind !== "executable");
    }
  }
  assert.throws(() => executableName(manifest), /names no executables for the released archives/);
});

test("a checksum line that does not parse names its line number", () => {
  assert.throws(
    () => parseChecksumSum(`${"a".repeat(64)} *ok.tar.xz\nnot a checksum\n`),
    /sha256\.sum line 2 does not read as "<sha256> <asset>"/,
  );
  assert.throws(() => parseChecksumSum("\n \n"), /sha256\.sum lists no asset checksums/);
});

test("the checksum list accepts the two-space form and both cases of hex", () => {
  const sums = parseChecksumSum(`${"A".repeat(64)}  some.tar.xz\n`);
  assert.equal(sums.get("some.tar.xz"), "a".repeat(64));
});

test("an ambiguous triple is refused rather than pinned by guess", () => {
  const manifest = distManifest();
  const artifact = manifest.artifacts[assetFor("x86_64-unknown-linux-gnu")];
  manifest.artifacts["gateway-x86_64-unknown-linux-gnu.tar.gz"] = { ...artifact };
  manifest.checksumSum.push(`${digestFor("x86_64-unknown-linux-gnu")} *gateway-x86_64-unknown-linux-gnu.tar.gz`);
  assert.throws(
    () =>
      selectTargets({
        manifest,
        checksums: parseChecksumSum(checksumSum(manifest)),
        sizes: sizesFor(),
      }),
    /targets\.x86_64-unknown-linux-gnu\.asset is ambiguous/,
  );
});

test("the built pin carries every five target entries in release order", () => {
  const { pin } = downloadRelease({ tag: TAG, gh: fakeGh() });
  assert.deepEqual(Object.keys(pin["targets"]), TARGETS);
  assert.equal(pin["pin_format"], 1);
  assert.equal(pin["executable"], "runvil-gateway");
  assert.equal(pin["spec"]["asyncapi"]["path"], "gateway-pin/asyncapi.yaml");
});

test("buildPin leaves out a spec the release did not publish", () => {
  const { pin } = downloadRelease({
    tag: TAG,
    gh: fakeGh({ manifest: distManifest({ specs: false }), specs: {} }),
  });
  assert.deepEqual(pin["spec"], {});
  assert.deepEqual(Object.keys(pin["targets"]), TARGETS);
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
      return releaseView({ manifest: distManifest() });
    }
    throw new Error("gh release download failed: HTTP 404");
  };
  assert.throws(() => downloadRelease({ tag: TAG, gh }), /HTTP 404/);
  assert.equal(existsSync(join(tmpdir(), "does-not-exist")), false);
});

test("every target entry names the archive its own triple builds", () => {
  const { pin } = downloadRelease({ tag: TAG, gh: fakeGh() });
  for (const target of TARGETS) {
    assert.equal(pin["targets"][target]["asset"], assetFor(target));
    assert.equal(pin["targets"][target]["sha256"], digestFor(target));
  }
});

test("buildPin keeps the repository it was given", () => {
  const pin = buildPin({
    tag: TAG,
    repository: REPOSITORY,
    version: "0.4.0",
    executable: "runvil-gateway",
    targets: Object.fromEntries(
      TARGETS.map((target) => [target, { asset: assetFor(target), sha256: digestFor(target), size: 1 }]),
    ),
    specs: {},
  });
  assert.equal(pin["repository"], REPOSITORY);
  assert.equal(pin["min_gateway"], "0.4.0");
});
