import type { ConvertOptions, FormatId, PlanStep } from "@ariadshift/ariad-wasm";
import { type Core, type Engine, runRoute } from "@/converter/engines";
import { MemoryStore } from "@/converter/file-store";
import { ConverterSession, INPUT_LIMIT_BYTES } from "@/converter/session";
import { loadCore } from "./wasm";

interface RecordedCall {
  engine: string;
  from: FormatId;
  to: FormatId;
  bytes: Uint8Array;
  options: ConvertOptions;
}

function fakeCore(
  stepsByPair: Record<string, PlanStep[]>,
  format: FormatId = "docx",
): {
  core: Core;
  calls: RecordedCall[];
} {
  const calls: RecordedCall[] = [];
  const makeEngine = (name: string): Engine => ({
    convert(bytes, from, to, options) {
      calls.push({ engine: name, from, to, bytes, options });
      return {
        bytes: new TextEncoder().encode(`output:${name}:${from}->${to}`),
        format: to,
        media_type: to === "html" ? "text/html" : "text/markdown",
        extension: to === "html" ? "html" : "md",
        route: [],
        warnings: [],
      };
    },
  });

  const core: Core = {
    version: () => "test",
    memoryBytes: () => 0,
    inspect: (_bytes, _name, totalBytes) => ({
      format,
      bytes: totalBytes ?? 0,
      meta: null,
      counts: null,
      warnings: [],
      reachable: ["markdown", "html", "docx", "epub", "pdf", "ariad-ir+json"],
      reachable_elsewhere: [],
    }),
    plan: (input, to) => ({
      steps: stepsByPair[`${input}->${to}`] ?? [],
      score: { fidelity: 1, editability: 1, estimated_duration_ms: 1 },
      measured: false,
      alternatives: [],
    }),
    engines: new Map<string, Engine>([
      ["pandoc", makeEngine("pandoc")],
      ["pdf", makeEngine("pdf")],
      ["ariad-core", makeEngine("ariad-core")],
    ]),
  };

  return { core, calls };
}

describe("runRoute input byte limits", () => {
  it("applies max_input_bytes to the original group and omits it for intermediate IR while keeping other limits", async () => {
    const { core, calls } = fakeCore({});
    const original = new Uint8Array([1, 2, 3, 4]);

    await runRoute(
      core,
      original,
      [
        { from: "docx", to: "ariad-ir+json", engine: "pandoc" },
        { from: "ariad-ir+json", to: "markdown", engine: "ariad-core" },
      ],
      {
        profile: "editable",
        title: "report",
        limits: {
          max_input_bytes: INPUT_LIMIT_BYTES,
          max_blocks: 500,
          max_ir_json_bytes: 64 * 1024 * 1024,
        },
      },
    );

    expect(calls).toHaveLength(2);
    expect(calls[0]).toMatchObject({
      engine: "pandoc",
      from: "docx",
      to: "ariad-ir+json",
      bytes: original,
      options: {
        profile: "editable",
        title: "report",
        limits: {
          max_input_bytes: INPUT_LIMIT_BYTES,
          max_blocks: 500,
          max_ir_json_bytes: 64 * 1024 * 1024,
        },
      },
    });
    expect(calls[1]).toMatchObject({
      engine: "ariad-core",
      from: "ariad-ir+json",
      to: "markdown",
      options: {
        profile: "editable",
        title: "report",
        limits: {
          max_blocks: 500,
          max_ir_json_bytes: 64 * 1024 * 1024,
        },
      },
    });
    expect(calls[1]?.options.limits?.max_input_bytes).toBeUndefined();
    expect("max_input_bytes" in (calls[1]?.options.limits ?? {})).toBe(false);
  });

  it("keeps max_input_bytes on a single-group route", async () => {
    const { core, calls } = fakeCore({});
    await runRoute(
      core,
      new TextEncoder().encode("# Title\n"),
      [
        { from: "markdown", to: "ariad-ir+json", engine: "ariad-core" },
        { from: "ariad-ir+json", to: "html", engine: "ariad-core" },
      ],
      {
        profile: "editable",
        title: "notes",
        limits: { max_input_bytes: INPUT_LIMIT_BYTES },
      },
    );

    expect(calls).toHaveLength(1);
    expect(calls[0]?.options.limits).toEqual({ max_input_bytes: INPUT_LIMIT_BYTES });
  });

  it.each([
    ["docx", "markdown", "pandoc"],
    ["docx", "html", "pandoc"],
    ["epub", "markdown", "pandoc"],
    ["epub", "html", "pandoc"],
    ["pdf", "markdown", "pdf"],
    ["pdf", "html", "pdf"],
  ] as const)(
    "ConverterSession passes max_input_bytes only to the first group on %s -> %s",
    async (source, target, firstEngine) => {
      const { core, calls } = fakeCore(
        {
          [`${source}->${target}`]: [
            { from: source, to: "ariad-ir+json", engine: firstEngine },
            { from: "ariad-ir+json", to: target, engine: "ariad-core" },
          ],
        },
        source,
      );
      const session = new ConverterSession(core, new MemoryStore());
      await session.ingest(new File([new Uint8Array([1, 2, 3])], `sample.${source}`));
      await session.convert(target, "editable");

      expect(calls).toHaveLength(2);
      expect(calls[0]?.options.limits).toEqual({ max_input_bytes: INPUT_LIMIT_BYTES });
      expect(calls[1]?.options.limits).toEqual({});
      expect(calls[1]?.options.limits?.max_input_bytes).toBeUndefined();
      expect("max_input_bytes" in (calls[1]?.options.limits ?? {})).toBe(false);
    },
  );

  it("converts intermediate IR JSON that expands past the session input byte cap", async () => {
    const realCore = loadCore();
    const coreEngine = realCore.engines.get("ariad-core");
    if (!coreEngine) {
      throw new Error("expected ariad-core engine");
    }
    const produced = await coreEngine.convert(
      new TextEncoder().encode(`# Intermediate IR heading\n\n${"x".repeat(512)}\n`),
      "markdown",
      "ariad-ir+json",
      {},
    );
    const irBytes = produced.bytes;
    const smallCap = 64;
    expect(irBytes.byteLength).toBeGreaterThan(smallCap);

    const pandoc: Engine = {
      convert() {
        return {
          bytes: irBytes,
          format: "ariad-ir+json",
          media_type: "application/json",
          extension: "ir.json",
          route: [],
          warnings: [],
        };
      },
    };
    const core: Core = {
      ...realCore,
      inspect: () => ({
        format: "docx",
        bytes: 8,
        meta: null,
        counts: null,
        warnings: [],
        reachable: ["markdown", "html"],
        reachable_elsewhere: [],
      }),
      engines: new Map([...realCore.engines, ["pandoc", pandoc]]),
    };

    const session = new ConverterSession(core, new MemoryStore(), smallCap);
    await session.ingest(new File([new Uint8Array(8)], "small.docx"));
    const converted = await session.convert("markdown", "editable");
    expect(converted.name).toBe("small.md");
    if (converted.location.kind !== "memory") {
      throw new Error("expected a memory location");
    }
    const text = new TextDecoder().decode(await converted.location.blob.arrayBuffer());
    expect(text).toContain("# Intermediate IR heading");
  });
});
