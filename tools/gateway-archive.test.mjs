import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { gzipSync } from "node:zlib";

import {
  ARCHIVE_FORMATS,
  extractExecutable,
  extractTarGz,
  extractTarXz,
  extractZip,
} from "./gateway-archive.mjs";

const BLOCK = 512;

/** One tar header block for a regular file of `size` bytes. */
function tarHeader(name, size) {
  const header = Buffer.alloc(BLOCK, 0);
  header.write(name, 0, 100, "utf8");
  header.write("0000644\0", 100, "utf8");
  header.write("0000000\0", 108, "utf8");
  header.write("0000000\0", 116, "utf8");
  header.write(`${size.toString(8).padStart(11, "0")}\0`, 124, "utf8");
  header.write("00000000000\0", 136, "utf8");
  header.write("        ", 148, "utf8");
  header.write("0", 156, "utf8");
  header.write("ustar\0", 257, "utf8");
  header.write("00", 263, "utf8");
  return header;
}

/** A gzipped tar archive holding `files`, each `{ name, content }`. */
function tarGz(files) {
  const blocks = [];
  for (const file of files) {
    const body = Buffer.from(file.content, "utf8");
    blocks.push(tarHeader(file.name, body.length));
    const padded = Buffer.alloc(Math.ceil(body.length / BLOCK) * BLOCK, 0);
    body.copy(padded);
    blocks.push(padded);
  }
  blocks.push(Buffer.alloc(BLOCK * 2, 0));
  return gzipSync(Buffer.concat(blocks));
}

/** A stored (uncompressed) zip archive holding `files`. */
function storedZip(files) {
  const locals = [];
  const central = [];
  let offset = 0;
  for (const file of files) {
    const body = Buffer.from(file.content, "utf8");
    const name = Buffer.from(file.name, "utf8");
    const local = Buffer.alloc(30, 0);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt32LE(0, 14);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(body.length, 22);
    local.writeUInt16LE(name.length, 26);
    locals.push(local, name, body);
    const entry = Buffer.alloc(46, 0);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(20, 4);
    entry.writeUInt16LE(20, 6);
    entry.writeUInt32LE(0, 16);
    entry.writeUInt32LE(body.length, 20);
    entry.writeUInt32LE(body.length, 24);
    entry.writeUInt16LE(name.length, 28);
    entry.writeUInt32LE(offset, 42);
    central.push(entry, name);
    offset += 30 + name.length + body.length;
  }
  const directory = Buffer.concat(central);
  const localBytes = Buffer.concat(locals);
  const end = Buffer.alloc(22, 0);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(files.length, 8);
  end.writeUInt16LE(files.length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(localBytes.length, 16);
  return Buffer.concat([localBytes, directory, end]);
}

/**
 * A temporary directory, removed when `body` returns. The xz fixtures are
 * built by the system `tar`, because Node cannot write xz and the reader
 * under test is `tar` itself.
 */
function withScratch(body) {
  const base = mkdtempSync(join(tmpdir(), "runvil-archive-test-"));
  try {
    return body(base);
  } finally {
    rmSync(base, { force: true, recursive: true });
  }
}

/** Runs `tar` in `cwd`, failing the test with its stderr when it fails. */
function tar(cwd, args) {
  const result = spawnSync("tar", args, { cwd, encoding: "utf8" });
  assert.equal(
    result.status,
    0,
    `tar ${args.join(" ")} failed: ${result.error?.message ?? result.stderr}`,
  );
  return result.stdout;
}

/** An xz-compressed tar of `names` under `base`, each file holding its base name. */
function tarXz(base, names) {
  for (const name of names) {
    const path = join(base, ...name.split("/"));
    mkdirSync(join(path, ".."), { recursive: true });
    writeFileSync(path, name.split("/").at(-1));
  }
  tar(base, ["-cJf", "fixture.tar.xz", "--", ...names]);
  return readFileSync(join(base, "fixture.tar.xz"));
}

test("reads the executable out of a gzipped tar by its base name", () => {
  const bytes = tarGz([
    { name: "runvil-gateway-x86_64-unknown-linux-gnu/README.md", content: "read me" },
    { name: "runvil-gateway-x86_64-unknown-linux-gnu/runvil-gateway", content: "ELF" },
  ]);
  assert.equal(extractTarGz(bytes, "runvil-gateway").toString("utf8"), "ELF");
});

test("reads the executable out of a zip by its base name", () => {
  const bytes = storedZip([
    { name: "runvil-gateway-x86_64-pc-windows-msvc/LICENSE", content: "license" },
    { name: "runvil-gateway-x86_64-pc-windows-msvc/runvil-gateway.exe", content: "MZ" },
  ]);
  assert.equal(extractZip(bytes, "runvil-gateway.exe").toString("utf8"), "MZ");
});

test("an archive without the executable reports nothing rather than a guess", () => {
  const bytes = tarGz([{ name: "pkg/README.md", content: "read me" }]);
  assert.equal(extractTarGz(bytes, "runvil-gateway"), null);
  assert.equal(extractZip(storedZip([{ name: "a.txt", content: "x" }]), "runvil-gateway"), null);
});

test("the extension picks the reader", () => {
  const tar = tarGz([{ name: "b/runvil-gateway", content: "bin" }]);
  assert.equal(extractExecutable("x.tar.gz", tar, "runvil-gateway").toString(), "bin");
  assert.equal(extractExecutable("x.tgz", tar, "runvil-gateway").toString(), "bin");
  const zip = storedZip([{ name: "b/runvil-gateway.exe", content: "bin" }]);
  assert.equal(extractExecutable("x.zip", zip, "runvil-gateway.exe").toString(), "bin");
});

test("an unsupported archive extension is refused by name", () => {
  assert.throws(() => extractExecutable("x.7z", Buffer.alloc(0), "runvil-gateway"), /unsupported Gateway archive x\.7z/);
  assert.deepEqual(ARCHIVE_FORMATS, [".tar.gz", ".tgz", ".tar.xz", ".zip"]);
});

test("reads the executable out of an xz tar and ignores a decoy", () => {
  withScratch((base) => {
    // A directory whose own name matches the wanted file, at a different
    // path: a listing ends it with a slash, and a matcher that trimmed the
    // slash would pick it and then fail to read it as a file.
    mkdirSync(join(base, "gateway-extras", "runvil-gateway"), { recursive: true });
    const bytes = tarXz(base, [
      "gateway-aarch64-apple-darwin/README.md",
      "gateway-extras/runvil-gateway/decoy.txt",
      "gateway-aarch64-apple-darwin/runvil-gateway.bak",
      "gateway-aarch64-apple-darwin/runvil-gateway",
    ]);
    const extracted = extractTarXz("gateway-aarch64-apple-darwin.tar.xz", bytes, "runvil-gateway");
    assert.equal(extracted.toString("utf8"), "runvil-gateway");
    assert.equal(
      extractExecutable("gateway-aarch64-apple-darwin.tar.xz", bytes, "runvil-gateway").toString("utf8"),
      "runvil-gateway",
    );
  });
});

test("an xz tar without the wanted member throws and names both", () => {
  withScratch((base) => {
    const bytes = tarXz(base, ["gateway-x86_64-unknown-linux-gnu/README.md"]);
    assert.throws(
      () => extractTarXz("gateway-x86_64-unknown-linux-gnu.tar.xz", bytes, "runvil-gateway"),
      (error) => {
        assert.match(error.message, /gateway-x86_64-unknown-linux-gnu\.tar\.xz/);
        assert.match(error.message, /runvil-gateway/);
        return true;
      },
    );
  });
});

test("a corrupt xz tar throws instead of returning empty bytes", () => {
  withScratch((base) => {
    const bytes = tarXz(base, ["pkg/runvil-gateway"]);
    const truncated = bytes.subarray(0, bytes.length - 64);
    assert.throws(
      () => extractTarXz("gateway.tar.xz", truncated, "runvil-gateway"),
      /cannot read gateway\.tar\.xz: tar exited with status \d+/,
    );
    assert.throws(
      () => extractTarXz("gateway.tar.xz", Buffer.from("not an xz archive at all"), "runvil-gateway"),
      /cannot read gateway\.tar\.xz: tar exited with status \d+/,
    );
  });
});

test("a non-zip buffer is refused rather than read as garbage", () => {
  assert.throws(() => extractZip(Buffer.alloc(64, 7), "runvil-gateway.exe"), /not a zip archive/);
});
