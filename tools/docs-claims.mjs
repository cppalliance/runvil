// Guard: the repository's prose follows the root AGENTS.md. Every
// rulebook uses Engine, Harness, Host, and Plugin as the Definitions
// section defines them, and the root AGENTS.md never describes Pack
// activation. Reads the files with node:fs rather than importing anything,
// so the check needs Node and no install.
// Run: node --test tools/docs-claims.mjs
import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** Every line of `file` (relative to the repo root) matching `phrase`, a string or a RegExp, tagged with its number. */
async function offendingLines(file, phrase) {
  const text = await readFile(path.join(repoRoot, file), "utf8");
  return text
    .split("\n")
    .map((line, index) => ({ line, number: index + 1 }))
    .filter(({ line }) => (phrase instanceof RegExp ? phrase.test(line) : line.includes(phrase)))
    .map(({ line, number }) => `${file}:${number}: ${line.trim()}`);
}

// Guard for the Engine, Harness, Host, and Plugin definitions in the root AGENTS.md.
// Coding sessions read the rulebooks (every AGENTS.md, every .cursor/rules
// file, and the crate doc of every lib.rs or main.rs that carries
// `## Invariants`) as instructions, so each one uses the four words one way.
// Inline and fenced code is skipped, and so is the Definitions section itself,
// whose rule text names the words it restricts.
//
// The three checkouts CI places inside this one are skipped: each holds
// the rulebooks of its own repository, which its own scan reads.
const SKIP_DIRS = new Set([".git", "target", "runvil-workshop", "runvil-docs", "runvil-harness"]);

async function* walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    if (!entry.isDirectory()) {
      yield path.join(dir, entry.name);
    } else if (!SKIP_DIRS.has(entry.name)) {
      yield* walk(path.join(dir, entry.name));
    }
  }
}

/** Every prose line of every rulebook, with inline code blanked, as `{ file, number, text }`. */
async function rulebookLines() {
  const out = [];
  for await (const full of walk(repoRoot)) {
    const file = path.relative(repoRoot, full).split(path.sep).join("/");
    const base = path.basename(file);
    let lines;
    if (base === "AGENTS.md" || (file.startsWith(".cursor/rules/") && base.endsWith(".mdc"))) {
      lines = (await readFile(full, "utf8")).split("\n").map((text, index) => ({ text, number: index + 1 }));
    } else if (base === "lib.rs" || base === "main.rs") {
      const doc = (await readFile(full, "utf8"))
        .split("\n")
        .map((text, index) => ({ text, number: index + 1 }))
        .filter(({ text }) => text.trimStart().startsWith("//!"));
      if (!doc.some(({ text }) => text.includes("## Invariants"))) {
        continue;
      }
      lines = doc.map(({ text, number }) => ({ text: text.trimStart().slice(3), number }));
    } else {
      continue;
    }
    let fenced = false;
    let definitions = false;
    for (const { text, number } of lines) {
      if (text.trimStart().startsWith("```")) {
        fenced = !fenced;
        continue;
      }
      if (file === "AGENTS.md" && text.startsWith("## ")) {
        definitions = text.startsWith("## Definitions");
      }
      if (!fenced && !definitions) {
        out.push({ file, number, text: text.replace(/`[^`]*`/g, " ") });
      }
    }
  }
  return out;
}

test("rulebooks use Engine, Harness, Host, and Plugin as the root AGENTS.md defines them", async () => {
  const offenders = [];
  for (const { file, number, text } of await rulebookLines()) {
    const where = `${file}:${number}: ${text.trim()}`;
    if (/\bhost(s|ed|ing)?\b|\bhost-/.test(text)) {
      offenders.push(`${where} (lowercase "host": required "Host" for the application, another word for any other sense)`);
    }
    if (/(?<![-_/:\w])(engine|harness)(?![-_/:\w])/.test(text)) {
      offenders.push(`${where} (lowercase "engine" or "harness": required "Engine" or "Harness" for the defined term)`);
    }
    if (/(?<![-_/:\w])plugins?(?![-_/:\w])/.test(text)) {
      offenders.push(`${where} (lowercase "plugin": required "Plugin" for the defined term)`);
    }
  }
  assert.deepEqual(offenders, [], "rulebook lines break the root AGENTS.md Definitions");
});

// Packs are installed once, when the Host sets up, and each run makes its
// own instance of each Pack it holds and takes a snapshot of them, so docs
// say a Pack is made for each run, never activated.
test("the root AGENTS.md never describes Pack activation", async () => {
  const offenders = await offendingLines("AGENTS.md", /activat/i);
  assert.deepEqual(offenders, [], "a doc still says Packs are activated; say installed, snapshotted, or declared");
});
