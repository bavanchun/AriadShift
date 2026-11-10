import { readFileSync } from "node:fs";
import { ConverterClient } from "@/converter/client";
import { MemoryStore } from "@/converter/file-store";
import type { RequestEnvelope, WorkerResponse } from "@/converter/protocol";
import { ConverterSession, handleRequest, type WorkerContext } from "@/converter/session";
import { toolCatalog } from "@/converter/tool-catalog";
import registry from "../../../schemas/tools.v1.json";
import { loadCore } from "./wasm";

const fixture = new Uint8Array(
  readFileSync(new URL("../../../fixtures/pdf-forms/forms-all-types.pdf", import.meta.url)),
);
const file = (name = "form.pdf") => new File([fixture], name);
const session = () => new ConverterSession(loadCore(), new MemoryStore());

describe("browser document tool runner", () => {
  it("merges the complete static registry with the runtime catalog without losing local tools", () => {
    const catalog = toolCatalog(loadCore().listTools?.());
    expect(catalog).toHaveLength(registry.tools.length);
    expect(catalog.find((tool) => tool.id === "pdf.encrypt")?.runtimes).toEqual(["local"]);
    expect(catalog.find((tool) => tool.id === "pdf.merge")?.inputs).toMatchObject({
      arity: "many",
      min: 2,
    });
    catalog[0]?.secret_params.push("mutated");
    expect(toolCatalog()[0]?.secret_params).not.toContain("mutated");
  });

  it("ingests slots, honors selected input order, merges and stores all split artifacts", async () => {
    const runner = session();
    const slots = await runner.ingestMany([file("first.pdf"), file("second.pdf")]);
    expect(new Set(slots.map((slot) => slot.id)).size).toBe(2);
    const merged = await runner.runTool({
      type: "runTool",
      toolId: "pdf.merge",
      params: {},
      inputIds: slots.map((slot) => slot.id).reverse(),
    });
    expect(merged.artifacts).toHaveLength(1);
    expect(merged.primaryLocation).toEqual(merged.artifacts[0]?.location);
    const split = await runner.runTool({
      type: "runTool",
      toolId: "pdf.split",
      params: { mode: "every", every: 1 },
      inputIds: [slots[0]?.id as string],
    });
    expect(split.artifacts.length).toBeGreaterThan(1);
    for (const artifact of split.artifacts) {
      expect(artifact.location.kind).toBe("memory");
      if (artifact.location.kind === "memory")
        expect(artifact.location.blob.size).toBe(artifact.size);
    }
    expect(split.elapsedMs).toBeGreaterThanOrEqual(0);
    expect(split.format).toBe("pdf");
  });

  it("accepts auxiliary form payloads even when conversion inspection cannot classify them", async () => {
    const exported = loadCore().runTool?.(
      [fixture],
      ["form.pdf"],
      "pdf.forms_export",
      '{"format":"json"}',
      16 * 1024 * 1024,
      false,
    );
    const runner = session();
    const slots = await runner.ingestMany([
      file(),
      new File([exported?.bytes as Uint8Array<ArrayBuffer>], "fields.json"),
    ]);
    expect(slots).toHaveLength(2);
    expect(slots[1]?.inspection).toBeNull();
    expect(slots[1]?.inspectionError?.code).toBe("unsupported_ir_version");
    const imported = await runner.runTool({
      type: "runTool",
      toolId: "pdf.forms_import",
      params: {},
    });
    expect(imported.format).toBe("pdf");
    expect(imported.artifacts[0]?.size).toBeGreaterThan(0);
  });

  it("returns structured JSON rather than copying opaque hidden WASM properties", async () => {
    const runner = session();
    await runner.ingest(file());
    const result = await runner.runTool({ type: "runTool", toolId: "pdf.info", params: {} });
    expect(result.data?.page_count).toBeGreaterThan(0);
    expect(structuredClone(result).data).toEqual(result.data);
  });

  it("rejects unknown, unavailable, missing-slot, invalid-param and over-budget requests", async () => {
    const runner = session();
    const [slot] = await runner.ingestMany([file()]);
    for (const [request, code] of [
      [{ toolId: "missing.tool", params: {} }, "unknown_tool"],
      [{ toolId: "pdf.encrypt", params: {} }, "engine_unavailable"],
      [{ toolId: "pdf.info", params: {}, inputIds: ["absent"] }, "no_input"],
      [{ toolId: "pdf.info", params: {}, budgetBytes: 1 }, "limit_exceeded"],
      [{ toolId: "pdf.info", params: {}, budgetBytes: Number.NaN }, "invalid_params"],
      [
        { toolId: "pdf.rotate", params: { angle: 13 }, inputIds: [slot?.id as string] },
        "invalid_params",
      ],
    ] as const) {
      await expect(
        runner.runTool({
          type: "runTool",
          ...request,
          inputIds: "inputIds" in request ? [...request.inputIds] : undefined,
        }),
      ).rejects.toMatchObject({ code });
    }
    await runner.reset();
    await expect(runner.inspectInsights()).rejects.toMatchObject({ code: "no_input" });
  });

  it("dispatches all new worker messages through the serialized session", async () => {
    const context: Promise<WorkerContext> = Promise.resolve({
      session: session(),
      storageSession: "test",
      version: "test",
      memoryLimitBytes: 64,
      resources: () => [],
    });
    expect((await handleRequest(context, { id: 1, type: "listTools" })).ok).toBe(true);
    const ingested = await handleRequest(context, { id: 2, type: "ingestMany", files: [file()] });
    expect(ingested.ok).toBe(true);
    expect((await handleRequest(context, { id: 3, type: "inspectInsights" })).ok).toBe(true);
    const result = await handleRequest(context, {
      id: 4,
      type: "runTool",
      toolId: "pdf.info",
      params: {},
    });
    expect(result).toMatchObject({ ok: true, result: { tool: "pdf.info", format: "json" } });
  });
});

it("exposes client helpers and recycles large modules before another multi-file ingest", async () => {
  const workers: Array<{ terminated: boolean; received: RequestEnvelope[] }> = [];
  const client = new ConverterClient(() => {
    const context = Promise.resolve({
      session: new ConverterSession(
        { ...loadCore(), memoryBytes: () => 512 * 1024 * 1024 },
        new MemoryStore(),
      ),
      storageSession: `session-${workers.length}`,
      version: "test",
      memoryLimitBytes: 64,
      resources: () => [],
    });
    const state = { terminated: false, received: [] as RequestEnvelope[] };
    workers.push(state);
    const target = new EventTarget();
    return Object.assign(target, {
      postMessage(request: RequestEnvelope) {
        state.received.push(request);
        void handleRequest(context, request).then((response: WorkerResponse) => {
          if (!state.terminated)
            target.dispatchEvent(new MessageEvent("message", { data: response }));
        });
      },
      terminate() {
        state.terminated = true;
      },
    }) as unknown as Worker;
  });
  expect((await client.listTools()).length).toBe(registry.tools.length);
  await client.ingestMany([file()]);
  expect((await client.inspectInsights()).hasForms).toBe(true);
  expect((await client.runTool("pdf.info", {})).data?.page_count).toBeGreaterThan(0);
  await client.ingestMany([file("next.pdf")]);
  expect(workers[0]?.terminated).toBe(true);
  expect(workers[1]?.received.map((request) => request.type)).toContain("retire");
  client.terminate();
});
