// Obtains the Gateway sidecar Workshop bundles: either the pinned release
// binary for one target, or a stub for compile-only jobs.
//
// The pin in `gateway-pin/pin.json` names the release and the sha256 of
// every asset. This script downloads (or reuses a cached copy of) one
// asset, checks its size and sha256 against the pin, extracts the
// executable, and hands it to `stage-gateway-sidecar.mjs`, which owns the
// Tauri naming rule and the destination. A mismatch fails loudly with both
// hashes, because a silently wrong Gateway is worse than no build.
//
// Usage:
//   node tools/fetch-gateway-sidecar.mjs [--target <triple>] [--cache-dir <path>]
//                                        [--verify-attestation] [--stub]
//
// `--stub` writes a placeholder file at the path Tauri expects and
// downloads nothing, so a job that only compiles the app needs no release.
// `--verify-attestation` additionally asks `gh release verify-asset` about
// the downloaded archive.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { extractExecutable } from "./gateway-archive.mjs";
import {
  PLACEHOLDER,
  TARGETS,
  placeholderFields,
  placeholderReport,
  readPin,
  releaseAssetUrl,
  requireFilledPin,
  targetEntry,
} from "./gateway-pin.mjs";
import {
  gatewayBinaryName,
  markGatewaySidecarPinned,
  stageGatewaySidecar,
} from "./stage-gateway-sidecar.mjs";

const REPOSITORY_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** The stub's bytes: a recognizable text, never runnable as a Gateway. */
const STUB_CONTENT =
  "This is a placeholder staged by tools/fetch-gateway-sidecar.mjs --stub.\n" +
  "It stands in for the Gateway sidecar so compile-only jobs need no release.\n" +
  "It is not an executable. Stage the real pinned binary before shipping.\n";

/** The host triple a build on this machine targets. */
export function hostTarget() {
  const key = `${process.platform}-${process.arch}`;
  const triples = {
    "win32-x64": "x86_64-pc-windows-msvc",
    "win32-arm64": "aarch64-pc-windows-msvc",
    "darwin-arm64": "aarch64-apple-darwin",
    "darwin-x64": "x86_64-apple-darwin",
    "linux-x64": "x86_64-unknown-linux-gnu",
    "linux-arm64": "aarch64-unknown-linux-gnu",
  };
  const triple = triples[key];
  if (triple === undefined) {
    throw new Error(`no Gateway target matches this machine (${key}); pass --target <triple>`);
  }
  return triple;
}

/** The default download cache: `RUNVIL_GATEWAY_CACHE`, else the user's cache directory. */
export function defaultCacheDir() {
  const fromEnvironment = process.env["RUNVIL_GATEWAY_CACHE"];
  if (fromEnvironment !== undefined && fromEnvironment !== "") {
    return fromEnvironment;
  }
  const base =
    process.platform === "win32"
      ? process.env["LOCALAPPDATA"] ?? join(homedir(), "AppData", "Local")
      : process.env["XDG_CACHE_HOME"] ?? join(homedir(), ".cache");
  return join(base, "runvil", "gateway-sidecar");
}

/** Lowercase hex sha256 of `bytes`. */
export function sha256Hex(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

/**
 * Throws unless `bytes` match the pin's recorded size and sha256, naming
 * both hashes and both sizes.
 */
export function verifyAsset(bytes, entry, asset) {
  if (bytes.length !== entry.size) {
    throw new Error(
      `Gateway asset ${asset} is ${bytes.length} bytes, but the pin records ${entry.size} bytes`,
    );
  }
  const actual = sha256Hex(bytes);
  if (actual !== entry.sha256) {
    throw new Error(
      `Gateway asset ${asset} sha256 is ${actual}, but the pin records ${entry.sha256}`,
    );
  }
  return actual;
}

/** Downloads `url` and returns its bytes, failing on any non-200 answer. */
async function downloadBytes(url) {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) {
    throw new Error(`downloading ${url} failed: ${response.status} ${response.statusText}`);
  }
  return Buffer.from(await response.arrayBuffer());
}

/** How a `gh` subprocess is run: a seam the tests replace. */
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
 * Ensures the pinned archive for `target` is in the cache and verified,
 * returning its path. A cache hit is re-verified on every run: the pin is
 * the authority, not the cache.
 */
async function ensureArchive(pin, target, cacheDir, { download }) {
  const entry = targetEntry(pin, target);
  const url = releaseAssetUrl(pin, entry.asset);
  const cached = join(cacheDir, pin.tag, entry.asset);
  if (existsSync(cached)) {
    const bytes = readFileSync(cached);
    try {
      verifyAsset(bytes, entry, entry.asset);
      return { path: cached, downloaded: false, url };
    } catch {
      // A cached file that no longer matches the pin is stale: refetch it
      // rather than failing, so a re-pinned cache heals itself.
      rmSync(cached, { force: true });
    }
  }
  const bytes = await download(url);
  verifyAsset(bytes, entry, entry.asset);
  mkdirSync(dirname(cached), { recursive: true });
  writeFileSync(cached, bytes);
  return { path: cached, downloaded: true, url };
}

/**
 * Obtains and stages the sidecar. Returns what was staged and how.
 *
 * `download` and `gh` are seams for the tests; production uses `fetch` and
 * the real `gh` executable.
 */
export async function fetchGatewaySidecar(options = {}) {
  const {
    root = REPOSITORY_ROOT,
    target = hostTarget(),
    cacheDir = defaultCacheDir(),
    stub = false,
    verifyAttestation = false,
    download = downloadBytes,
    gh = runGh,
  } = options;
  const binary = gatewayBinaryName(target);
  if (!TARGETS.includes(target)) {
    throw new Error(
      `unsupported Gateway target ${target}; the pin records ${TARGETS.join(", ")}`,
    );
  }
  if (stub) {
    return stageStub({ root, target, cacheDir, binary });
  }
  const pin = readPin(root);
  if (pin.executable === PLACEHOLDER) {
    throw new Error("gateway-pin/pin.json still holds a placeholder in executable");
  }
  try {
    requireFilledPin(pin, { source: "gateway-pin/pin.json", target });
  } catch (error) {
    // A person running this before the pin is filled needs the whole list,
    // not just the fields one target reads, so the report follows the
    // refusal.
    throw new Error(`${error.message}\n${placeholderReport(pin, { source: "gateway-pin/pin.json" })}`);
  }
  const entry = targetEntry(pin, target);
  const archive = await ensureArchive(pin, target, cacheDir, { download });
  if (verifyAttestation) {
    gh(["release", "verify-asset", pin.tag, archive.path, "--repo", pin.repository]);
  }
  const extracted = extractExecutable(entry.asset, readFileSync(archive.path), binary);
  if (extracted === null) {
    throw new Error(`Gateway asset ${entry.asset} holds no ${binary}`);
  }
  const scratch = mkdtempSync(join(cacheDir, "extract-"));
  try {
    const source = join(scratch, binary);
    writeFileSync(source, extracted);
    const staged = stageGatewaySidecar({ root, target, source });
    markGatewaySidecarPinned({ root, target, kind: "release", tag: pin.tag });
    return {
      staged,
      target,
      tag: pin.tag,
      version: pin.version,
      asset: entry.asset,
      url: archive.url,
      downloaded: archive.downloaded,
      stub: false,
    };
  } finally {
    rmSync(scratch, { force: true, recursive: true });
  }
}

/** Writes the placeholder file and stages it, downloading nothing. */
function stageStub({ root, target, cacheDir, binary }) {
  const directory = join(cacheDir, "stub");
  mkdirSync(directory, { recursive: true });
  const source = join(directory, binary);
  writeFileSync(source, STUB_CONTENT);
  const staged = stageGatewaySidecar({ root, target, source });
  markGatewaySidecarPinned({ root, target, kind: "stub" });
  return {
    staged,
    target,
    tag: null,
    version: null,
    asset: null,
    url: null,
    downloaded: false,
    stub: true,
  };
}

function parseArguments(args) {
  const values = new Map();
  const flagNames = new Set(["--stub", "--verify-attestation"]);
  const valueNames = new Set(["--target", "--cache-dir"]);
  const flags = new Set();
  for (let index = 0; index < args.length; index += 1) {
    const name = args[index];
    if (flagNames.has(name)) {
      if (flags.has(name) || values.has(name)) {
        throw new Error(`duplicate fetch argument: ${name}`);
      }
      flags.add(name);
      continue;
    }
    if (!valueNames.has(name)) {
      throw new Error(
        "usage: fetch-gateway-sidecar.mjs [--target <triple>] [--cache-dir <path>] " +
          "[--verify-attestation] [--stub]",
      );
    }
    const value = args[index + 1];
    if (value === undefined) {
      throw new Error(`missing value for fetch argument: ${name}`);
    }
    if (values.has(name)) {
      throw new Error(`duplicate fetch argument: ${name}`);
    }
    values.set(name, value);
    index += 1;
  }
  return {
    target: values.get("--target"),
    cacheDir: values.get("--cache-dir"),
    stub: flags.has("--stub"),
    verifyAttestation: flags.has("--verify-attestation"),
  };
}

async function main(args) {
  const parsed = parseArguments(args);
  const options = { stub: parsed.stub, verifyAttestation: parsed.verifyAttestation };
  if (parsed.target !== undefined) {
    options.target = parsed.target;
  }
  if (parsed.cacheDir !== undefined) {
    options.cacheDir = parsed.cacheDir;
  }
  const result = await fetchGatewaySidecar(options);
  if (result.stub) {
    console.log(`staged a stub for ${result.target} at ${result.staged}`);
    console.log("this file is not a runnable Gateway; never ship a build made with --stub");
    return;
  }
  console.log(
    `${result.downloaded ? "downloaded" : "reused"} ${result.asset} (${result.url}), ` +
      `sha256 verified against the pin for ${result.tag}`,
  );
  console.log(`staged ${result.target} from Gateway ${result.version} at ${result.staged}`);
}

if (
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href
) {
  try {
    await main(process.argv.slice(2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
