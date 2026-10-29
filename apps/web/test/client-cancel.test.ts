import { ConverterClient } from "@/converter/client";
import type { RequestEnvelope, WorkerResponse } from "@/converter/protocol";

class ControlledWorker extends EventTarget {
  static workers: ControlledWorker[] = [];
  readonly session = `session-${ControlledWorker.workers.length + 1}`;
  readonly received: RequestEnvelope[] = [];
  terminated = false;
  recycle = false;
  holdConvert = false;
  holdRetire = false;

  constructor() {
    super();
    ControlledWorker.workers.push(this);
  }

  postMessage(request: RequestEnvelope) {
    this.received.push(request);
    if (
      request.type === "plan" ||
      (request.type === "convert" && this.holdConvert) ||
      (request.type === "retire" && this.holdRetire)
    )
      return;
    const result =
      request.type === "init"
        ? { session: this.session }
        : request.type === "convert"
          ? { recycle: this.recycle }
          : {};
    const response = { id: request.id, ok: true, result } as WorkerResponse;
    queueMicrotask(() => this.dispatchEvent(new MessageEvent("message", { data: response })));
  }

  terminate() {
    this.terminated = true;
  }
}

describe("worker cancellation and replacement", () => {
  beforeEach(() => {
    ControlledWorker.workers = [];
  });

  it("rejects pending requests on cancel and retires the old session", async () => {
    const client = new ConverterClient(() => new ControlledWorker() as unknown as Worker);
    await client.request({ type: "init" });
    const worker = ControlledWorker.workers[0] as ControlledWorker;
    worker.holdConvert = true;
    const pending = client.request({ type: "convert", to: "html", profile: "editable" });
    const rejected = expect(pending).rejects.toMatchObject({ code: "cancelled", fatal: false });
    client.cancel();
    await rejected;
    expect(worker.terminated).toBe(true);
    await client.request({ type: "reset" });
    expect(ControlledWorker.workers[1]?.received).toContainEqual(
      expect.objectContaining({ type: "retire", sessions: [worker.session] }),
    );
  });

  it("retries cleanup when its replacement is cancelled before retirement completes", async () => {
    const client = new ConverterClient(() => {
      const worker = new ControlledWorker();
      worker.holdRetire = ControlledWorker.workers.length === 2;
      return worker as unknown as Worker;
    });
    await client.request({ type: "init" });
    client.cancel();
    await client.request({ type: "init" });
    client.cancel();
    await client.request({ type: "reset" });
    expect(ControlledWorker.workers[2]?.received).toContainEqual(
      expect.objectContaining({ type: "retire", sessions: ["session-1", "session-2"] }),
    );
  });

  it("rejects pending requests on recycle and retires the old session", async () => {
    const client = new ConverterClient(() => new ControlledWorker() as unknown as Worker);
    await client.request({ type: "init" });
    const worker = ControlledWorker.workers[0] as ControlledWorker;
    worker.recycle = true;
    await client.request({ type: "convert", to: "html", profile: "editable" });
    const pending = client.request({ type: "plan", to: "html", profile: "editable" });
    const rejected = expect(pending).rejects.toMatchObject({ code: "worker_replaced" });
    await client.request({ type: "ingest", file: new File(["x"], "x.md") });
    await rejected;
    expect(worker.terminated).toBe(true);
    expect(ControlledWorker.workers[1]?.received).toContainEqual(
      expect.objectContaining({ type: "retire", sessions: [worker.session] }),
    );
  }, 1000);
});
