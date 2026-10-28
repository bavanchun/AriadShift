// The download, runtime URL and offline manifest share the same Pandoc version.
export const pandocVersion = "3.12";
export const pandocUrl = `/wasm/pandoc/pandoc.wasm?v=${pandocVersion}`;
// SHA-256 of the upstream pandoc-3.12.wasm.zip release asset and the unmodified
// inner pandoc.wasm binary. The sync script refuses any other binary, and the
// browser checks pandocWasmSha256 before caching or compiling.
export const pandocArchiveSha256 =
  "f14bc3e7722c8bdd58188707e8d5445ce1ac909eae43373f903b45c2f7390cf9";
export const pandocWasmSha256 = "1c69902d11b31711d510f2e317263fb4a5954601117b41038121ea2979110bf8";
