// Engine configuration the EmbedPDF integration requires (see the isolation POC report):
// the engine runs in a worker, and the PDFium module URL is absolute and same-origin.
import { pdfiumWasmPath } from "../../pdfium-assets.mjs";

/**
 * The engine worker is a blob: worker, so a relative URL would resolve against the blob's
 * origin and fail without any console error. Resolve against the page instead.
 */
export function resolveWasmUrl(path: string = pdfiumWasmPath, base?: string): string {
  const root = base ?? (typeof location === "undefined" ? undefined : location.href);
  if (!root) throw new Error("The PDFium module URL needs a page location to resolve against.");
  const page = new URL(root);
  const url = new URL(path, page);
  if (url.origin !== page.origin)
    throw new Error("The PDFium module must be served from the same origin as the page.");
  return url.href;
}

export function webAssemblySupported(): boolean {
  return typeof WebAssembly !== "undefined" && typeof WebAssembly.instantiate === "function";
}

export async function sourceToBuffer(source: {
  data: Blob | ArrayBuffer | Uint8Array;
}): Promise<ArrayBuffer> {
  const { data } = source;
  if (data instanceof Blob) return data.arrayBuffer();
  if (data instanceof ArrayBuffer) return data.slice(0);
  return data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength) as ArrayBuffer;
}
