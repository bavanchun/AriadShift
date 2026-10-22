import { groupSteps, runRoute } from "@/converter/engines";
import { MemoryStore } from "@/converter/file-store";
import type { ConvertedFile, RequestEnvelope } from "@/converter/protocol";
import {
  ANALYSIS_LIMIT_BYTES,
  ConverterSession,
  fileStem,
  handleRequest,
  serialize,
  toConverterError,
  type WorkerContext,
} from "@/converter/session";
import { loadCore } from "./wasm";

const decoder = new TextDecoder();

function markdownFile(text: string, name = "notes.md") {
  return new File([text], name, { type: "text/markdown" });
}

async function outputText(file: ConvertedFile) {
  if (file.location.kind !== "memory") {
    throw new Error("expected a memory location");
  }
  return decoder.decode(await file.location.blob.arrayBuffer());
}

describe("ConverterSession", () => {
  it("ingests, inspects, plans and converts Markdown to HTML", async () => {
    const session = new ConverterSession(loadCore(), new MemoryStore());
    const ingested = await session.ingest(markdownFile("# Notes\n\nHello *there*.\n"));
    expect(ingested.inspection.format).toBe("markdown");
    expect(ingested.inspection.reachable).toEqual(["ariad-ir+json", "html"]);

    const route = session.plan("html", "editable");
    expect(route.steps).toHaveLength(2);

    expect(ingested.recycle).toBe(false);

    const output = await session.convert("html", "editable");
    expect(output.name).toBe("notes.html");
    expect(output.recycle).toBe(false);
    expect(output.mediaType).toBe("text/html");
    const html = await outputText(output);
    expect(html).toContain("<title>notes</title>");
    expect(html).toContain("<em>there</em>");
  });

  it("names IR JSON outputs and strips .ir.json from inputs", async () => {
    const session = new ConverterSession(loadCore(), new MemoryStore());
    await session.ingest(markdownFile("# A\n", "doc.md"));
    const ir = await session.convert("ariad-ir+json", "editable");
    expect(ir.name).toBe("doc.ir.json");

    await session.ingest(new File([await outputText(ir)], "doc.ir.json"));
    const markdown = await session.convert("markdown", "editable");
    expect(markdown.name).toBe("doc.md");
    expect(await outputText(markdown)).toContain("# A");
  });

  it("requires an input and forgets it on reset", async () => {
    const store = new MemoryStore();
    const session = new ConverterSession(loadCore(), store);
    await expect(session.convert("html", "editable")).rejects.toMatchObject({ code: "no_input" });
    await session.ingest(markdownFile("x"));
    await session.reset();
    expect(() => session.plan("html", "editable")).toThrow();
    await expect(store.readInput()).rejects.toMatchObject({ code: "no_input" });
  });

  it("reports browser-unavailable targets as typed errors", async () => {
    const session = new ConverterSession(loadCore(), new MemoryStore());
    await session.ingest(markdownFile("# Hi\n"));
    expect(() => session.plan("docx", "editable")).toThrow(
      expect.objectContaining({ code: "no_route" }),
    );
  });

  it("refuses inputs above the memory store limit", async () => {
    const session = new ConverterSession(loadCore(), new MemoryStore(4));
    await expect(session.ingest(markdownFile("too long"))).rejects.toMatchObject({
      code: "storage_limit",
    });
  });

  it("refuses inputs above the browser limit before storing them", async () => {
    const store = new MemoryStore();
    const session = new ConverterSession(loadCore(), store, 8);
    await expect(session.ingest(markdownFile("more than eight bytes"))).rejects.toMatchObject({
      code: "limit_exceeded",
      limit: "browser_input_bytes",
    });
    await expect(store.readInput()).rejects.toMatchObject({ code: "no_input" });
  });

  it("classifies large inputs from a prefix without reading them", async () => {
    const session = new ConverterSession(loadCore(), new MemoryStore());
    const text = `# Big\n\n${"word ".repeat(ANALYSIS_LIMIT_BYTES / 5 + 10)}\n`;
    const ingested = await session.ingest(markdownFile(text, "big.md"));
    expect(ingested.size).toBeGreaterThan(ANALYSIS_LIMIT_BYTES);
    expect(ingested.inspection.format).toBe("markdown");
    expect(ingested.inspection.counts).toBeNull();
    expect(ingested.inspection.warnings.map((warning) => warning.code)).toEqual([
      "document_too_large",
    ]);
  });

  it("asks for a new worker once the module memory is large", async () => {
    const core = { ...loadCore(), memoryBytes: () => 512 * 1024 * 1024 };
    const session = new ConverterSession(core, new MemoryStore());
    expect((await session.ingest(markdownFile("# Hi\n"))).recycle).toBe(true);
  });

  it("passes retired sessions to the store", async () => {
    const store = new MemoryStore();
    const retire = vi.spyOn(store, "retire").mockResolvedValue(1);
    const session = new ConverterSession(loadCore(), store);
    expect(await session.retire(["old"])).toEqual({ held: 1 });
    expect(retire).toHaveBeenCalledWith(["old"]);
  });
});

describe("routes", () => {
  it("groups consecutive steps of one engine", () => {
    expect(
      groupSteps([
        { from: "markdown", to: "ariad-ir+json", engine: "ariad-core" },
        { from: "ariad-ir+json", to: "html", engine: "ariad-core" },
      ]),
    ).toEqual([{ engine: "ariad-core", from: "markdown", to: "html" }]);
    expect(
      groupSteps([
        { from: "markdown", to: "ariad-ir+json", engine: "ariad-core" },
        { from: "ariad-ir+json", to: "docx", engine: "pandoc" },
      ]).map((group) => group.engine),
    ).toEqual(["ariad-core", "pandoc"]);
  });

  it("hands each run of steps to its engine", async () => {
    const core = loadCore();
    const calls: string[] = [];
    const docx = {
      convert: (bytes: Uint8Array, from: string, to: string) => {
        calls.push(`${from}>${to}:${decoder.decode(bytes).includes("ariad-ir/0")}`);
        return {
          bytes: new Uint8Array([80, 75]),
          format: to,
          media_type: "application/test",
          extension: "docx",
          route: [],
          warnings: [{ code: "test_warning", message: "from the second engine" }],
        };
      },
    };
    const output = await runRoute(
      { ...core, engines: new Map([...core.engines, ["pandoc", docx]]) },
      new TextEncoder().encode("# Hi\n"),
      [
        { from: "markdown", to: "ariad-ir+json", engine: "ariad-core" },
        { from: "ariad-ir+json", to: "docx", engine: "pandoc" },
      ],
      { title: "hi" },
    );
    expect(calls).toEqual(["ariad-ir+json>docx:true"]);
    expect(output.extension).toBe("docx");
    expect(output.warnings.map((warning) => warning.code)).toEqual(["test_warning"]);
  });

  it("fails cleanly for engines the browser does not have", async () => {
    await expect(
      runRoute(
        loadCore(),
        new Uint8Array(),
        [{ from: "docx", to: "ariad-ir+json", engine: "pandoc" }],
        {},
      ),
    ).rejects.toMatchObject({ code: "engine_unavailable", engine: "pandoc" });
  });
});

describe("worker requests", () => {
  const context: Promise<WorkerContext> = Promise.resolve({
    session: new ConverterSession(loadCore(), new MemoryStore()),
    storageSession: "session-1",
    version: "test",
    memoryLimitBytes: 4,
    resources: () => ["https://example.test/worker.js"],
  });

  it("answers init with the storage kind and resources", async () => {
    const response = await handleRequest(context, { id: 1, type: "init" } as RequestEnvelope);
    expect(response).toEqual({
      id: 1,
      ok: true,
      result: {
        version: "test",
        session: "session-1",
        storage: "memory",
        memoryLimitBytes: 4,
        inputLimitBytes: 16 * 1024 * 1024,
        resources: ["https://example.test/worker.js"],
      },
    });
  });

  it("returns typed errors instead of throwing", async () => {
    const response = await handleRequest(context, {
      id: 2,
      type: "convert",
      to: "html",
      profile: "editable",
    });
    expect(response).toMatchObject({ id: 2, ok: false, error: { code: "no_input" } });
  });

  it("runs requests one at a time in arrival order", async () => {
    const queue = serialize<string>();
    const order: string[] = [];
    const slow = queue(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
      order.push("slow");
      return "slow";
    });
    const failing = queue(async () => {
      order.push("failing");
      throw new Error("boom");
    });
    const fast = queue(async () => {
      order.push("fast");
      return "fast";
    });
    await expect(slow).resolves.toBe("slow");
    await expect(failing).rejects.toThrow("boom");
    await expect(fast).resolves.toBe("fast");
    expect(order).toEqual(["slow", "failing", "fast"]);
  });

  it("keeps storage failures non-fatal", () => {
    expect(toConverterError(new DOMException("full", "QuotaExceededError"))).toEqual({
      code: "storage_full",
      message: "The browser has no room left to store this file.",
    });
    expect(toConverterError(new DOMException("gone", "NotFoundError"))).toMatchObject({
      code: "storage_error",
    });
    expect(toConverterError(new DOMException("gone", "NotFoundError")).fatal).toBeUndefined();
  });

  it("marks unexpected failures as fatal", () => {
    expect(toConverterError(new RangeError("out of memory"))).toMatchObject({
      code: "internal_error",
      fatal: true,
    });
    expect(toConverterError({ code: "no_route", message: "x" })).toEqual({
      code: "no_route",
      message: "x",
    });
  });
});

describe("fileStem", () => {
  it.each([
    ["notes.md", "notes"],
    ["Report.Final.html", "Report.Final"],
    ["doc.IR.JSON", "doc"],
    [".md", ".md"],
    ["", "document"],
    ["README", "README"],
  ])("%s → %s", (name, stem) => {
    expect(fileStem(name)).toBe(stem);
  });
});
