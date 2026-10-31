import assert from "node:assert/strict";
import { gzipSync } from "node:zlib";
import test from "node:test";

import {
  ARCHIVE_FORMATS,
  extractExecutable,
  extractTarGz,
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
  assert.deepEqual(ARCHIVE_FORMATS, [".tar.gz", ".tgz", ".zip"]);
});

test("a non-zip buffer is refused rather than read as garbage", () => {
  assert.throws(() => extractZip(Buffer.alloc(64, 7), "runvil-gateway.exe"), /not a zip archive/);
});
