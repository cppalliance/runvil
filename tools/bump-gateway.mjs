// Moves the Gateway pin to a published release.
//
// Given a release tag, this script asks `gh` for the release and reads the
// two files cargo-dist publishes: `sha256.sum`, its list of asset checksums,
// and `dist-manifest.json`, its build record. The two must agree on every
// target's archive name and sha256 before anything is written, and the
// release's own asset list must state the size. The tag alone supplies the
// version, so `min_gateway` is always derivable.
//
// The spec files are optional. The Gateway repository publishes none, so a
// release that has them gets them copied into `gateway-pin/` and recorded by
// sha256, and a release without them records no spec entry at all. An
// absent entry says the release publishes no spec, which is a different
// thing from a hash nobody has filled in, so absence is written by leaving
// the entry out rather than by writing a placeholder.
//
// Nothing is written until every download and check has succeeded, so a
// wrong tag or a mismatched checksum leaves the pin exactly as it was.
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

/** The checksum asset: one line of sha256 and asset name per released file. */
export const CHECKSUM_ASSET = "sha256.sum";

/** The build record cargo-dist publishes beside the archives. */
export const DIST_MANIFEST_ASSET = "dist-manifest.json";

/**
 * The spec files a release may attach, and the pin keys they fill. A
 * release that publishes none of them is still pinnable.
 */
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

/** Whether `value` reads as a lowercase or uppercase sha256 in hex. */
function isSha256(value) {
  return typeof value === "string" && /^[0-9a-fA-F]{64}$/.test(value);
}

/**
 * Reads `sha256.sum` into a map of asset name to lowercase sha256. A line
 * that does not read as a checksum names its number rather than being
 * skipped, because a skipped line is an asset that would pin with no check.
 */
export function parseChecksumSum(text, source = CHECKSUM_ASSET) {
  const checksums = new Map();
  const lines = text.split("\n");
  for (const [index, raw] of lines.entries()) {
    const line = raw.trim();
    if (line === "") {
      continue;
    }
    const match = /^([0-9a-fA-F]{64}) [ *](.+)$/.exec(line);
    if (match === null) {
      throw new Error(
        `${source} line ${index + 1} does not read as "<sha256> <asset>": ${line}`,
      );
    }
    const asset = match[2].trim();
    if (asset === "") {
      throw new Error(`${source} line ${index + 1} names no asset: ${line}`);
    }
    checksums.set(asset, match[1].toLowerCase());
  }
  if (checksums.size === 0) {
    throw new Error(`${source} lists no asset checksums`);
  }
  return checksums;
}

/** The version a release tag carries: `gateway-v0.3.0` becomes `0.3.0`. */
export function versionFromTag(tag) {
  const match = /(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)$/.exec(tag);
  if (match === null) {
    throw new Error(`the tag ${tag} does not end in a version like 0.3.0`);
  }
  return match[1];
}

/**
 * The release's version, taken from the tag and checked against the version
 * `dist-manifest.json` records. The tag is the authority: a person names it
 * on the command line, so a manifest that disagrees is a broken release
 * rather than a reason to pin a version nobody asked for.
 */
export function releaseVersion(manifest, tag) {
  const version = versionFromTag(tag);
  const stated = manifest?.["releases"]?.[0]?.["app_version"];
  if (typeof stated === "string" && stated !== "" && stated !== version) {
    throw new Error(
      `${DIST_MANIFEST_ASSET} for ${tag} records app_version ${stated}, so the tag does not name the release it points at`,
    );
  }
  return version;
}

/**
 * The executable's name inside the archives, which `dist-manifest.json`
 * records once per archive. Every archive must name the same executable, so
 * a mixed release fails here rather than after the first fetch.
 */
export function executableName(manifest) {
  const names = new Set();
  for (const artifact of Object.values(manifest?.["artifacts"] ?? {})) {
    if (artifact?.["kind"] !== "executable-zip") {
      continue;
    }
    for (const asset of artifact["assets"] ?? []) {
      if (asset?.["kind"] === "executable" && typeof asset["name"] === "string") {
        names.add(asset["name"]);
      }
    }
  }
  if (names.size !== 1) {
    const described = names.size === 0 ? "no" : [...names].join(" and ");
    throw new Error(
      `${DIST_MANIFEST_ASSET} names ${described} executable` +
        `${names.size === 1 ? "" : "s"} for the released archives; expected exactly one`,
    );
  }
  return [...names][0];
}

/**
 * One pin target entry per released triple: the archive's name, its sha256,
 * and its size. The name and hash come from `dist-manifest.json` and must
 * also appear in `sha256.sum` with the same hash, and the release's own
 * asset digest must agree when GitHub states one. `sizes` and `digests` come
 * from the release's asset list, because neither checksum file records a
 * size.
 *
 * Every disagreement is collected and reported together, by pin field, so a
 * release that is wrong in several places says so in one run.
 */
export function selectTargets({ manifest, checksums, sizes, digests = new Map() }) {
  const artifacts = manifest?.["artifacts"];
  if (artifacts === null || typeof artifacts !== "object" || Array.isArray(artifacts)) {
    throw new Error(`${DIST_MANIFEST_ASSET} holds no artifacts object`);
  }
  const problems = [];
  const targets = {};
  for (const target of TARGETS) {
    const matches = Object.entries(artifacts).filter(
      ([, artifact]) =>
        artifact?.["kind"] === "executable-zip" &&
        (artifact?.["target_triples"] ?? []).includes(target),
    );
    if (matches.length === 0) {
      problems.push(`targets.${target}.asset is absent from ${DIST_MANIFEST_ASSET}`);
      continue;
    }
    if (matches.length > 1) {
      const named = matches.map(([name]) => name).join(", ");
      problems.push(`targets.${target}.asset is ambiguous in ${DIST_MANIFEST_ASSET}: ${named}`);
      continue;
    }
    const [asset, artifact] = matches[0];
    const recorded = artifact?.["checksums"]?.["sha256"];
    if (!isSha256(recorded)) {
      problems.push(`targets.${target}.sha256 is absent from ${DIST_MANIFEST_ASSET} for ${asset}`);
      continue;
    }
    const listed = checksums.get(asset);
    if (listed === undefined) {
      problems.push(`targets.${target}.asset ${asset} is absent from ${CHECKSUM_ASSET}`);
      continue;
    }
    if (listed !== recorded.toLowerCase()) {
      problems.push(
        `targets.${target}.sha256 is ${recorded.toLowerCase()} in ${DIST_MANIFEST_ASSET} ` +
          `but ${listed} in ${CHECKSUM_ASSET}`,
      );
      continue;
    }
    const digest = digests.get(asset);
    if (digest !== undefined && digest.toLowerCase() !== recorded.toLowerCase()) {
      problems.push(
        `targets.${target}.sha256 is ${recorded.toLowerCase()} in ${DIST_MANIFEST_ASSET} ` +
          `but the release states ${digest.toLowerCase()}`,
      );
      continue;
    }
    const size = sizes.get(asset);
    if (!Number.isInteger(size) || size <= 0) {
      problems.push(`targets.${target}.size is absent from the release's asset list for ${asset}`);
      continue;
    }
    targets[target] = { asset, sha256: recorded.toLowerCase(), size };
  }
  if (problems.length > 0) {
    throw new Error(`the release cannot be pinned: ${problems.join(", ")}`);
  }
  return targets;
}

/**
 * Builds the pin object. `min_gateway` is the release's own version: this
 * release has every operation Workshop calls, so it is the lowest Gateway
 * that does.
 *
 * `specs` holds the spec files the release published, keyed by pin key. A
 * key that is missing leaves that entry out of `spec`, so a consumer reads
 * absence rather than a hash that means nothing.
 */
export function buildPin({ tag, repository, version, executable, targets, specs }) {
  const spec = {};
  for (const { key } of SPEC_ASSETS) {
    const found = specs[key];
    if (found === undefined) {
      continue;
    }
    spec[key] = { path: `gateway-pin/${found.name}`, sha256: found.sha256 };
  }
  return {
    pin_format: 1,
    repository,
    tag,
    version,
    executable,
    min_gateway: version,
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
 * Reads the release's asset list, returning each asset's size and the
 * sha256 GitHub states for it. `gh` prints the digest as `sha256:<hex>`, and
 * the field is absent on an older release, so a missing digest is reported
 * as no digest rather than as a mismatch.
 */
export function readReleaseAssets(release) {
  const sizes = new Map();
  const digests = new Map();
  for (const asset of release?.["assets"] ?? []) {
    const name = asset?.["name"];
    if (typeof name !== "string" || name.length === 0) {
      continue;
    }
    if (Number.isInteger(asset["size"])) {
      sizes.set(name, asset["size"]);
    }
    const digest = asset["digest"];
    if (isSha256(digest)) {
      digests.set(name, digest);
    } else if (typeof digest === "string" && digest.startsWith("sha256:")) {
      const hex = digest.slice("sha256:".length);
      if (isSha256(hex)) {
        digests.set(name, hex);
      }
    }
  }
  return { sizes, digests };
}

/** Names of the release's assets, in the order `gh` listed them. */
export function releaseAssetNames(release) {
  return (release?.["assets"] ?? [])
    .map((asset) => asset?.["name"])
    .filter((name) => typeof name === "string" && name.length > 0);
}

/**
 * Downloads a release's checksum list and build record, plus any spec files
 * it publishes, and returns the new pin text and the spec texts, without
 * touching the repository. `gh` and `download` are seams for the tests.
 */
export function downloadRelease({
  tag,
  repository = DEFAULT_RELEASE_REPOSITORY,
  gh = runGh,
  download,
}) {
  const fetch = download ?? gh;

  const scratch = mkdtempSync(join(tmpdir(), "runvil-bump-gateway-"));
  try {
    let release;
    try {
      const text = gh([
        "release",
        "view",
        tag,
        "--repo",
        repository,
        "--json",
        "tagName,name,assets",
      ]);
      release = typeof text === "string" && text.trim() !== "" ? JSON.parse(text) : {};
    } catch (error) {
      throw new Error(
        `Gateway release ${tag} was not found in ${repository}: ${error.message}\n` +
          "nothing was written; list the tags with " +
          `\`gh release list --repo ${repository}\``,
      );
    }
    const published = releaseAssetNames(release);
    if (!published.includes(CHECKSUM_ASSET)) {
      throw new Error(
        `Gateway release ${tag} in ${repository} publishes no ${CHECKSUM_ASSET} asset, ` +
          "so the archive checksums cannot be checked; nothing was written",
      );
    }
    if (!published.includes(DIST_MANIFEST_ASSET)) {
      throw new Error(
        `Gateway release ${tag} in ${repository} publishes no ${DIST_MANIFEST_ASSET} asset, ` +
          "so the archive names cannot be cross-checked; nothing was written",
      );
    }

    const wanted = [CHECKSUM_ASSET, DIST_MANIFEST_ASSET];
    const specsToFetch = SPEC_ASSETS.filter((spec) => published.includes(spec.name));
    wanted.push(...specsToFetch.map((spec) => spec.name));
    const patterns = wanted.flatMap((name) => ["--pattern", name]);
    fetch(["release", "download", tag, "--repo", repository, "--dir", scratch, ...patterns]);

    const checksums = parseChecksumSum(readFileSync(join(scratch, CHECKSUM_ASSET), "utf8"));
    let manifest;
    try {
      manifest = JSON.parse(readFileSync(join(scratch, DIST_MANIFEST_ASSET), "utf8"));
    } catch (error) {
      throw new Error(`${DIST_MANIFEST_ASSET} for ${tag} is not valid JSON: ${error.message}`);
    }
    const version = releaseVersion(manifest, tag);
    const executable = executableName(manifest);
    const { sizes, digests } = readReleaseAssets(release);
    const targets = selectTargets({ manifest, checksums, sizes, digests });

    const specs = {};
    for (const spec of SPEC_ASSETS) {
      if (!specsToFetch.includes(spec)) {
        continue;
      }
      const bytes = readFileSync(join(scratch, spec.name));
      const sha256 = sha256Hex(bytes);
      const listed = checksums.get(spec.name);
      if (listed !== undefined && listed !== sha256) {
        throw new Error(
          `${spec.name} for ${tag} has sha256 ${sha256}, but ${CHECKSUM_ASSET} records ${listed}`,
        );
      }
      specs[spec.key] = { name: spec.name, sha256, text: bytes.toString("utf8") };
    }

    const pin = buildPin({ tag, repository, version, executable, targets, specs });
    const placeholders = placeholderValues(pin);
    if (placeholders.length > 0) {
      throw new Error(
        `the pin for ${tag} still holds a placeholder in ${placeholders.join(", ")}; ` +
          "a release must state every field",
      );
    }
    return { pin, pinText: renderPin(pin), specs, published };
  } finally {
    rmSync(scratch, { force: true, recursive: true });
  }
}

/** Every path in `pin` whose value is `PLACEHOLDER`, as dotted names. */
function placeholderValues(pin) {
  const found = [];
  const walk = (value, path) => {
    if (value === PLACEHOLDER) {
      found.push(path);
      return;
    }
    if (value !== null && typeof value === "object") {
      for (const [key, child] of Object.entries(value)) {
        walk(child, path === "" ? key : `${path}.${key}`);
      }
    }
  };
  walk(pin, "");
  return found;
}

/**
 * Runs the bump: downloads, checks, prints the diff, and writes the spec
 * copies and then the pin. `dryRun` stops before the writes.
 *
 * A spec copy the release no longer publishes is left in place rather than
 * deleted: the pin stops naming it, and a person decides whether the file
 * stays as a reference.
 */
export function bumpGateway({
  root = REPOSITORY_ROOT,
  tag,
  repository = DEFAULT_RELEASE_REPOSITORY,
  gh = runGh,
  download,
  dryRun = false,
  log = console.log,
}) {
  if (typeof tag !== "string" || tag.length === 0) {
    throw new Error("bump-gateway needs a release tag, for example `gateway-v0.4.0`");
  }
  const { pin, pinText, specs } = downloadRelease({
    tag,
    repository,
    gh,
    download: download ?? gh,
  });
  const written = Object.values(specs).map(({ name, text }) => ({
    path: specPath(root, name),
    text,
  }));
  const diffs = [
    renderDiff(pinPath(root), readIfPresent(pinPath(root)), pinText),
    ...written.map((file) => renderDiff(file.path, readIfPresent(file.path), file.text)),
  ];
  const absent = SPEC_ASSETS.filter(({ key }) => specs[key] === undefined).map(({ name }) => name);
  if (absent.length > 0) {
    log(
      `notice: ${tag} publishes no ${absent.join(" and ")}, so the pin records no spec because ` +
        "the release publishes none; no file was invented",
    );
  }
  if (!dryRun) {
    // The spec copies land first: the pin is the authority that says they
    // are current, so it never names a spec that is not yet written.
    for (const file of written) {
      writeAtomic(file.path, file.text);
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
