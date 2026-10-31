// Reading one executable out of a release archive.
//
// The Gateway's releases are built by cargo-dist, which publishes a zip for
// Windows and a gzipped tar elsewhere. Node ships no tar or zip reader, and
// adding a dependency to run one build step is not worth it, so this module
// reads exactly the two shapes cargo-dist writes and refuses everything
// else by name.

import { gunzipSync, inflateRawSync } from "node:zlib";
import { basename } from "node:path";

const TAR_BLOCK = 512;
const ZIP_LOCAL_HEADER = 0x04034b50;
const ZIP_CENTRAL_HEADER = 0x02014b50;
const ZIP_END_OF_CENTRAL_DIRECTORY = 0x06054b50;
const ZIP_STORED = 0;
const ZIP_DEFLATED = 8;
const ZIP64_SENTINEL = 0xffffffff;

/** The archive formats a Gateway release asset may use, by extension. */
export const ARCHIVE_FORMATS = [".tar.gz", ".tgz", ".zip"];

/**
 * Extracts the file named `wanted` (matched on its base name) from `bytes`,
 * choosing the reader by the asset's extension. Returns `null` when the
 * archive holds no such file, so the caller names what it expected.
 */
export function extractExecutable(assetName, bytes, wanted) {
  const lower = assetName.toLowerCase();
  if (lower.endsWith(".zip")) {
    return extractZip(bytes, wanted);
  }
  if (lower.endsWith(".tar.gz") || lower.endsWith(".tgz")) {
    return extractTarGz(bytes, wanted);
  }
  throw new Error(
    `unsupported Gateway archive ${assetName}; expected one of ${ARCHIVE_FORMATS.join(", ")}`,
  );
}

/** Reads a NUL-terminated string from `buffer` at `offset`, at most `length` bytes. */
function readString(buffer, offset, length) {
  const slice = buffer.subarray(offset, offset + length);
  const end = slice.indexOf(0);
  return slice.toString("utf8", 0, end === -1 ? slice.length : end);
}

/** Extracts the file named `wanted` from a gzipped tar archive. */
export function extractTarGz(bytes, wanted) {
  const tar = gunzipSync(bytes);
  let offset = 0;
  while (offset + TAR_BLOCK <= tar.length) {
    const header = tar.subarray(offset, offset + TAR_BLOCK);
    if (header.every((byte) => byte === 0)) {
      break;
    }
    const name = readString(header, 0, 100);
    const sizeText = readString(header, 124, 12).trim();
    const size = sizeText === "" ? 0 : Number.parseInt(sizeText, 8);
    if (!Number.isFinite(size) || size < 0) {
      throw new Error(`malformed tar header for ${name || "<unnamed>"}`);
    }
    const type = header[156] === 0 ? "0" : String.fromCharCode(header[156]);
    const prefix = readString(header, 345, 155);
    const full = prefix === "" ? name : `${prefix}/${name}`;
    offset += TAR_BLOCK;
    const data = tar.subarray(offset, offset + size);
    offset += Math.ceil(size / TAR_BLOCK) * TAR_BLOCK;
    // Only regular files carry an executable; "0" is the plain-file type
    // and a zero byte is what some writers put there instead.
    if ((type === "0" || type === "7") && basename(full) === wanted) {
      return Buffer.from(data);
    }
  }
  return null;
}

/** Extracts the file named `wanted` from a zip archive. */
export function extractZip(bytes, wanted) {
  const directory = findZipCentralDirectory(bytes);
  let offset = directory.offset;
  for (let index = 0; index < directory.count; index += 1) {
    if (bytes.readUInt32LE(offset) !== ZIP_CENTRAL_HEADER) {
      throw new Error("malformed zip central directory");
    }
    const method = bytes.readUInt16LE(offset + 10);
    const compressedSize = bytes.readUInt32LE(offset + 20);
    const nameLength = bytes.readUInt16LE(offset + 28);
    const extraLength = bytes.readUInt16LE(offset + 30);
    const commentLength = bytes.readUInt16LE(offset + 32);
    const localOffset = bytes.readUInt32LE(offset + 42);
    const name = bytes.toString("utf8", offset + 46, offset + 46 + nameLength);
    offset += 46 + nameLength + extraLength + commentLength;
    if (compressedSize === ZIP64_SENTINEL || localOffset === ZIP64_SENTINEL) {
      throw new Error(`zip64 archives are not supported (${name}); publish a plain zip`);
    }
    if (basename(name) !== wanted) {
      continue;
    }
    if (bytes.readUInt32LE(localOffset) !== ZIP_LOCAL_HEADER) {
      throw new Error(`malformed zip local header for ${name}`);
    }
    const localNameLength = bytes.readUInt16LE(localOffset + 26);
    const localExtraLength = bytes.readUInt16LE(localOffset + 28);
    const dataStart = localOffset + 30 + localNameLength + localExtraLength;
    const data = bytes.subarray(dataStart, dataStart + compressedSize);
    if (method === ZIP_STORED) {
      return Buffer.from(data);
    }
    if (method === ZIP_DEFLATED) {
      return inflateRawSync(data);
    }
    throw new Error(`unsupported zip compression method ${method} for ${name}`);
  }
  return null;
}

/** Locates the zip end-of-central-directory record and the directory it points at. */
function findZipCentralDirectory(bytes) {
  const lowest = Math.max(0, bytes.length - 22 - 0xffff);
  for (let offset = bytes.length - 22; offset >= lowest; offset -= 1) {
    if (bytes.readUInt32LE(offset) !== ZIP_END_OF_CENTRAL_DIRECTORY) {
      continue;
    }
    return {
      count: bytes.readUInt16LE(offset + 10),
      offset: bytes.readUInt32LE(offset + 16),
    };
  }
  throw new Error("not a zip archive: no end-of-central-directory record");
}
