import { createHash } from "node:crypto";
import { AriadApiError, type AriadClient, HASH_CHUNK_BYTES, sha256Hex } from "@ariadshift/sdk";
import type { CloudApi } from "@/lib/cloud/api";
import { CloudJob } from "@/lib/cloud/cloud-job";
import { type HashRequest, type HashResponse, hashFile } from "@/lib/cloud/hash";
import { INITIAL_JOB_STATE, stepLabel } from "@/lib/cloud/steps";

const ABC_SHA256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

function bytesOf(length: number): Uint8Array<ArrayBuffer> {
  const bytes = new Uint8Array(length);
  for (let index = 0; index < length; index += 1) {
    bytes[index] = (index * 31 + (index >>> 8)) & 0xff;
  }
  return bytes;
}

const nodeSha256 = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex");

type WorkerMode = "hash" | "fail-to-load" | "hang";

/** Stands in for the hash worker: answers with the SDK hash, or fails to load. */
class FakeWorker extends EventTarget {
  static mode: WorkerMode = "hash";
  static instances: FakeWorker[] = [];
  readonly url: string;
  readonly options: WorkerOptions | undefined;
  terminated = false;

  constructor(url: URL | string, options?: WorkerOptions) {
    super();
    this.url = String(url);
    this.options = options;
    FakeWorker.instances.push(this);
  }

  postMessage({ file }: HashRequest) {
    const reply = (data: HashResponse) => {
      if (!this.terminated) {
        this.dispatchEvent(new MessageEvent("message", { data }));
      }
    };
    queueMicrotask(() => {
      if (FakeWorker.mode === "fail-to-load") {
        this.dispatchEvent(new Event("error", { cancelable: true }));
        return;
      }
      if (FakeWorker.mode === "hang") {
        return;
      }
      void sha256Hex(file, {
        onProgress: (hashed, total) => reply({ kind: "progress", hashed, total }),
      }).then((sha256) => reply({ kind: "done", sha256 }));
    });
  }

  terminate() {
    this.terminated = true;
  }
}

afterEach(() => {
  vi.unstubAllGlobals();
  FakeWorker.mode = "hash";
  FakeWorker.instances = [];
});

describe("hashFile", () => {
  it("hashes on the main thread when workers are unavailable", async () => {
    expect(typeof Worker).toBe("undefined");
    const bytes = bytesOf(HASH_CHUNK_BYTES + 1);
    const progress: Array<[number, number]> = [];

    const digest = await hashFile(new Blob([bytes]), {
      onProgress: (hashed, total) => progress.push([hashed, total]),
    });

    expect(digest).toBe(nodeSha256(bytes));
    expect(progress).toEqual([
      [HASH_CHUNK_BYTES, bytes.length],
      [bytes.length, bytes.length],
    ]);
  });

  it("hashes in a module worker, forwards progress and stops the worker", async () => {
    vi.stubGlobal("Worker", FakeWorker);
    const bytes = bytesOf(HASH_CHUNK_BYTES * 2 + 7);
    const progress: number[] = [];

    const digest = await hashFile(new File([bytes], "big.pdf"), {
      onProgress: (hashed) => progress.push(hashed),
    });

    expect(digest).toBe(nodeSha256(bytes));
    expect(progress).toEqual([HASH_CHUNK_BYTES, HASH_CHUNK_BYTES * 2, bytes.length]);
    const [worker] = FakeWorker.instances;
    expect(FakeWorker.instances).toHaveLength(1);
    expect(worker?.url).toMatch(/hash-worker\.ts/);
    expect(worker?.options).toEqual({ type: "module", name: "ariadshift-hash" });
    expect(worker?.terminated).toBe(true);
  });

  it("falls back to the main thread when the worker fails to load", async () => {
    vi.stubGlobal("Worker", FakeWorker);
    FakeWorker.mode = "fail-to-load";

    expect(await hashFile(new Blob(["abc"]))).toBe(ABC_SHA256);
    expect(FakeWorker.instances[0]?.terminated).toBe(true);
  });

  it("stops the worker and rejects with the reason on abort", async () => {
    vi.stubGlobal("Worker", FakeWorker);
    FakeWorker.mode = "hang";
    const controller = new AbortController();
    const reason = new DOMException("canceled", "AbortError");

    const pending = hashFile(new Blob(["abc"]), { signal: controller.signal });
    controller.abort(reason);

    await expect(pending).rejects.toBe(reason);
    expect(FakeWorker.instances[0]?.terminated).toBe(true);
  });

  it("does not start when the signal already aborted", async () => {
    vi.stubGlobal("Worker", FakeWorker);
    const controller = new AbortController();
    controller.abort();

    await expect(hashFile(new Blob(["abc"]), { signal: controller.signal })).rejects.toMatchObject({
      name: "AbortError",
    });
    expect(FakeWorker.instances).toHaveLength(0);
  });
});

describe("hashing progress in a cloud job", () => {
  it("shows the hashed share while preparing and sends the digest", async () => {
    const bytes = bytesOf(HASH_CHUNK_BYTES * 2 + HASH_CHUNK_BYTES / 2);
    const file = new File([bytes], "scan.pdf", { type: "application/pdf" });
    const requests: unknown[] = [];
    const client = {
      createUpload: async (request: unknown) => {
        requests.push(request);
        throw new AriadApiError(429, "quota_exceeded", "the upload allowance is exhausted");
      },
    } as unknown as AriadClient;
    const api = {
      client,
      deleteUpload: async () => true,
      onEventConnection: () => () => undefined,
    } as unknown as Pick<CloudApi, "client" | "deleteUpload" | "onEventConnection">;
    const job = new CloudJob(file, {
      api,
      host: "api.example.com",
      context: () => ({
        fileName: file.name,
        fileSize: file.size,
        from: "pdf",
        to: "docx",
        limits: null,
        signInOffered: true,
        signedIn: false,
      }),
    });
    const labels: string[] = [];
    job.subscribe((state) => {
      if (state.phase === "preparing") {
        labels.push(stepLabel(state, 0));
      }
    });

    await job.start({ to: "docx" });

    expect(labels).toEqual(["Preparing…", "Preparing 40%", "Preparing 80%", "Preparing 100%"]);
    expect(requests).toEqual([
      { filename: "scan.pdf", size: file.size, sha256: nodeSha256(bytes) },
    ]);
    expect(job.state.phase).toBe("error");
  });

  it("labels preparing without a percentage before any bytes are hashed", () => {
    const state = { ...INITIAL_JOB_STATE, phase: "preparing" as const, total: 100 };
    expect(stepLabel(state, 0)).toBe("Preparing…");
    expect(stepLabel({ ...state, hashed: 45 }, 0)).toBe("Preparing 45%");
  });
});
