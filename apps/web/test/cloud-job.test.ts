import {
  AriadApiError,
  type AriadClient,
  type Job,
  type JobEvent,
  type OutputFile,
  type Upload,
} from "@ariadshift/sdk";
import type { CloudApi, EventConnection } from "@/lib/cloud/api";
import { CloudJob, type CloudJobDependencies } from "@/lib/cloud/cloud-job";
import type { Transfer } from "@/lib/cloud/transfer";

const NOW = Date.parse("2026-10-25T11:00:00Z");
const FILE = new File(["# Report\n"], "report.md", { type: "text/markdown" });

const PENDING: Upload = {
  id: "0b9f2c4e-5d1a-4f3b-9a43-7c2e1d6f8a10",
  status: "pending",
  filename: "report.md",
  size: FILE.size,
  sha256: "0".repeat(64),
  format: null,
  created_at: "2026-10-25T11:00:00Z",
  expires_at: "2026-10-25T11:20:00Z",
  upload: {
    method: "PUT",
    url: "https://storage.example.com/t/abc",
    headers: { "x-amz-checksum-sha256": "AAAA" },
    expires_at: "2026-10-25T11:15:00Z",
  },
};
const COMPLETED: Upload = {
  ...PENDING,
  status: "completed",
  format: "markdown",
  upload: undefined,
  expires_at: "2026-10-25T12:00:00Z",
};
const JOB: Job = {
  id: "5f0e8d3a-2c4b-4e71-8b6a-9d1c3e2f4a57",
  status: "queued",
  upload_id: PENDING.id,
  from: "markdown",
  to: "docx",
  profile: "editable",
  pack: "core",
  route: [],
  warnings: [],
  error: null,
  attempts: 0,
  created_at: "2026-10-25T11:00:01Z",
  started_at: null,
  finished_at: null,
  expires_at: "2026-10-25T12:00:01Z",
} as Job;
const OUTPUT: OutputFile = {
  id: "c1a7e3f9-0b2d-4c6e-8f1a-3b5d7e9f2c48",
  filename: "report.docx",
  format: "docx",
  media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  size: 4,
  sha256: "f".repeat(64),
  url: "https://storage.example.com/o/1",
  expires_at: "2026-10-25T11:05:00Z",
};

function event(id: number, kind: string, data: unknown = {}): JobEvent {
  return { id, job_id: JOB.id, kind, data, created_at: "2026-10-25T11:00:02Z" } as JobEvent;
}

interface Fake {
  calls: string[];
  client: Partial<Record<keyof AriadClient, unknown>>;
  events: JobEvent[];
  final: Job;
  connections: Set<(connection: EventConnection) => void>;
  deleteUpload: (id: string) => Promise<boolean>;
}

function fake(): Fake {
  const state: Fake = {
    calls: [],
    events: [
      event(1, "queued", { pack: "core" }),
      event(2, "started", { attempt: 1 }),
      event(3, "progress", { stage: "write" }),
      event(4, "succeeded", { outputs: 1 }),
    ],
    final: { ...JOB, status: "succeeded" } as Job,
    connections: new Set(),
    deleteUpload: async () => true,
    client: {},
  };
  state.client = {
    createUpload: async (request: unknown, options: { turnstileToken?: string }) => {
      state.calls.push(`createUpload ${JSON.stringify(request)} ${options.turnstileToken ?? ""}`);
      return PENDING;
    },
    completeUpload: async (id: string) => {
      state.calls.push(`completeUpload ${id}`);
      return COMPLETED;
    },
    createJob: async (options: { uploadId: string; to: string; profile?: string }) => {
      state.calls.push(`createJob ${options.uploadId} ${options.to} ${options.profile}`);
      return JOB;
    },
    getJob: async () => {
      state.calls.push("getJob");
      return state.final;
    },
    events: async function* () {
      state.calls.push("events");
      yield* state.events;
    },
    outputs: async () => {
      state.calls.push("outputs");
      return { job_id: JOB.id, outputs: [OUTPUT] };
    },
    download: async (output: OutputFile, options: { verifySha256?: boolean }) => {
      state.calls.push(`download ${output.id} ${options.verifySha256}`);
      return new Blob(["docx"]);
    },
    deleteJob: async (id: string) => {
      state.calls.push(`deleteJob ${id}`);
    },
  };
  return state;
}

function controller(state: Fake, overrides: Partial<CloudJobDependencies> = {}) {
  const transfer: Transfer = async (file, presigned, options) => {
    state.calls.push(`transfer ${presigned.url} ${presigned.headers["x-amz-checksum-sha256"]}`);
    options?.onProgress?.(file.size / 2, file.size);
    options?.onProgress?.(file.size, file.size);
  };
  const api = {
    client: state.client as unknown as AriadClient,
    deleteUpload: async (id: string) => {
      state.calls.push(`deleteUpload ${id}`);
      return state.deleteUpload(id);
    },
    onEventConnection: (listener: (connection: EventConnection) => void) => {
      state.connections.add(listener);
      return () => state.connections.delete(listener);
    },
  } satisfies CloudJobDependencies["api"] as unknown as Pick<
    CloudApi,
    "client" | "deleteUpload" | "onEventConnection"
  >;
  const saved: string[] = [];
  const job = new CloudJob(FILE, {
    api,
    host: "api.example.com",
    transfer,
    save: (_blob, name) => saved.push(name),
    context: () => ({
      fileName: FILE.name,
      fileSize: FILE.size,
      from: "markdown",
      to: "docx",
      limits: null,
      signInOffered: true,
      signedIn: false,
    }),
    now: () => NOW,
    ...overrides,
  });
  const phases: string[] = [];
  job.subscribe((value) => {
    if (phases.at(-1) !== value.phase) {
      phases.push(value.phase);
    }
  });
  return { job, phases, saved };
}

describe("CloudJob", () => {
  it("uploads with the presigned request, converts and lists the outputs", async () => {
    const state = fake();
    const { job, phases } = controller(state);
    await job.start({ to: "docx", profile: "editable", turnstileToken: "token-1" });

    expect(phases).toEqual([
      "preparing",
      "uploading",
      "checking",
      "queued",
      "converting",
      "succeeded",
    ]);
    expect(state.calls[0]).toMatch(
      new RegExp(
        `^createUpload \\{"filename":"report.md","size":${FILE.size},"sha256":"[0-9a-f]{64}"\\} token-1$`,
      ),
    );
    expect(state.calls.slice(1)).toEqual([
      "transfer https://storage.example.com/t/abc AAAA",
      `completeUpload ${PENDING.id}`,
      `createJob ${PENDING.id} docx editable`,
      "events",
      "getJob",
      "outputs",
    ]);
    expect(job.state.outputs).toEqual([OUTPUT]);
    expect(job.state.expiresAt).toBe(JOB.expires_at);
    expect(job.state.stage).toBe("write");
  });

  it("downloads fresh URLs, then deletes after the download when asked", async () => {
    const state = fake();
    const { job, saved } = controller(state);
    await job.start({ to: "docx" });
    state.calls.length = 0;
    await job.download(OUTPUT, true);
    expect(saved).toEqual(["report.docx"]);
    expect(state.calls).toEqual(["outputs", `download ${OUTPUT.id} true`, `deleteJob ${JOB.id}`]);
    expect(job.state.phase).toBe("deleted");
    expect(job.state.notice).toBe(
      "Deleted from api.example.com after your download started. The input and output files are gone.",
    );
  });

  it("deletes now and keeps a tombstone", async () => {
    const state = fake();
    const { job } = controller(state);
    await job.start({ to: "docx" });
    await job.deleteNow();
    expect(state.calls.at(-1)).toBe(`deleteJob ${JOB.id}`);
    expect(job.state.phase).toBe("deleted");
    expect(job.state.notice).toBe(
      "Deleted from api.example.com. The input and output files are gone.",
    );
  });

  it("keeps the staged file after a refused upload and maps the error", async () => {
    const state = fake();
    state.client.createUpload = async () => {
      throw new AriadApiError(429, "quota_exceeded", "the plan's upload allowance is exhausted");
    };
    const { job } = controller(state);
    await job.start({ to: "docx" });
    expect(job.state.phase).toBe("error");
    expect(job.state.upload).toBeNull();
    expect(job.state.error?.title).toBe("Cloud allowance used up");
    expect(job.state.error?.actions).toEqual(["sign-in", "use-device"]);
    expect(job.retryNeedsUpload).toBe(true);
  });

  it("cancels an upload and deletes it when the server can", async () => {
    const state = fake();
    const { job } = controller(state, {
      transfer: (_file, _presigned, options) =>
        new Promise((_, reject) => {
          options?.signal?.addEventListener("abort", () =>
            reject(new DOMException("canceled", "AbortError")),
          );
        }),
    });
    const running = job.start({ to: "docx" });
    await vi.waitFor(() => expect(job.state.phase).toBe("uploading"));
    await job.cancel();
    await running;
    expect(state.calls).toContain(`deleteUpload ${PENDING.id}`);
    expect(job.state.phase).toBe("upload-canceled");
    expect(job.state.notice).toBe("Upload canceled. Nothing was kept.");
  });

  it("names the automatic deletion when the server cannot delete uploads", async () => {
    const state = fake();
    state.deleteUpload = async () => false;
    const { job } = controller(state, {
      transfer: (_file, _presigned, options) =>
        new Promise((_, reject) => {
          options?.signal?.addEventListener("abort", () =>
            reject(new DOMException("canceled", "AbortError")),
          );
        }),
    });
    const running = job.start({ to: "docx" });
    await vi.waitFor(() => expect(job.state.phase).toBe("uploading"));
    await job.cancel();
    await running;
    expect(job.state.notice).toMatch(
      /^Upload canceled\. Any partial data is deleted automatically at /,
    );
  });

  it("reports a failed job with plain copy and retries with the same upload", async () => {
    const state = fake();
    state.events = [event(1, "queued"), event(2, "failed", { code: "input_io", message: "hash" })];
    state.final = {
      ...JOB,
      status: "failed",
      error: { code: "input_io", message: "hash mismatch" },
    } as Job;
    const { job } = controller(state);
    await job.start({ to: "docx" });
    expect(job.state.phase).toBe("failed");
    expect(job.state.error?.message).toBe(
      "The conversion failed: the uploaded file could not be read.",
    );
    expect(job.state.error?.serverCopy).toMatch(/^The uploaded file is kept until /);

    state.calls.length = 0;
    state.events = [event(1, "succeeded")];
    state.final = { ...JOB, status: "succeeded" } as Job;
    await job.retry();
    expect(state.calls[0]).toBe(`createJob ${PENDING.id} docx undefined`);
    expect(job.state.phase).toBe("succeeded");
  });

  it("marks reconnections and resumes after the stream gives up", async () => {
    const state = fake();
    state.client.events = async function* () {
      yield event(1, "queued");
      for (const listener of state.connections) {
        listener({ jobId: JOB.id, attempt: 2, state: "connecting" });
      }
      throw new TypeError("network down");
    };
    state.final = { ...JOB, status: "running" } as Job;
    const { job, phases } = controller(state);
    await job.start({ to: "docx" });
    expect(phases).toContain("queued");
    expect(job.state.phase).toBe("error");
    expect(job.state.error?.message).toMatch(/^Your file is safe on the server until /);

    state.client.events = async function* () {
      yield event(2, "succeeded");
    };
    state.final = { ...JOB, status: "succeeded" } as Job;
    await job.retry();
    expect(job.state.phase).toBe("succeeded");
    expect(job.state.reconnecting).toBe(false);
  });

  it("treats a 410 from the outputs as already deleted", async () => {
    const state = fake();
    const { job } = controller(state);
    await job.start({ to: "docx" });
    state.client.outputs = async () => {
      throw new AriadApiError(410, "expired", "files deleted");
    };
    await job.download(OUTPUT, false);
    expect(job.state.phase).toBe("deleted");
    expect(job.state.notice).toBe("This file was already deleted from api.example.com.");
  });

  it("discards server copies on start over", async () => {
    const state = fake();
    const { job } = controller(state);
    await job.start({ to: "docx" });
    await expect(job.discard()).resolves.toEqual({ status: "deleted" });
    expect(state.calls.at(-1)).toBe(`deleteJob ${JOB.id}`);
    await expect(job.discard()).resolves.toEqual({ status: "none" });
  });

  it.each([
    ["offline", new TypeError("Failed to fetch")],
    ["refused with a 500", new AriadApiError(500, "internal", "storage unavailable")],
  ])("keeps a failed start-over deletion retryable when %s", async (_case, failure) => {
    const state = fake();
    const { job } = controller(state);
    await job.start({ to: "docx" });
    const deleteJob = state.client.deleteJob;
    state.client.deleteJob = async (id: string) => {
      state.calls.push(`deleteJob ${id}`);
      throw failure;
    };

    const outcome = await job.discard();
    expect(outcome).toMatchObject({
      status: "kept",
      id: JOB.id,
      fileName: "report.md",
      reason: "failed",
      expiresAt: JOB.expires_at,
    });
    // A failed deletion is not remembered as done: the panel's unmount tries again.
    await expect(job.discard()).resolves.toMatchObject({ status: "kept", reason: "failed" });
    expect(state.calls.filter((call) => call.startsWith("deleteJob"))).toHaveLength(2);

    state.client.deleteJob = deleteJob;
    if (outcome.status !== "kept") {
      throw new Error("expected a kept server copy");
    }
    await expect(outcome.retry()).resolves.toEqual({ status: "deleted" });
    await expect(job.discard()).resolves.toEqual({ status: "none" });
  });

  it("shares one deletion between start over and the panel's unmount", async () => {
    const state = fake();
    const { job } = controller(state);
    await job.start({ to: "docx" });
    const [first, second] = await Promise.all([job.discard(), job.discard()]);
    expect(first).toBe(second);
    expect(state.calls.filter((call) => call.startsWith("deleteJob"))).toHaveLength(1);
  });

  it("reports an upload the server cannot delete on request", async () => {
    const state = fake();
    state.deleteUpload = async () => false;
    state.client.createJob = async () => {
      throw new AriadApiError(422, "unsupported_route", "no route to docx");
    };
    const { job } = controller(state);
    await job.start({ to: "docx" });
    expect(job.state.upload?.status).toBe("completed");
    expect(job.state.job).toBeNull();

    await expect(job.discard()).resolves.toMatchObject({
      status: "kept",
      id: COMPLETED.id,
      reason: "unsupported",
      expiresAt: COMPLETED.expires_at,
    });
    expect(state.calls.at(-1)).toBe(`deleteUpload ${COMPLETED.id}`);
  });
});
