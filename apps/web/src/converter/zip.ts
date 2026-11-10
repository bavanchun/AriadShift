import type { ConverterError } from "./protocol";

export interface ZipEntryInput {
  name: string;
  bytes: Uint8Array;
  mtime?: Date;
}

export interface ZipOptions {
  /** Includes file data, names, headers and the central directory. */
  maxTotalBytes?: number;
}

export const DEFAULT_ZIP_LIMIT_BYTES = 256 * 1024 * 1024;
const ZIP_MIN_TIMESTAMP = Date.UTC(1980, 0, 1);
const ZIP_MAX_TIMESTAMP = Date.UTC(2107, 11, 31, 23, 59, 58);
const encoder = new TextEncoder();
const crcTable = Uint32Array.from({ length: 256 }, (_, value) => {
  let crc = value;
  for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
  return crc >>> 0;
});

function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff;
  for (const byte of bytes) crc = (crc >>> 8) ^ (crcTable[(crc ^ byte) & 0xff] as number);
  return (crc ^ 0xffffffff) >>> 0;
}

/** Flatten paths so archive labels can never become extraction paths. */
function safeName(name: string): string {
  const base = name.replaceAll("\\", "/").split("/").pop() ?? "";
  const clean = Array.from(base, (character) => {
    const code = character.charCodeAt(0);
    return code < 32 || code === 127 || '<>:"|?*'.includes(character) ? "_" : character;
  }).join("");
  return clean.replace(/[. ]+$/, "") || "document";
}

function uniqueName(name: string, used: Set<string>): string {
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 ? name.slice(0, dot) : name;
  const extension = dot > 0 ? name.slice(dot) : "";
  let candidate = name;
  // Names are compared case-folded: `A.pdf` and `a.pdf` collide on macOS and Windows.
  for (let index = 2; used.has(candidate.toLowerCase()); index++)
    candidate = `${stem} (${index})${extension}`;
  used.add(candidate.toLowerCase());
  return candidate;
}

function limitExceeded(): ConverterError {
  return { code: "limit_exceeded", message: "The ZIP archive exceeds its byte or ZIP32 limits." };
}

/**
 * Writes a ZIP32 archive using Store (method 0), supported by every ZIP reader.
 * Store keeps this API synchronous and works without CompressionStream support.
 * Missing timestamps use the ZIP epoch so identical entries produce identical bytes.
 */
export function createZipArchive(
  entries: ZipEntryInput[],
  options: ZipOptions = {},
): Uint8Array<ArrayBuffer> {
  const limit = options.maxTotalBytes ?? DEFAULT_ZIP_LIMIT_BYTES;
  if (!Number.isSafeInteger(limit) || limit <= 0)
    throw {
      code: "invalid_params",
      message: "The ZIP byte limit must be a positive safe integer.",
    } satisfies ConverterError;
  if (entries.length >= 0xffff) throw limitExceeded();
  const used = new Set<string>();
  let size = 22;
  const prepared = entries.map((entry) => {
    const name = encoder.encode(uniqueName(safeName(entry.name), used));
    size += 30 + 46 + 2 * name.length + entry.bytes.byteLength;
    if (name.length > 0xffff || size > limit || size >= 0xffffffff) throw limitExceeded();
    const timestamp = (entry.mtime ?? new Date(ZIP_MIN_TIMESTAMP)).getTime();
    if (!Number.isFinite(timestamp))
      throw {
        code: "invalid_params",
        message: "The ZIP entry timestamp is invalid.",
      } satisfies ConverterError;
    const date = new Date(Math.max(ZIP_MIN_TIMESTAMP, Math.min(ZIP_MAX_TIMESTAMP, timestamp)));
    return {
      name,
      bytes: entry.bytes,
      time: (date.getUTCHours() << 11) | (date.getUTCMinutes() << 5) | (date.getUTCSeconds() >>> 1),
      date:
        ((date.getUTCFullYear() - 1980) << 9) | ((date.getUTCMonth() + 1) << 5) | date.getUTCDate(),
    };
  });
  if (size > limit) throw limitExceeded();
  const archive = new Uint8Array(size);
  const view = new DataView(archive.buffer);
  let offset = 0;
  let directoryOffset = prepared.reduce(
    (sum, entry) => sum + 30 + entry.name.length + entry.bytes.length,
    0,
  );
  const directoryStart = directoryOffset;
  for (const entry of prepared) {
    const crc = crc32(entry.bytes);
    view.setUint32(offset, 0x04034b50, true);
    view.setUint16(offset + 4, 20, true);
    view.setUint16(offset + 6, 0x0800, true);
    view.setUint16(offset + 10, entry.time, true);
    view.setUint16(offset + 12, entry.date, true);
    view.setUint32(offset + 14, crc, true);
    view.setUint32(offset + 18, entry.bytes.length, true);
    view.setUint32(offset + 22, entry.bytes.length, true);
    view.setUint16(offset + 26, entry.name.length, true);
    archive.set(entry.name, offset + 30);
    archive.set(entry.bytes, offset + 30 + entry.name.length);

    view.setUint32(directoryOffset, 0x02014b50, true);
    view.setUint16(directoryOffset + 4, 20, true);
    view.setUint16(directoryOffset + 6, 20, true);
    view.setUint16(directoryOffset + 8, 0x0800, true);
    view.setUint16(directoryOffset + 12, entry.time, true);
    view.setUint16(directoryOffset + 14, entry.date, true);
    view.setUint32(directoryOffset + 16, crc, true);
    view.setUint32(directoryOffset + 20, entry.bytes.length, true);
    view.setUint32(directoryOffset + 24, entry.bytes.length, true);
    view.setUint16(directoryOffset + 28, entry.name.length, true);
    view.setUint32(directoryOffset + 42, offset, true);
    archive.set(entry.name, directoryOffset + 46);
    offset += 30 + entry.name.length + entry.bytes.length;
    directoryOffset += 46 + entry.name.length;
  }
  view.setUint32(directoryOffset, 0x06054b50, true);
  view.setUint16(directoryOffset + 8, prepared.length, true);
  view.setUint16(directoryOffset + 10, prepared.length, true);
  view.setUint32(directoryOffset + 12, directoryOffset - directoryStart, true);
  view.setUint32(directoryOffset + 16, directoryStart, true);
  return archive;
}

export function createZipBlob(entries: ZipEntryInput[], options: ZipOptions = {}): Blob {
  return new Blob([createZipArchive(entries, options)], { type: "application/zip" });
}
