// The download, runtime URL and offline manifest share the same Pandoc version.
export const pandocVersion = "3.12";
export const pandocUrl = `/wasm/pandoc/pandoc.wasm?v=${pandocVersion}`;
// SHA-256 of the unmodified pandoc.wasm in the pinned release archive. The sync script
// refuses any other binary, and the browser checks it before caching or compiling.
export const pandocWasmSha256 = "1c69902d11b31711d510f2e317263fb4a5954601117b41038121ea2979110bf8";
