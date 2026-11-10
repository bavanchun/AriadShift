import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { type BatchOptions, BatchRunner } from "@/converter/batch";
import { ConverterClient, openOutput } from "@/converter/client";
import { MemoryStore } from "@/converter/file-store";
import { batchHandoff, recipeHandoff } from "@/converter/handoff";
import type { RequestEnvelope, WorkerResponse } from "@/converter/protocol";
import { ConverterSession, handleRequest } from "@/converter/session";
import { toolCatalog } from "@/converter/tool-catalog";
import { createZipArchive, createZipBlob } from "@/converter/zip";
import { loadCore } from "./wasm";

const encoder = new TextEncoder();
const decoder = new TextDecoder();
const fixture = new Uint8Array(
  readFileSync(new URL("../../../fixtures/pdf-forms/forms-all-types.pdf", import.meta.url)),
);
const file = (name: string) => new File([fixture], name);
const tool = (id: string) => {
  const found = toolCatalog().find((entry) => entry.id === id);
  if (!found) throw new Error(`Missing tool: ${id}`);
  return found;
};

/** Independent central-directory walk also checks every referenced local header. */
function readZip(bytes: Uint8Array) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const end = bytes.length - 22;
  expect(view.getUint32(end, true)).toBe(0x06054b50);
  expect(view.getUint32(end + 4, true)).toBe(0);
  expect(view.getUint16(end + 20, true)).toBe(0);
  const count = view.getUint16(end + 10, true);
  expect(view.getUint16(end + 8, true)).toBe(count);
  let offset = view.getUint32(end + 16, true);
  expect(view.getUint32(end + 12, true)).toBe(end - offset);
  const entries = [];
  for (let index = 0; index < count; index++) {
    expect(view.getUint32(offset, true)).toBe(0x02014b50);
    const nameLength = view.getUint16(offset + 28, true);
    const nameBytes = bytes.subarray(offset + 46, offset + 46 + nameLength);
    const local = view.getUint32(offset + 42, true);
    const size = view.getUint32(offset + 24, true);
    const crc = view.getUint32(offset + 16, true);
    expect(view.getUint32(local, true)).toBe(0x04034b50);
    expect(view.getUint16(local + 6, true)).toBe(0x0800);
    expect(view.getUint16(offset + 8, true)).toBe(0x0800);
    expect(view.getUint16(local + 8, true)).toBe(0);
    expect(view.getUint16(offset + 10, true)).toBe(0);
    expect(view.getUint32(local + 14, true)).toBe(crc);
    expect(view.getUint32(local + 18, true)).toBe(size);
    expect(view.getUint32(local + 22, true)).toBe(size);
    expect(view.getUint32(offset + 20, true)).toBe(size);
    expect(view.getUint16(local + 26, true)).toBe(nameLength);
    expect(bytes.subarray(local + 30, local + 30 + nameLength)).toEqual(nameBytes);
    entries.push({
      name: decoder.decode(nameBytes),
      crc,
      bytes: bytes.slice(local + 30 + nameLength, local + 30 + nameLength + size),
    });
    offset +=
      46 + nameLength + view.getUint16(offset + 30, true) + view.getUint16(offset + 32, true);
  }
  expect(offset).toBe(end);
  return entries;
}

describe("ZIP archives", () => {
  it("writes interoperable UTF-8 Store headers, the standard CRC-32 vector and an empty entry", async () => {
    const entries = [
      { name: "Tài liệu 📄.pdf", bytes: encoder.encode("123456789") },
      { name: "empty.txt", bytes: new Uint8Array() },
    ];
    const bytes = createZipArchive(entries);
    expect(readZip(bytes)).toEqual([
      { name: entries[0]?.name, crc: 0xcbf43926, bytes: encoder.encode("123456789") },
      { name: "empty.txt", crc: 0, bytes: new Uint8Array() },
    ]);
    const blob = createZipBlob(entries);
    expect(blob.type).toBe("application/zip");
    expect(new Uint8Array(await blob.arrayBuffer())).toEqual(bytes);
    expect(createZipArchive(entries)).toEqual(bytes);
    expect(readZip(createZipArchive([]))).toEqual([]);
  });

  it("deduplicates names after flattening traversal, Windows paths and invalid basenames", () => {
    const names = [
      "../../doc.pdf",
      "/doc.pdf",
      "C:\\dir\\doc.pdf",
      "doc (2).pdf",
      "..",
      "/",
      "bad\0:name.pdf",
    ];
    const zip = createZipArchive(names.map((name) => ({ name, bytes: new Uint8Array() })));
    expect(readZip(zip).map((entry) => entry.name)).toEqual([
      "doc.pdf",
      "doc (2).pdf",
      "doc (3).pdf",
      "doc (2) (2).pdf",
      "document",
      "document (2)",
      "bad__name.pdf",
    ]);
  });

  it("counts ZIP overhead in the byte guard and rejects ZIP32 overflow before allocation", () => {
    const entry = { name: "a", bytes: encoder.encode("data") };
    const size = createZipArchive([entry]).length;
    expect(createZipArchive([entry], { maxTotalBytes: size })).toHaveLength(size);
    expect(() => createZipArchive([entry], { maxTotalBytes: size - 1 })).toThrow(
      expect.objectContaining({ code: "limit_exceeded" }),
    );
    expect(() => createZipArchive([], { maxTotalBytes: 21 })).toThrow(
      expect.objectContaining({ code: "limit_exceeded" }),
    );
    expect(() => createZipArchive(Array.from({ length: 0xffff }, () => entry))).toThrow(
      expect.objectContaining({ code: "limit_exceeded" }),
    );
    expect(() =>
      createZipArchive([{ name: "a".repeat(0x10000), bytes: new Uint8Array() }]),
    ).toThrow(expect.objectContaining({ code: "limit_exceeded" }));
    for (const limit of [0, -1, Number.NaN, Number.POSITIVE_INFINITY])
      expect(() => createZipArchive([], { maxTotalBytes: limit })).toThrow(
        expect.objectContaining({ code: "invalid_params" }),
      );
  });

  it("encodes optional UTC modification dates at DOS two-second resolution", () => {
    const bytes = createZipArchive([
      { name: "a", bytes: new Uint8Array(), mtime: new Date("2026-11-11T12:34:57Z") },
    ]);
    const view = new DataView(bytes.buffer);
    expect(view.getUint16(10, true)).toBe((12 << 11) | (34 << 5) | 28);
    expect(view.getUint16(12, true)).toBe((46 << 9) | (11 << 5) | 11);

    const beforeEpoch = new DataView(
      createZipArchive([
        { name: "a", bytes: new Uint8Array(), mtime: new Date("1979-12-31T23:59:59Z") },
      ]).buffer,
    );
    expect(beforeEpoch.getUint16(10, true)).toBe(0);
    expect(beforeEpoch.getUint16(12, true)).toBe((1 << 5) | 1);

    const afterMaximum = new DataView(
      createZipArchive([
        { name: "a", bytes: new Uint8Array(), mtime: new Date("2108-02-29T12:00:00Z") },
      ]).buffer,
    );
    expect(afterMaximum.getUint16(10, true)).toBe((23 << 11) | (59 << 5) | 29);
    expect(afterMaximum.getUint16(12, true)).toBe((127 << 9) | (12 << 5) | 31);
  });
});

interface TestWorker {
  store: MemoryStore;
  terminated: boolean;
}

const clients: ConverterClient[] = [];
afterEach(() => {
  for (const client of clients.splice(0)) client.terminate();
  vi.unstubAllGlobals();
});

function harness(options: BatchOptions = {}, beforeRun?: (name: string) => Promise<void>) {
  const events: string[] = [];
  const workers: TestWorker[] = [];
  const client = new ConverterClient(() => {
    const state = { store: new MemoryStore(), terminated: false };
    workers.push(state);
    const context = Promise.resolve({
      session: new ConverterSession(loadCore(), state.store),
      storageSession: `batch-${workers.length}`,
      version: "test",
      memoryLimitBytes: 64 * 1024 * 1024,
      resources: () => [],
    });
    const target = new EventTarget();
    return Object.assign(target, {
      postMessage(request: RequestEnvelope) {
        const name = state.store.listInputs()[0]?.name ?? "";
        if (request.type === "ingestMany") {
          expect(state.store.listInputs()).toHaveLength(0);
          expect(state.store.outputLocation()).toBeNull();
          events.push(`ingest:${request.files[0]?.name}`);
        }
        if (request.type === "runTool") events.push(`run:${name}`);
        if (request.type === "reset") events.push("reset");
        void (async () => {
          let response: WorkerResponse;
          try {
            if (request.type === "runTool") await beforeRun?.(name);
            if (state.terminated) return;
            response = await handleRequest(context, request);
          } catch (error) {
            response = {
              id: request.id,
              ok: false,
              error: { code: "test_failure", message: String(error) },
            };
          }
          if (!state.terminated)
            target.dispatchEvent(new MessageEvent("message", { data: response }));
        })();
      },
      terminate() {
        state.terminated = true;
      },
    }) as unknown as Worker;
  });
  clients.push(client);
  return { client, runner: new BatchRunner(client, options), events, workers };
}

function deferred() {
  let resolve = () => {};
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("BatchRunner", () => {
  it("runs twenty real WASM jobs sequentially and clears worker inputs and outputs between items", async () => {
    const { runner, events, workers } = harness();
    const progress: number[] = [];
    const unsubscribe = runner.onProgress((state) => {
      progress.push(state.completed);
    });
    const files = Array.from({ length: 20 }, (_, index) => file(`${index}.pdf`));
    const result = await runner.run("pdf.info", {}, files);
    unsubscribe();
    expect(result.completed).toBe(20);
    expect(result.total).toBe(20);
    expect(result.currentItemId).toBeNull();
    expect(result.items.every((item) => item.status === "done")).toBe(true);
    expect(events).toEqual(
      files.flatMap((input) => [`ingest:${input.name}`, `run:${input.name}`, "reset"]),
    );
    expect(progress[0]).toBe(0);
    expect(progress.at(-1)).toBe(20);
    expect(
      workers.every(
        (worker) =>
          worker.store.listInputs().length === 0 && worker.store.outputLocation() === null,
      ),
    ).toBe(true);
    for (const item of result.items) expect(item.result?.data?.page_count).toBeGreaterThan(0);
  });

  it("rejects large files before ingest and enforces aggregate retained output bytes before the next input", async () => {
    const guarded = harness({ perFileBudgetBytes: fixture.length - 1 });
    const result = await guarded.runner.run("pdf.info", {}, [file("large.pdf")]);
    expect(result.items[0]).toMatchObject({ status: "failed", error: { code: "limit_exceeded" } });
    expect(guarded.events).toEqual(["reset"]);
    const probe = harness();
    const sample = await probe.runner.run("pdf.info", {}, [file("same.pdf")]);
    const size = sample.items[0]?.result?.artifacts[0]?.size as number;
    const bounded = harness({ batchOutputBudgetBytes: size });
    const output = await bounded.runner.run("pdf.info", {}, [file("same.pdf"), file("same.pdf")]);
    expect(output.items.map((item) => item.status)).toEqual(["done", "failed"]);
    expect(output.items[1]?.error?.code).toBe("limit_exceeded");
    expect(bounded.events.filter((event) => event.startsWith("ingest:"))).toHaveLength(1);
  });

  it("checks actual artifact sizes, discards an oversized result and still cleans its input", async () => {
    const { client, runner, workers } = harness({ batchOutputBudgetBytes: 1 });
    const runTool = client.runTool.bind(client);
    vi.spyOn(client, "runTool").mockImplementation(async (...args) => {
      const result = await runTool(...args);
      for (const artifact of result.artifacts) artifact.size = 0;
      return result;
    });
    const result = await runner.run("pdf.info", {}, [file("document.pdf")]);
    expect(result.items[0]).toMatchObject({ status: "failed", error: { code: "limit_exceeded" } });
    expect(result.items[0]?.result).toBeUndefined();
    expect(workers[0]?.store.listInputs()).toHaveLength(0);
  });

  it("continues after partial failure and retries only failed items", async () => {
    const calls: string[] = [];
    let fail = true;
    const { runner } = harness({}, async (name) => {
      calls.push(name);
      if (name === "bad.pdf" && fail) {
        fail = false;
        throw new Error("One failed item");
      }
    });
    const result = await runner.run("pdf.info", {}, [
      file("good.pdf"),
      file("bad.pdf"),
      file("last.pdf"),
    ]);
    expect(result.items.map((item) => item.status)).toEqual(["done", "failed", "done"]);
    const retried = await runner.retryFailed();
    expect(retried.items.every((item) => item.status === "done")).toBe(true);
    expect(calls).toEqual(["good.pdf", "bad.pdf", "last.pdf", "bad.pdf"]);
    expect(retried.items[0]?.id).toBe(result.items[0]?.id);
  });

  it("cancels a running worker and all queued items, retains done results and retries cancelled items", async () => {
    const entered = deferred();
    const release = deferred();
    let pause = true;
    const { runner, workers } = harness({}, async (name) => {
      if (name === "second.pdf" && pause) {
        pause = false;
        entered.resolve();
        await release.promise;
      }
    });
    const running = runner.run("pdf.info", {}, [
      file("first.pdf"),
      file("second.pdf"),
      file("third.pdf"),
    ]);
    await entered.promise;
    await expect(runner.retryFailed()).rejects.toMatchObject({ code: "batch_running" });
    await expect(runner.run("pdf.info", {}, [])).rejects.toMatchObject({ code: "batch_running" });
    runner.cancelBatch();
    release.resolve();
    const result = await running;
    expect(result.items.map((item) => item.status)).toEqual(["done", "cancelled", "cancelled"]);
    expect(workers[0]?.terminated).toBe(true);
    expect(
      readZip(new Uint8Array(await (await runner.packageBatchZip()).arrayBuffer())),
    ).toHaveLength(1);
    expect((await runner.retryFailed()).items.every((item) => item.status === "done")).toBe(true);
  });

  it("cancels individual queued and running items without cancelling their neighbours", async () => {
    const entered = deferred();
    const release = deferred();
    const { runner } = harness({}, async (name) => {
      if (name === "first.pdf") {
        entered.resolve();
        await release.promise;
      }
    });
    const running = runner.run("pdf.info", {}, [
      file("first.pdf"),
      file("skip.pdf"),
      file("last.pdf"),
    ]);
    await entered.promise;
    runner.cancelItem(runner.items[1]?.id as string);
    runner.cancelItem(runner.items[0]?.id as string);
    release.resolve();
    expect((await running).items.map((item) => item.status)).toEqual([
      "cancelled",
      "cancelled",
      "done",
    ]);
    expect((await runner.retryFailed()).items.every((item) => item.status === "done")).toBe(true);
  });

  it("packages every split artifact, preserves bytes after reset and deduplicates repeated output names", async () => {
    const { runner } = harness();
    const result = await runner.run("pdf.split", { mode: "every", every: 1 }, [
      file("same.pdf"),
      file("same.pdf"),
    ]);
    expect(result.items.every((item) => item.status === "done")).toBe(true);
    const artifacts = result.items.flatMap((item) => item.result?.artifacts ?? []);
    expect(artifacts.length).toBeGreaterThan(2);
    const zip = await runner.packageBatchZip("results");
    expect(zip.name).toBe("results.zip");
    expect(zip.type).toBe("application/zip");
    const entries = readZip(new Uint8Array(await zip.arrayBuffer()));
    expect(new Set(entries.map((entry) => entry.name)).size).toBe(artifacts.length);
    for (const [index, artifact] of artifacts.entries())
      expect(entries[index]?.bytes).toEqual(
        new Uint8Array(await (await openOutput(artifact.location)).arrayBuffer()),
      );
  });

  it("detaches OPFS file snapshots before reset deletes their backing files", async () => {
    const { client, runner } = harness();
    const outputs = new Map<string, Blob>();
    vi.stubGlobal("navigator", {
      storage: {
        getDirectory: async () => ({
          getFileHandle: async (name: string) => ({
            getFile: async () => {
              const blob = outputs.get(name);
              if (!blob) throw new Error("Deleted output");
              return Object.assign(blob.slice(), {
                arrayBuffer: async () => {
                  if (!outputs.has(name)) throw new Error("Deleted snapshot");
                  return blob.arrayBuffer();
                },
              });
            },
          }),
        }),
      },
    });
    const run = client.runTool.bind(client);
    vi.spyOn(client, "runTool").mockImplementation(async (...args) => {
      const result = await run(...args);
      for (const artifact of result.artifacts) {
        outputs.set(artifact.name, await openOutput(artifact.location));
        artifact.location = { kind: "opfs", directory: [], name: artifact.name };
      }
      return result;
    });
    const reset = client.reset.bind(client);
    vi.spyOn(client, "reset").mockImplementation(async () => {
      outputs.clear();
      return reset();
    });
    const result = await runner.run("pdf.info", {}, [file("document.pdf")]);
    expect(result.items[0]?.status).toBe("done");
    expect(outputs.size).toBe(0);
    expect(
      readZip(new Uint8Array(await (await runner.packageBatchZip()).arrayBuffer())),
    ).toHaveLength(1);
  });

  it("retires a worker when reset fails and isolates throwing progress callbacks", async () => {
    const { client, runner, workers } = harness();
    vi.spyOn(client, "reset").mockRejectedValueOnce({
      code: "storage_error",
      message: "Cannot clear",
    });
    runner.onProgress(() => {
      throw new Error("View failed");
    });
    const result = await runner.run("pdf.info", {}, [file("first.pdf"), file("second.pdf")]);
    expect(result.items.map((item) => item.status)).toEqual(["failed", "done"]);
    expect(result.items[0]?.error?.code).toBe("storage_error");
    expect(workers[0]?.terminated).toBe(true);
  });

  it("validates budgets and single-input tools without starting worker jobs", async () => {
    const { client, runner, events } = harness();
    for (const value of [0, -1, Number.NaN, Number.POSITIVE_INFINITY])
      expect(() => new BatchRunner(client, { batchOutputBudgetBytes: value })).toThrow(
        expect.objectContaining({ code: "invalid_params" }),
      );
    await expect(runner.run("missing.tool", {}, [])).rejects.toMatchObject({
      code: "unknown_tool",
    });
    await expect(runner.run("pdf.merge", {}, [file("a.pdf")])).rejects.toMatchObject({
      code: "wrong_input",
    });
    expect(events).toEqual([]);
    expect(await runner.run("pdf.info", {}, [])).toMatchObject({
      completed: 0,
      total: 0,
      items: [],
    });
  });
});

function shellArgs(command: string): string[] {
  return execFileSync("sh", ["-c", `ashift() { printf '%s\\0' "$@"; }; ${command}`])
    .toString()
    .split("\0")
    .slice(0, -1);
}

describe("batch and recipe handoff", () => {
  const names = ["a'b.pdf", "$(printf hacked).pdf", "line\nfile.pdf", "-flag.pdf"];

  it("quotes all batch arguments and uses supported CLI parameter flags", () => {
    const params = { angle: 90, pages: "all'\n$()", extra: { enabled: true } };
    const result = batchHandoff(tool("pdf.merge"), params, names, {
      locale: "vi",
      outputDir: "-out'",
    });
    expect(result.executionMode).toBe("browser");
    expect(result.installUrl).toBe("/vi/download");
    expect(result.deepLinkUrl).toBe("ariadshift://tool/pdf.merge");
    expect(shellArgs(result.cliCommand)).toEqual([
      "tool",
      "pdf.merge",
      ...names.map((name) => (name.startsWith("-") ? `./${name}` : name)),
      "-o",
      "./-out'",
      ...Object.entries(params).flatMap(([key, value]) => [
        "--param",
        `${key}=${JSON.stringify(value)}`,
      ]),
    ]);

    const batch = batchHandoff(tool("pdf.rotate"), params, names, { outputDir: "-out'" });
    expect(shellArgs(batch.cliCommand)).toEqual([
      "recipe",
      "run",
      "recipe.json",
      ...names.map((name) => (name.startsWith("-") ? `./${name}` : name)),
      "-o",
      "./-out'",
    ]);
    expect(JSON.parse(batch.recipeJson).steps).toEqual([{ tool: "pdf.rotate", params }]);
  });

  it("redacts every known secret, nested arrays and descriptor-specific fields without mutating the recipe", () => {
    const secretFields = [
      "password",
      "user_password",
      "owner_password",
      "passphrase",
      "api_key",
      "token",
      "pin",
      "secret",
      "pkcs12_password",
      "key_password",
      "p12_password",
      "pfx_password",
      "token_pin",
    ];
    const params = Object.fromEntries(secretFields.map((key) => [key, `sensitive-${key}`]));
    const original = {
      version: "ariad-recipe/1",
      steps: [
        {
          tool: "pdf.encrypt",
          params: { ...params, algorithm: "aes-256", nested: [{ token: "nested-value" }] },
        },
      ],
    };
    const result = recipeHandoff(JSON.stringify(original), names);
    const redacted = JSON.parse(result.recipeJson);
    for (const key of secretFields) {
      expect(redacted.steps[0].params[key]).toEqual({ $secret: "prompt" });
      expect(result.recipeJson).not.toContain(`sensitive-${key}`);
    }
    expect(redacted.steps[0].params.nested[0].token).toEqual({ $secret: "prompt" });
    expect(result.recipeJson).not.toContain("nested-value");
    expect(redacted.steps[0].params.algorithm).toBe("aes-256");
    expect(original.steps[0]?.params).toMatchObject({ password: "sensitive-password" });
    const batch = batchHandoff(
      { ...tool("pdf.encrypt"), secret_params: ["custom_credential"] },
      { custom_credential: "custom-value", password: "password-value" },
      [names[0] as string],
    );
    expect(batch.recipeJson).not.toContain("custom-value");
    expect(batch.cliCommand).not.toContain("custom-value");
    expect(batch.cliCommand).not.toContain("password-value");
    expect(shellArgs(batch.cliCommand)).toContain("custom_credential=@custom_credential.txt");
    expect(JSON.parse(batch.recipeJson).steps[0].params.custom_credential).toEqual({
      $secret: "prompt",
    });
  });

  it("generates recipe and watch commands without bypassing imported-recipe confirmation", () => {
    const result = recipeHandoff(
      { version: "ariad-recipe/1", steps: [{ tool: "pdf.info" }] },
      names,
      { recipeName: "-recipe'$.json" },
    );
    expect(shellArgs(result.cliCommand)).toEqual([
      "recipe",
      "run",
      "./-recipe'$.json",
      ...names.map((name) => (name.startsWith("-") ? `./${name}` : name)),
      "-o",
      "out/",
    ]);
    const watch = batchHandoff(tool("pdf.encrypt"), { owner_password: "private" }, names, {
      watchDir: "-watch'\n$()",
      recipeName: "recipe'$.json",
    });
    expect(watch.executionMode).toBe("desktop");
    expect(shellArgs(watch.cliCommand)).toEqual([
      "recipe",
      "run",
      "recipe'$.json",
      "--watch",
      "./-watch'\n$()",
      "-o",
      "out/",
    ]);
    expect(watch.recipeJson).not.toContain("private");
    expect(watch.cliCommand).not.toContain("--yes");
    expect(() => batchHandoff(tool("pdf.info"), {}, ["bad\0name"])).toThrow();
    expect(() => recipeHandoff("[]", [])).toThrow();
  });

  it("redacts secret handles and case variants and never echoes malformed recipe content", () => {
    const recipe = {
      version: "ariad-recipe/1",
      steps: [
        {
          tool: "pdf.encrypt",
          params: {
            PASSWORD: "hidden-value",
            credential: { $secret: "sec_private_handle" },
          },
        },
      ],
    };
    const result = recipeHandoff(recipe, ["document.pdf"]);
    expect(JSON.parse(result.recipeJson).steps[0].params).toEqual({
      PASSWORD: { $secret: "prompt" },
      credential: { $secret: "prompt" },
    });
    expect(recipe.steps[0]?.params.PASSWORD).toBe("hidden-value");
    const custom = batchHandoff(
      { ...tool("pdf.encrypt"), secret_params: ["custom_credential"] },
      { CUSTOM_CREDENTIAL: "hidden-value" },
      ["document.pdf"],
    );
    expect(custom.recipeJson).not.toContain("hidden-value");
    expect(custom.cliCommand).not.toContain("hidden-value");
    expect(() => recipeHandoff('{"password":"hidden-value"', [])).toThrow(
      "The recipe JSON is invalid.",
    );
  });
});
