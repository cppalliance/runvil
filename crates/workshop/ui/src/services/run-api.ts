// Validated HTTP boundary for the prompt-contract API. POST
// /prompts/contract turns prompt markdown into the Run window's
// contract DTO; every response arrives as unknown and is parsed field
// by field into the narrow types below before any consumer touches it,
// no casts. Failures throw typed CatalogError variants
// (services/error-catalog.ts); a 422 parse failure surfaces the
// server's parse_<kind> code and line-numbered message in the thrown
// error's message. The fetch and JSON mechanics are the shared floor
// in json-request.ts.

import { CatalogError, ErrorCatalog } from "./error-catalog";
import { errorCode, errorMessage, isRecord, readJson, request } from "./json-request";

/** A declared input or output file. */
export interface RunContractFile {
  readonly path: string;
  readonly description: string;
}

/** A declared Pack: its one-segment name, such as `web`. */
export interface RunContractPack {
  readonly id: string;
}

/**
 * The input field types the wire format allows: a schema property's scalar
 * type, or `json` for any other property schema.
 */
export type RunContractArgType = "string" | "boolean" | "integer" | "number" | "json";

/** One input field, derived from one input schema property. */
export interface RunContractArg {
  readonly name: string;
  readonly type: RunContractArgType;
  readonly optional: boolean;
  /** The property's default, any JSON value; null when absent. */
  readonly default: unknown;
  /** The human-readable description; null when absent. */
  readonly description: string | null;
}

/** The input fields; implicit names the single prose field of a prompt without an input schema. */
export interface RunContractArgs {
  readonly implicit: boolean;
  readonly fields: readonly RunContractArg[];
}

/** One declared model role. */
export interface RunContractModel {
  readonly label: string;
  readonly keywords: readonly string[];
  /** The minimum context window in tokens; null when absent. */
  readonly minContext: number | null;
  readonly description: string | null;
}

/** The Run-window contract: everything the panel renders. */
export interface RunContract {
  readonly name: string;
  readonly description: string;
  /** The Engine major the `runvil:` key declares; null when absent. */
  readonly runvil: number | null;
  /** The declared tool-loop cap; null for the runtime default. */
  readonly maxToolIterations: number | null;
  readonly input: RunContractFile | null;
  readonly output: RunContractFile | null;
  readonly uses: readonly RunContractPack[];
  readonly args: RunContractArgs;
  readonly models: readonly RunContractModel[];
}

function parseFile(value: unknown): RunContractFile | null {
  if (!isRecord(value)) {
    return null;
  }
  const { path, description } = value;
  if (typeof path !== "string" || typeof description !== "string") {
    return null;
  }
  return { path, description };
}

function parsePack(value: unknown): RunContractPack | null {
  if (!isRecord(value)) {
    return null;
  }
  const { id } = value;
  if (typeof id !== "string") {
    return null;
  }
  return { id };
}

const ARG_TYPES: readonly string[] = ["string", "boolean", "integer", "number", "json"];

function parseArg(value: unknown): RunContractArg | null {
  if (!isRecord(value)) {
    return null;
  }
  const { name, type, optional } = value;
  if (typeof name !== "string" || typeof type !== "string" || !ARG_TYPES.includes(type)) {
    return null;
  }
  if (typeof optional !== "boolean") {
    return null;
  }
  const defaultValue: unknown = value.default ?? null;
  const description = value.description;
  if (description !== null && typeof description !== "string") {
    return null;
  }
  return {
    name,
    type: type as RunContractArgType,
    optional,
    default: defaultValue,
    description: description ?? null,
  };
}

function parseArgs(value: unknown): RunContractArgs | null {
  if (!isRecord(value)) {
    return null;
  }
  const { implicit, fields } = value;
  if (typeof implicit !== "boolean" || !Array.isArray(fields)) {
    return null;
  }
  const parsed: RunContractArg[] = [];
  for (const field of fields) {
    const arg = parseArg(field);
    if (arg === null) {
      return null;
    }
    parsed.push(arg);
  }
  return { implicit, fields: parsed };
}

function parseModel(value: unknown): RunContractModel | null {
  if (!isRecord(value)) {
    return null;
  }
  const { label, keywords, min_context } = value;
  if (typeof label !== "string") {
    return null;
  }
  if (!Array.isArray(keywords) || !keywords.every((keyword) => typeof keyword === "string")) {
    return null;
  }
  if (min_context !== null && typeof min_context !== "number") {
    return null;
  }
  const description = value.description;
  if (description !== null && typeof description !== "string") {
    return null;
  }
  return {
    label,
    keywords,
    minContext: min_context ?? null,
    description: description ?? null,
  };
}

function parseContract(body: unknown): RunContract | null {
  if (!isRecord(body)) {
    return null;
  }
  const { name, description, runvil, max_tool_iterations, uses, models } = body;
  if (typeof name !== "string" || typeof description !== "string") {
    return null;
  }
  if (runvil !== null && typeof runvil !== "number") {
    return null;
  }
  if (max_tool_iterations !== null && typeof max_tool_iterations !== "number") {
    return null;
  }
  const input = body.input === null ? null : parseFile(body.input);
  const output = body.output === null ? null : parseFile(body.output);
  if (body.input !== null && input === null) {
    return null;
  }
  if (body.output !== null && output === null) {
    return null;
  }
  if (!Array.isArray(uses) || !Array.isArray(models)) {
    return null;
  }
  const parsedPacks: RunContractPack[] = [];
  for (const pack of uses) {
    const parsed = parsePack(pack);
    if (parsed === null) {
      return null;
    }
    parsedPacks.push(parsed);
  }
  const args = parseArgs(body.args);
  if (args === null) {
    return null;
  }
  const parsedModels: RunContractModel[] = [];
  for (const model of models) {
    const parsed = parseModel(model);
    if (parsed === null) {
      return null;
    }
    parsedModels.push(parsed);
  }
  return {
    name,
    description,
    runvil: runvil ?? null,
    maxToolIterations: max_tool_iterations ?? null,
    input,
    output,
    uses: parsedPacks,
    args,
    models: parsedModels,
  };
}

/**
 * Throws the typed failure for one non-OK response. A 422 parse failure
 * reports the server's parse_<kind> code ahead of its line-numbered
 * message, so the panel's error row shows both.
 */
function httpFailure(body: unknown, status: number, route: string): never {
  const message = errorMessage(body, status, route);
  const code = errorCode(body);
  throw new CatalogError(ErrorCatalog.HttpStatus, code === null ? message : `[${code}] ${message}`, {
    status,
  });
}

/**
 * Parses prompt markdown into the Run window's contract. `name` is the
 * prompt's display name (the file's, when it came from one). Throws
 * typed CatalogError variants on transport, HTTP, and shape failures.
 */
export async function fetchPromptContract(name: string, text: string): Promise<RunContract> {
  const route = "/prompts/contract";
  const response = await request(route, `POST ${route}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name, text }),
  });
  const body = await readJson(response, `POST ${route}`);
  if (!response.ok) {
    httpFailure(body, response.status, `POST ${route}`);
  }
  const contract = parseContract(body);
  if (contract === null) {
    throw new CatalogError(
      ErrorCatalog.UnexpectedShape,
      `POST ${route} returned an unexpected shape`,
    );
  }
  return contract;
}
