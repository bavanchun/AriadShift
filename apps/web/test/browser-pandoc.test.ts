import { describe, expect, it, vi } from "vitest";
import type { Core, Engine } from "@/converter/engines";
import { browserEngines } from "@/converter/browser-engines";

let lastTarget = "";
let stampCalls: { bytes: Uint8Array; target: string; timestamp: string }[] = [];

vi.mock("@/converter/pandoc", () => ({
  Pandoc: class {
    convert(_bytes: Uint8Array, _from: string, to: string) {
      lastTarget = to;
      return Promise.resolve({
        bytes: new Uint8Array([10, 20, 30]),
        warnings: [],
      });
    }
  },
}));

vi.mock("@ariadshift/ariad-wasm", () => ({
  writePandoc: (bytes: Uint8Array) => ({ bytes, warnings: [] }),
  readPandoc: (bytes: Uint8Array) => ({ bytes, warnings: [] }),
  stampPackage: (bytes: Uint8Array, target: string, timestamp: string) => {
    stampCalls.push({ bytes, target, timestamp });
    const stamped = new Uint8Array(bytes.length + 1);
    stamped.set(bytes);
    stamped[bytes.length] = 99;
    return stamped;
  },
}));

function fakeCore(): Core {
  const dummyEngine: Engine = {
    convert(_bytes, _from, to) {
      return {
        bytes: new Uint8Array(),
        format: to,
        media_type: "application/octet-stream",
        extension: to,
        route: [],
        warnings: [],
      };
    },
  };
  return {
    version: () => "0.3.0",
    memoryBytes: () => 0,
    inspect: () => ({
      format: "pdf",
      bytes: 0,
      meta: null,
      counts: null,
      warnings: [],
      reachable: [],
      reachable_elsewhere: [],
    }),
    plan: () => ({
      steps: [],
      score: { fidelity: 1, editability: 1, estimated_duration_ms: 1 },
      measured: false,
      alternatives: [],
    }),
    engines: new Map<string, Engine>([["ariad-core", dummyEngine]]),
  };
}

describe("browserEngines Pandoc media types and package stamping", () => {
  it("maps odt media type and invokes stampPackage with odt", async () => {
    stampCalls = [];
    const core = browserEngines(fakeCore());
    const pandocEngine = core.engines.get("pandoc");
    expect(pandocEngine).toBeDefined();

    const inputIr = new TextEncoder().encode('{"version":"ariad-ir/1","body":[]}');
    const result = await pandocEngine!.convert(inputIr, "ariad-ir+json", "odt", {});

    expect(result.format).toBe("odt");
    expect(result.extension).toBe("odt");
    expect(result.media_type).toBe("application/vnd.oasis.opendocument.text");
    expect(lastTarget).toBe("odt");
    expect(stampCalls).toHaveLength(1);
    expect(stampCalls[0].target).toBe("odt");
    expect(result.bytes).toEqual(new Uint8Array([10, 20, 30, 99]));
  });

  it("maps rtf media type and bypasses zip stampPackage", async () => {
    stampCalls = [];
    const core = browserEngines(fakeCore());
    const pandocEngine = core.engines.get("pandoc");
    expect(pandocEngine).toBeDefined();

    const inputIr = new TextEncoder().encode('{"version":"ariad-ir/1","body":[]}');
    const result = await pandocEngine!.convert(inputIr, "ariad-ir+json", "rtf", {});

    expect(result.format).toBe("rtf");
    expect(result.extension).toBe("rtf");
    expect(result.media_type).toBe("application/rtf");
    expect(lastTarget).toBe("rtf");
    expect(stampCalls).toHaveLength(0); // RTF is plain text, never zip-stamped
    expect(result.bytes).toEqual(new Uint8Array([10, 20, 30]));
  });

  it("maps docx and epub media types with respective targets", async () => {
    stampCalls = [];
    const core = browserEngines(fakeCore());
    const pandocEngine = core.engines.get("pandoc");

    const inputIr = new TextEncoder().encode('{"version":"ariad-ir/1","body":[]}');

    const docxResult = await pandocEngine!.convert(inputIr, "ariad-ir+json", "docx", {});
    expect(docxResult.media_type).toBe(
      "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    );
    expect(lastTarget).toBe("docx");
    expect(stampCalls[0].target).toBe("docx");

    const epubResult = await pandocEngine!.convert(inputIr, "ariad-ir+json", "epub", {});
    expect(epubResult.media_type).toBe("application/epub+zip");
    expect(lastTarget).toBe("epub3"); // epub passes epub3 to pandoc
    expect(stampCalls[1].target).toBe("epub");
  });
});
