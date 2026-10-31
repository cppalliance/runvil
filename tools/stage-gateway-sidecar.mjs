import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPOSITORY_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function targetNames(target) {
  if (!/^[A-Za-z0-9_]+(?:-[A-Za-z0-9_]+){2,}$/.test(target)) {
    throw new Error(`invalid target triple: ${target}`);
  }
  const extension = target.split("-").includes("windows") ? ".exe" : "";
  return {
    binary: `runvil-gateway${extension}`,
    sidecar: `runvil-gateway-${target}${extension}`,
  };
}

export function gatewayBinaryName(target) {
  return targetNames(target).binary;
}

export function gatewaySidecarName(target) {
  return targetNames(target).sidecar;
}

/**
 * The file tauri.conf.json's `externalBin` entry resolves for `target`.
 * This is the one owner of that naming rule: the fetch script, the build
 * pipeline, and CI all ask for the path rather than spelling it again.
 */
export function gatewaySidecarPath({ root = REPOSITORY_ROOT, target }) {
  return join(
    root,
    "crates",
    "workshop",
    "desktop",
    "binaries",
    gatewaySidecarName(target),
  );
}

export function stageGatewaySidecar({ root = REPOSITORY_ROOT, target, source }) {
  const expectedSourceName = gatewayBinaryName(target);
  const sourcePath = resolve(source);
  if (!existsSync(sourcePath)) {
    throw new Error(`Gateway source binary does not exist: ${sourcePath}`);
  }
  if (!lstatSync(sourcePath).isFile()) {
    throw new Error(`Gateway source binary is not a file: ${sourcePath}`);
  }
  if (basename(sourcePath) !== expectedSourceName) {
    throw new Error(
      `Gateway source binary must be named ${expectedSourceName} for ${target}`,
    );
  }

  const destination = gatewaySidecarPath({ root, target });
  mkdirSync(dirname(destination), { recursive: true });
  copyFileSync(sourcePath, destination);
  return destination;
}

/**
 * Marks the staged sidecar as authoritative: the desktop build script then
 * leaves it alone instead of refreshing it from a workspace-built Gateway.
 * Without this, a stale `target/<profile>/runvil-gateway` silently replaces
 * a downloaded and checksummed release binary.
 *
 * `kind` records which of the two it was, `release` or `stub`, so a
 * developer running `cargo workshop` can be told when the sidecar it reuses
 * is only a placeholder.
 */
export function markGatewaySidecarPinned({ root = REPOSITORY_ROOT, target, kind = "release", tag = null }) {
  const marker = gatewaySidecarPinMarkerPath({ root, target });
  mkdirSync(dirname(marker), { recursive: true });
  writeFileSync(
    marker,
    `${JSON.stringify({ kind, tag }, null, 2)}\n`,
  );
  return marker;
}

/**
 * The marker's record, or `null` when the sidecar was not staged by the
 * fetch script. A malformed marker still counts as a marker: the desktop
 * build script only needs to know the file is not its to refresh, and an
 * unknown kind is reported as unknown rather than assumed to be a release.
 */
export function readGatewaySidecarPin({ root = REPOSITORY_ROOT, target }) {
  const marker = gatewaySidecarPinMarkerPath({ root, target });
  let text;
  try {
    text = readFileSync(marker, "utf8");
  } catch {
    return null;
  }
  try {
    const parsed = JSON.parse(text);
    return { kind: typeof parsed["kind"] === "string" ? parsed["kind"] : "unknown", tag: parsed["tag"] ?? null };
  } catch {
    return { kind: "unknown", tag: null };
  }
}

/**
 * The marker beside the staged sidecar saying it is authoritative. Named
 * so the sidecar's own name stays exactly what Tauri expects; the marker
 * sits next to it and is never bundled.
 */
export function gatewaySidecarPinMarkerPath({ root = REPOSITORY_ROOT, target }) {
  return `${gatewaySidecarPath({ root, target })}.pinned`;
}

export function removeGatewaySidecar({ root = REPOSITORY_ROOT, target }) {
  const destination = gatewaySidecarPath({ root, target });
  rmSync(destination, { force: true });
  rmSync(gatewaySidecarPinMarkerPath({ root, target }), { force: true });
  return destination;
}

function parseArguments(args) {
  const [action, ...options] = args;
  if (action !== "stage" && action !== "remove" && action !== "path") {
    throw new Error("usage: stage-gateway-sidecar.mjs <stage|remove|path> --target <triple> [--source <path>]");
  }

  const values = new Map();
  for (let index = 0; index < options.length; index += 2) {
    const name = options[index];
    const value = options[index + 1];
    if ((name !== "--target" && name !== "--source") || value === undefined) {
      throw new Error(`invalid sidecar argument: ${name ?? "<missing>"}`);
    }
    if (values.has(name)) {
      throw new Error(`duplicate sidecar argument: ${name}`);
    }
    values.set(name, value);
  }

  const target = values.get("--target");
  if (target === undefined) {
    throw new Error("missing required sidecar argument: --target");
  }
  const source = values.get("--source");
  if (action === "stage" && source === undefined) {
    throw new Error("missing required sidecar argument: --source");
  }
  if (action !== "stage" && source !== undefined) {
    throw new Error(`${action} does not accept --source`);
  }
  return { action, source, target };
}

function main(args) {
  const { action, source, target } = parseArguments(args);
  if (action === "path") {
    console.log(gatewaySidecarPath({ target }));
    return;
  }
  const path =
    action === "stage"
      ? stageGatewaySidecar({ source, target })
      : removeGatewaySidecar({ target });
  console.log(`${action === "stage" ? "staged" : "removed"} ${path}`);
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
