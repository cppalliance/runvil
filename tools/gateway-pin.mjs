// The Gateway pin: the one file that records the release Workshop obtains
// its sidecar from, plus the checksums that prove the bytes.
//
// Two scripts read it. `fetch-gateway-sidecar.mjs` downloads one target's
// asset and verifies it against the recorded sha256; `bump-gateway.mjs`
// rewrites it from a published release. Both refuse a placeholder: a pin
// that names nothing must fail loudly, because a silent empty download
// would ship a Workshop without its Gateway.

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPOSITORY_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** The directory holding the pin and the committed spec copies. */
export const PIN_DIRECTORY = "gateway-pin";

/** The pin's file name inside {@link PIN_DIRECTORY}. */
export const PIN_FILE_NAME = "pin.json";

/** The value that marks a field nobody has filled in yet. */
export const PLACEHOLDER = "PLACEHOLDER";

/** Every target triple a Gateway release must publish. */
export const TARGETS = [
  "x86_64-pc-windows-msvc",
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
];

/** The repository that publishes Gateway releases. */
export const DEFAULT_RELEASE_REPOSITORY = "cppalliance/runvil-gateway";

/** The absolute path of the pin file for the repository at `root`. */
export function pinPath(root = REPOSITORY_ROOT) {
  return join(root, PIN_DIRECTORY, PIN_FILE_NAME);
}

/** The absolute path of one committed spec copy for the repository at `root`. */
export function specPath(root = REPOSITORY_ROOT, name) {
  return join(root, PIN_DIRECTORY, name);
}

function requireObject(value, field, source) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${source}: ${field} must be a JSON object`);
  }
  return value;
}

function requireString(value, field, source) {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${source}: ${field} must be a non-empty string`);
  }
  return value;
}

function requireNumber(value, field, source) {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 0) {
    throw new Error(`${source}: ${field} must be a non-negative integer`);
  }
  return value;
}

/**
 * Parses and shape-checks pin text. Shape checks only: a placeholder
 * value passes here and is refused later by the consumer that would have
 * used it, so a fresh pin stays readable while nobody can fetch from it.
 */
export function parsePin(text, source = PIN_FILE_NAME) {
  let pin;
  try {
    pin = JSON.parse(text);
  } catch (error) {
    throw new Error(`${source}: not valid JSON: ${error.message}`);
  }
  requireObject(pin, "the pin", source);
  if (pin["pin_format"] !== 1) {
    throw new Error(`${source}: pin_format must be 1, got ${JSON.stringify(pin["pin_format"])}`);
  }
  requireString(pin["repository"], "repository", source);
  requireString(pin["tag"], "tag", source);
  requireString(pin["version"], "version", source);
  requireString(pin["executable"], "executable", source);
  requireString(pin["min_gateway"], "min_gateway", source);
  const spec = requireObject(pin["spec"], "spec", source);
  for (const name of ["openapi", "asyncapi"]) {
    const entry = requireObject(spec[name], `spec.${name}`, source);
    requireString(entry["path"], `spec.${name}.path`, source);
    requireString(entry["sha256"], `spec.${name}.sha256`, source);
  }
  const targets = requireObject(pin["targets"], "targets", source);
  for (const target of TARGETS) {
    const entry = requireObject(targets[target], `targets.${target}`, source);
    requireString(entry["asset"], `targets.${target}.asset`, source);
    requireString(entry["sha256"], `targets.${target}.sha256`, source);
    requireNumber(entry["size"], `targets.${target}.size`, source);
  }
  for (const target of Object.keys(targets)) {
    if (!TARGETS.includes(target)) {
      throw new Error(`${source}: targets.${target} is not one of the five release targets`);
    }
  }
  return pin;
}

/** Reads and shape-checks the pin file of the repository at `root`. */
export function readPin(root = REPOSITORY_ROOT) {
  const path = pinPath(root);
  let text;
  try {
    text = readFileSync(path, "utf8");
  } catch (error) {
    throw new Error(`cannot read the Gateway pin at ${path}: ${error.message}`);
  }
  return parsePin(text, path);
}

/** Renders a pin canonically: two-space indent and a trailing newline. */
export function renderPin(pin) {
  return `${JSON.stringify(pin, null, 2)}\n`;
}

/** One target's entry, refusing a triple the pin does not record. */
export function targetEntry(pin, target) {
  const entry = pin["targets"]?.[target];
  if (entry === undefined) {
    throw new Error(
      `the Gateway pin records no target ${target}; recorded: ${Object.keys(pin["targets"] ?? {}).join(", ")}`,
    );
  }
  return entry;
}

/**
 * Every field of `pin` that is still a placeholder, as dotted paths. An
 * empty list means the pin names a release a consumer may use.
 */
export function placeholderFields(pin) {
  const found = [];
  for (const field of ["tag", "version", "min_gateway"]) {
    if (pin[field] === PLACEHOLDER) {
      found.push(field);
    }
  }
  for (const name of ["openapi", "asyncapi"]) {
    if (pin["spec"]?.[name]?.["sha256"] === PLACEHOLDER) {
      found.push(`spec.${name}.sha256`);
    }
  }
  for (const [target, entry] of Object.entries(pin["targets"] ?? {})) {
    if (entry.asset === PLACEHOLDER) {
      found.push(`targets.${target}.asset`);
    }
    if (entry.sha256 === PLACEHOLDER) {
      found.push(`targets.${target}.sha256`);
    }
    if (entry.size === 0) {
      found.push(`targets.${target}.size`);
    }
  }
  return found;
}

/**
 * Refuses a pin a consumer cannot use, naming the file and every field
 * still to fill in. `target` narrows the refusal to the fields that
 * target's download needs, so a half-filled pin fails only where it must.
 */
export function requireFilledPin(pin, { source = PIN_FILE_NAME, target } = {}) {
  let missing = placeholderFields(pin);
  if (target !== undefined) {
    missing = missing.filter(
      (field) => !field.startsWith("targets.") || field.startsWith(`targets.${target}.`),
    );
  }
  if (missing.length > 0) {
    throw new Error(
      `${source} still holds placeholders (${missing.join(", ")}); ` +
        "run `node tools/bump-gateway.mjs <tag>` against a published Gateway release",
    );
  }
}

/**
 * A reader-facing report of every field still to fill in, or a sentence
 * saying the pin is complete. `tools/fetch-gateway-sidecar.mjs` prints this
 * when a person runs it before the pin is filled, so the failure names the
 * work rather than only the symptom.
 */
export function placeholderReport(pin, { source = PIN_FILE_NAME } = {}) {
  const fields = placeholderFields(pin);
  if (fields.length === 0) {
    return "the Gateway pin is complete";
  }
  return [
    `${source} still has ${fields.length} unfilled field(s):`,
    ...fields.map((field) => `  ${field}`),
  ].join("\n");
}

/** The release download URL of one asset. */
export function releaseAssetUrl(pin, asset) {
  return `https://github.com/${pin["repository"]}/releases/download/${pin["tag"]}/${asset}`;
}
