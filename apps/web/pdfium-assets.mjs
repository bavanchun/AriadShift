// The PDF viewer's engine: the unmodified PDFium WebAssembly module shipped by
// `@embedpdf/pdfium`, aggregated as a separate static file next to the licence texts the
// package ships with it. `scripts/sync-pdfium.mjs` refuses any other binary.
export const pdfiumPackageVersion = "2.15.1";
export const pdfiumWasmSha256 = "5e4cd023c3dad4a895b3571ca573d3fc51bac6de48360d95283134385b954eaa";
// Same-origin path of the copied module, relative to the site root. The viewer resolves it
// to an absolute URL because the engine worker is a blob: worker.
export const pdfiumWasmPath = `/wasm/pdfium/pdfium.wasm?v=${pdfiumPackageVersion}`;
