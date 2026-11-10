import { resolveWasmUrl, sourceToBuffer, webAssemblySupported } from "@/viewer/engine";

describe("viewer engine configuration", () => {
  it("resolves the PDFium module to an absolute URL against the page", () => {
    expect(resolveWasmUrl(undefined, "https://site.test/en/app?x=1")).toBe(
      "https://site.test/wasm/pdfium/pdfium.wasm?v=2.15.1",
    );
    expect(resolveWasmUrl("/wasm/other.wasm", "https://site.test/en/app")).toBe(
      "https://site.test/wasm/other.wasm",
    );
  });

  it("keeps a path-prefixed deployment working with a relative path", () => {
    expect(resolveWasmUrl("../wasm/pdfium/pdfium.wasm", "https://site.test/docs/en/app")).toBe(
      "https://site.test/docs/wasm/pdfium/pdfium.wasm",
    );
  });

  it("refuses a module from another origin", () => {
    expect(() =>
      resolveWasmUrl("https://cdn.example.com/pdfium.wasm", "https://site.test/"),
    ).toThrow(/same origin/);
  });

  it("needs a page location to resolve against when none is given", () => {
    expect(() => resolveWasmUrl("/x.wasm")).toThrow(/location/);
  });

  it("detects WebAssembly", () => {
    expect(webAssemblySupported()).toBe(true);
  });

  it("reads every source shape into an independent ArrayBuffer", async () => {
    const bytes = new Uint8Array([1, 2, 3, 4, 5]);
    expect([...new Uint8Array(await sourceToBuffer({ data: new Blob([bytes]) }))]).toEqual([
      1, 2, 3, 4, 5,
    ]);
    const buffer = bytes.buffer.slice(0) as ArrayBuffer;
    const copy = await sourceToBuffer({ data: buffer });
    expect(copy).not.toBe(buffer);
    expect([...new Uint8Array(copy)]).toEqual([1, 2, 3, 4, 5]);
    // A view into a larger buffer yields only its own bytes.
    expect([...new Uint8Array(await sourceToBuffer({ data: bytes.subarray(1, 4) }))]).toEqual([
      2, 3, 4,
    ]);
  });
});
