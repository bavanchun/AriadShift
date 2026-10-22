import { ConverterClient } from "@/converter/client";
import type { RequestEnvelope, WorkerResponse } from "@/converter/protocol";

/** A stand-in Worker whose replies the test controls. */
class FakeWorker extends EventTarget {
  static all: FakeWorker[] = [];
  readonly session = `session-${FakeWorker.all.length + 1}`;
  readonly received: RequestEnvelope[] = [];
  terminated = false;
  reply: (request: RequestEnvelope) => WorkerResponse | null = (request) => this.ok(request);

  constructor() {
    super();
    FakeWorker.all.push(this);
  }

  ok(request: RequestEnvelope, extra: object = {}): WorkerResponse {
    const result =
      request.type === "init" ? { session: this.session } : request.type === "reset" ? null : extra;
    return { id: request.id, ok: true, result } as WorkerResponse;
  }

  postMessage(request: RequestEnvelope) {
    this.received.push(request);
    const response = this.reply(request);
    if (response) {
      queueMicrotask(() => this.dispatchEvent(new MessageEvent("message", { data: response })));
    }
  }

  terminate() {
    this.terminated = true;
  }
}

function newClient() {
  return new ConverterClient(() => new FakeWorker() as unknown as Worker);
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("ConverterClient", () => {
  beforeEach(() => {
    FakeWorker.all = [];
  });

  it("learns the worker's session before the first request", async () => {
    const client = newClient();
    await client.request({ type: "reset" });
    const [worker] = FakeWorker.all;
    expect(worker?.received.map((request) => request.type)).toEqual(["init", "reset"]);
  });

  it("fails every pending request on a fatal error and cleans up after the worker", async () => {
    const client = newClient();
    await client.request({ type: "reset" });
    const first = FakeWorker.all[0] as FakeWorker;
    first.reply = (request) =>
      request.type === "convert"
        ? {
            id: request.id,
            ok: false,
            error: { code: "internal_error", message: "trap", fatal: true },
          }
        : null;

    const converting = client.request({ type: "convert", to: "html", profile: "editable" });
    const planning = client.request({ type: "plan", to: "html", profile: "editable" });
    await expect(converting).rejects.toMatchObject({ code: "internal_error" });
    await expect(planning).rejects.toMatchObject({ fatal: true });
    expect(first.terminated).toBe(true);

    await settle();
    await client.request({ type: "reset" });
    const second = FakeWorker.all[1] as FakeWorker;
    expect(second.received.map((request) => request.type)).toEqual(["init", "retire", "reset"]);
    expect(second.received[1]).toMatchObject({ type: "retire", sessions: ["session-1"] });
  });

  it("replaces a worker whose module grew large before the next file", async () => {
    const client = newClient();
    const first = (() => {
      const pending = client.request({ type: "reset" });
      return { pending, worker: FakeWorker.all[0] as FakeWorker };
    })();
    await first.pending;
    first.worker.reply = (request) =>
      request.type === "convert"
        ? first.worker.ok(request, { recycle: true })
        : first.worker.ok(request);
    await client.request({ type: "convert", to: "html", profile: "editable" });
    await client.request({ type: "plan", to: "html", profile: "editable" });
    expect(FakeWorker.all).toHaveLength(1);

    await settle();
    await client.request({ type: "ingest", file: new File(["x"], "x.md") });
    expect(first.worker.terminated).toBe(true);
    const second = FakeWorker.all[1] as FakeWorker;
    expect(second.received.map((request) => request.type)).toEqual(["init", "retire", "ingest"]);
  });
});
