// Moves the Gateway pin to a published release.
//
// Given a release tag, this script asks `gh` for the release, downloads its
// `sidecar.json` manifest and the two spec files, checks the specs against
// the manifest's recorded hashes, and only then rewrites `gateway-pin/pin.json`
// and the committed spec copies. Nothing is written until every download and
// check has succeeded, so a wrong tag or a missing asset leaves the pin
// exactly as it was.
//
// The manifest is the `sidecar.json` the Gateway repository attaches to each
// release. It must record, for each of the five release targets, an asset
// name, its sha256, and its size, plus the version, `min_gateway`, the
// executable's name, and the two spec hashes. A release without one cannot
// be pinned.
//
// Usage:
//   node tools/bump-gateway.mjs <tag> [--repo <owner/name>] [--dry-run]
//
// `--dry-run` performs every download and check, prints the diff, and writes
// nothing.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  DEFAULT_RELEASE_REPOSITORY,
  PLACEHOLDER,
  TARGETS,
  pinPath,
  renderPin,
  specPath,
} from "./gateway-pin.mjs";

const REPOSITORY_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** The manifest every pinnable Gateway release attaches. */
export const MANIFEST_ASSET = "sidecar.json";

/** The spec files every pinnable Gateway release attaches, and their pin keys. */
export const SPEC_ASSETS = [
  { name: "openapi.yaml", key: "openapi" },
  { name: "asyncapi.yaml", key: "asyncapi" },
];

/** Lowercase hex sha256 of `bytes`. */
export function sha256Hex(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

/** Runs `gh` and returns its stdout, failing with the message `gh` printed. */
function runGh(args) {
  const result = spawnSync("gh", args, { encoding: "utf8" });
  if (result.error !== undefined && result.error !== null) {
    throw new Error(`could not run gh ${args.join(" ")}: ${result.error.message}`);
  }
  if (result.status !== 0) {
    const detail = (result.stderr || result.stdout || "").trim();
    throw new Error(`gh ${args.join(" ")} failed${detail === "" ? "" : `: ${detail}`}`);
  }
  return result.stdout;
}

/**
 * Refuses a manifest that cannot produce a usable pin, naming every field
 * that is missing or mistyped. A short manifest is a bug in the Gateway's
 * release, not something to paper over here.
 */
export function validateManifest(manifest, tag) {
  const problems = [];
  const named = (value) => typeof value === "string" && value.length > 0;
  if (!named(manifest["version"])) {
    problems.push("version");
  } else if (!tag.endsWith(manifest["version"])) {
    problems.push(`version ${manifest["version"]} does not match tag ${tag}`);
  }
  for (const field of ["min_gateway", "executable"]) {
    if (!named(manifest[field])) {
      problems.push(field);
    }
  }
  for (const { key } of SPEC_ASSETS) {
    if (!named(manifest["spec"]?.[key]?.["sha256"])) {
      problems.push(`spec.${key}.sha256`);
    }
  }
  for (const target of TARGETS) {
    const entry = manifest["targets"]?.[target];
    if (!named(entry?.["asset"])) {
      problems.push(`targets.${target}.asset`);
    }
    if (!named(entry?.["sha256"])) {
      problems.push(`targets.${target}.sha256`);
    }
    if (!Number.isInteger(entry?.["size"]) || entry["size"] <= 0) {
      problems.push(`targets.${target}.size`);
    }
  }
  if (problems.length > 0) {
    throw new Error(`${MANIFEST_ASSET} for ${tag} cannot be pinned: ${problems.join(", ")}`);
  }
  return manifest;
}

/** Builds the pin object for `tag` from a validated manifest. */
export function buildPin(manifest, tag, repository) {
  const targets = {};
  for (const target of TARGETS) {
    const entry = manifest["targets"][target];
    targets[target] = {
      asset: entry["asset"],
      sha256: entry["sha256"],
      size: entry["size"],
    };
  }
  const spec = {};
  for (const { key } of SPEC_ASSETS) {
    spec[key] = {
      path: `gateway-pin/${SPEC_ASSETS.find((asset) => asset.key === key).name}`,
      sha256: manifest["spec"][key]["sha256"],
    };
  }
  return {
    pin_format: 1,
    repository,
    tag,
    version: manifest["version"],
    executable: manifest["executable"],
    min_gateway: manifest["min_gateway"],
    spec,
    targets,
  };
}

/**
 * A readable diff of `before` and `after`: two lines of context around the
 * lines that differ, marked `-` and `+` and prefixed with the file. Text
 * that did not change reports as unchanged.
 */
export function renderDiff(name, before, after) {
  if (before === after) {
    return `${name}: unchanged`;
  }
  const oldLines = before.split("\n");
  const newLines = after.split("\n");
  let prefix = 0;
  while (
    prefix < oldLines.length &&
    prefix < newLines.length &&
    oldLines[prefix] === newLines[prefix]
  ) {
    prefix += 1;
  }
  let suffix = 0;
  while (
    suffix < oldLines.length - prefix &&
    suffix < newLines.length - prefix &&
    oldLines[oldLines.length - 1 - suffix] === newLines[newLines.length - 1 - suffix]
  ) {
    suffix += 1;
  }
  const lines = [`--- ${name}`, `+++ ${name}`];
  for (const line of oldLines.slice(Math.max(0, prefix - 2), prefix)) {
    lines.push(` ${line}`);
  }
  for (const line of oldLines.slice(prefix, oldLines.length - suffix)) {
    lines.push(`-${line}`);
  }
  for (const line of newLines.slice(prefix, newLines.length - suffix)) {
    lines.push(`+${line}`);
  }
  const tail = oldLines.length - suffix;
  for (const line of oldLines.slice(tail, tail + 2)) {
    lines.push(` ${line}`);
  }
  return lines.join("\n");
}

/** Writes `text` to `path` in one step: a sibling temp file, then a rename. */
function writeAtomic(path, text) {
  const temporary = `${path}.bump-tmp`;
  writeFileSync(temporary, text);
  renameSync(temporary, path);
}

function readIfPresent(path) {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return "";
  }
}

/**
 * Downloads a release's manifest and specs and returns the new pin text and
 * the spec texts, without touching the repository. `gh` is a seam for the
 * tests.
 */
export function downloadRelease({ tag, repository = DEFAULT_RELEASE_REPOSITORY, gh = runGh }) {
  const scratch = mkdtempSync(join(tmpdir(), "runvil-bump-gateway-"));
  try {
    try {
      gh(["release", "view", tag, "--repo", repository, "--json", "tagName,assets"]);
    } catch (error) {
      throw new Error(
        `Gateway release ${tag} was not found in ${repository}: ${error.message}\n` +
          "nothing was written; list the tags with " +
          `\`gh release list --repo ${repository}\``,
      );
    }
    const names = [MANIFEST_ASSET, ...SPEC_ASSETS.map((spec) => spec.name)];
    const patterns = names.flatMap((name) => ["--pattern", name]);
    gh(["release", "download", tag, "--repo", repository, "--dir", scratch, ...patterns]);
    let manifest;
    try {
      manifest = JSON.parse(readFileSync(join(scratch, MANIFEST_ASSET), "utf8"));
    } catch (error) {
      throw new Error(`${MANIFEST_ASSET} for ${tag} is not valid JSON: ${error.message}`);
    }
    validateManifest(manifest, tag);
    const specs = {};
    for (const { name, key } of SPEC_ASSETS) {
      const bytes = readFileSync(join(scratch, name));
      const actual = sha256Hex(bytes);
      const recorded = manifest["spec"][key]["sha256"];
      if (actual !== recorded) {
        throw new Error(
          `${name} for ${tag} has sha256 ${actual}, but ${MANIFEST_ASSET} records ${recorded}`,
        );
      }
      specs[key] = { name, text: bytes.toString("utf8") };
    }
    const pin = buildPin(manifest, tag, repository);
    if (Object.values(pin["targets"]).some((entry) => entry["sha256"] === PLACEHOLDER)) {
      throw new Error(`${MANIFEST_ASSET} for ${tag} still names a placeholder checksum`);
    }
    return { pin, pinText: renderPin(pin), specs };
  } finally {
    rmSync(scratch, { force: true, recursive: true });
  }
}

/**
 * Runs the bump: downloads, checks, prints the diff, and writes the spec
 * copies and then the pin. `dryRun` stops before the writes.
 */
export function bumpGateway({
  root = REPOSITORY_ROOT,
  tag,
  repository = DEFAULT_RELEASE_REPOSITORY,
  gh = runGh,
  dryRun = false,
}) {
  if (typeof tag !== "string" || tag.length === 0) {
    throw new Error("bump-gateway needs a release tag, for example `gateway-v0.4.0`");
  }
  const { pin, pinText, specs } = downloadRelease({ tag, repository, gh });
  const diffs = [
    renderDiff(pinPath(root), readIfPresent(pinPath(root)), pinText),
    ...SPEC_ASSETS.map(({ name, key }) =>
      renderDiff(specPath(root, name), readIfPresent(specPath(root, name)), specs[key].text),
    ),
  ];
  if (!dryRun) {
    // The spec copies land first: the pin is the authority that says they
    // are current, so it never names a spec that is not yet written.
    for (const { name, key } of SPEC_ASSETS) {
      writeAtomic(specPath(root, name), specs[key].text);
    }
    writeAtomic(pinPath(root), pinText);
  }
  return { pin, diffs, written: !dryRun };
}

function parseArguments(args) {
  let tag;
  let repository = DEFAULT_RELEASE_REPOSITORY;
  let dryRun = false;
  let repoSeen = false;
  for (let index = 0; index < args.length; index += 1) {
    const name = args[index];
    if (name === "--dry-run") {
      if (dryRun) {
        throw new Error("duplicate bump argument: --dry-run");
      }
      dryRun = true;
      continue;
    }
    if (name === "--repo") {
      const value = args[index + 1];
      if (value === undefined) {
        throw new Error("missing value for bump argument: --repo");
      }
      if (repoSeen) {
        throw new Error("duplicate bump argument: --repo");
      }
      repository = value;
      repoSeen = true;
      index += 1;
      continue;
    }
    if (name.startsWith("-")) {
      throw new Error(`unsupported bump argument: ${name}`);
    }
    if (tag !== undefined) {
      throw new Error(`unexpected second tag: ${name}`);
    }
    tag = name;
  }
  if (tag === undefined) {
    throw new Error("usage: bump-gateway.mjs <tag> [--repo <owner/name>] [--dry-run]");
  }
  return { tag, repository, dryRun };
}

function main(args) {
  const parsed = parseArguments(args);
  const result = bumpGateway(parsed);
  for (const diff of result.diffs) {
    console.log(diff);
  }
  console.log(
    result.written
      ? `pinned Gateway ${result.pin.tag} (${result.pin.version})`
      : `dry run: Gateway ${result.pin.tag} would be pinned; nothing was written`,
  );
}

if (
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href
) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
