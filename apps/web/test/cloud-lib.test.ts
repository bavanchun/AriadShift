import { AriadApiError, type Capabilities } from "@ariadshift/sdk";
import {
  type CloudApiOptions,
  cloudTargets,
  createCloudApi,
  findOperation,
  formatFromName,
  type PlanLimits,
} from "@/lib/cloud/api";
import { readCloudConfig } from "@/lib/cloud/config";
import { type CloudErrorContext, describeCloudError, describeJobFailure } from "@/lib/cloud/errors";
import {
  announcementFor,
  applyJobEvent,
  type CloudJobState,
  formatClock,
  formatElapsed,
  formatExpiry,
  formatRetention,
  INITIAL_JOB_STATE,
  stepLabel,
} from "@/lib/cloud/steps";

const LIMITS: PlanLimits = {
  plan: "anonymous",
  authenticated: false,
  retention_s: 3600,
  requests_per_minute: 30,
  limits: { max_input_bytes: 10 * 1024 * 1024, max_memory_mb: 1024, max_pages: 50, timeout_s: 120 },
  quotas: {
    uploads_per_day: { limit: 20, remaining: 18, used: 2 },
    jobs_per_day: { limit: 20, remaining: 18, used: 2 },
    input_bytes_per_day: { limit: 100 * 1024 * 1024, remaining: 90 * 1024 * 1024, used: 0 },
    pending_uploads: { limit: 1, remaining: 1, used: 0 },
    concurrent_jobs: { limit: 1, remaining: 1, used: 0 },
  },
};

const CONTEXT: CloudErrorContext = {
  fileName: "report.pdf",
  fileSize: 14.2 * 1024 * 1024,
  from: "pdf",
  to: "docx",
  limits: LIMITS,
  signInOffered: true,
  signedIn: false,
  serverExpiresAt: null,
};

const CAPABILITIES = {
  version: "ariad-capabilities/0",
  generated_at: "2026-10-25T11:00:00Z",
  packs: ["core", "office"],
  formats: [
    { id: "markdown", extensions: ["md", "markdown"], media_type: "text/markdown" },
    { id: "html", extensions: ["html", "htm"], media_type: "text/html" },
    { id: "docx", extensions: ["docx"], media_type: "application/vnd.openxmlformats" },
    { id: "pdf", extensions: ["pdf"], media_type: "application/pdf" },
  ],
  edges: [
    { from: "markdown", to: "ariad-ir+json", engine: "ariad-core", pack: "core", license: "MIT" },
    { from: "ariad-ir+json", to: "html", engine: "ariad-core", pack: "core", license: "MIT" },
    { from: "ariad-ir+json", to: "docx", engine: "pandoc", pack: "core", license: "GPL" },
    { from: "pdf", to: "ariad-ir+json", engine: "pdf-text", pack: "core", license: "MIT" },
  ],
} as unknown as Capabilities;

describe("readCloudConfig", () => {
  it("hides cloud mode without an API base URL", () => {
    expect(readCloudConfig({})).toBeNull();
    expect(readCloudConfig({ api: "  ", auth: "https://auth.example.com" })).toBeNull();
    expect(readCloudConfig({ api: "ftp://api.example.com" })).toBeNull();
    expect(readCloudConfig({ api: "not a url" })).toBeNull();
  });

  it("normalises the base URLs and names the API host", () => {
    expect(
      readCloudConfig({
        api: "https://api.example.com/",
        auth: "https://auth.example.com/api/auth",
        turnstileSiteKey: " 0x4AAA ",
        operator: "",
      }),
    ).toEqual({
      apiUrl: "https://api.example.com",
      apiHost: "api.example.com",
      authUrl: "https://auth.example.com",
      turnstileSiteKey: "0x4AAA",
      operator: null,
    });
    expect(readCloudConfig({ api: "http://localhost:8080" })?.authUrl).toBeNull();
  });
});

describe("feature detection", () => {
  const ok = (data: unknown, status = 200) => ({
    data,
    response: new Response(null, { status }),
  });

  it("finds only operations the generated SDK exports", () => {
    const operations = { getLimits: () => undefined, notAFunction: 1 };
    expect(findOperation("getLimits", operations)).toBeTypeOf("function");
    expect(findOperation("notAFunction", operations)).toBeUndefined();
    expect(findOperation("deleteUpload", operations)).toBeUndefined();
  });

  it("works without the limits and upload-delete endpoints", async () => {
    const api = createCloudApi({ baseUrl: "https://api.example.com", operations: {} });
    expect(api.supportsLimits).toBe(false);
    expect(api.supportsUploadDelete).toBe(false);
    await expect(api.limits()).resolves.toBeNull();
    await expect(api.deleteUpload("u1")).resolves.toBe(false);
  });

  it("calls the endpoints when the contract has them", async () => {
    const calls: unknown[] = [];
    const operations: CloudApiOptions["operations"] = {
      getLimits: async (options: unknown) => {
        calls.push(options);
        return ok(LIMITS);
      },
      deleteUpload: async (options: { path: { id: string } }) => {
        calls.push(options.path);
        return ok(undefined, 204);
      },
    };
    const api = createCloudApi({ baseUrl: "https://api.example.com", operations });
    expect(api.supportsLimits && api.supportsUploadDelete).toBe(true);
    await expect(api.limits()).resolves.toEqual(LIMITS);
    await expect(api.deleteUpload("u1")).resolves.toBe(true);
    expect(calls[1]).toEqual({ id: "u1" });
  });

  it("throws the SDK's error for a refused optional call", async () => {
    const api = createCloudApi({
      baseUrl: "https://api.example.com",
      operations: {
        deleteUpload: async () => ({
          error: { error: { code: "conflict", message: "a job uses the upload" } },
          response: new Response(null, { status: 409 }),
        }),
      },
    });
    const refused = api.deleteUpload("u1");
    await expect(refused).rejects.toBeInstanceOf(AriadApiError);
    await expect(refused).rejects.toMatchObject({ status: 409, code: "conflict" });
  });

  it("reports event-stream reconnections", async () => {
    const seen: string[] = [];
    const api = createCloudApi({
      baseUrl: "https://api.example.com",
      operations: {},
      fetch: async () => new Response(null, { status: 503 }),
    });
    api.onEventConnection((connection) => seen.push(`${connection.attempt}:${connection.state}`));
    const job = "5f0e8d3a-2c4b-4e71-8b6a-9d1c3e2f4a57";
    const events = api.client.events(job, { maxRetries: 1, retryDelayMs: 1 });
    await expect(events.next()).rejects.toMatchObject({ status: 503 });
    expect(seen).toEqual(["1:connecting", "1:failed", "2:connecting", "2:failed"]);
  });
});

describe("cloud targets", () => {
  it("follows edges through interchange formats and lists only documents", () => {
    expect(cloudTargets(CAPABILITIES, "markdown").sort()).toEqual(["docx", "html"]);
    expect(cloudTargets(CAPABILITIES, "pdf").sort()).toEqual(["docx", "html"]);
    expect(cloudTargets(CAPABILITIES, "docx")).toEqual([]);
  });

  it("detects a format by the longest matching extension", () => {
    expect(formatFromName(CAPABILITIES, "Report.PDF")).toBe("pdf");
    expect(formatFromName(CAPABILITIES, "notes.markdown")).toBe("markdown");
    expect(formatFromName(CAPABILITIES, "archive.zip")).toBeNull();
  });
});

describe("job steps", () => {
  const now = Date.parse("2026-10-25T12:00:00Z");
  const step = (state: CloudJobState, kind: string, data: unknown = {}, at = now) =>
    applyJobEvent(state, { kind, data }, at);

  it("maps each server signal to one label with elapsed time", () => {
    let state: CloudJobState = { ...INITIAL_JOB_STATE, phase: "uploading", sent: 45, total: 100 };
    expect(stepLabel(state, now)).toBe("Uploading 45%");
    state = step(state, "queued", { pack: "core" });
    expect(stepLabel(state, now + 5_000)).toBe("Queued · 0:05");
    state = step(state, "started", { attempt: 1 }, now + 6_000);
    state = step(state, "progress", { stage: "write" }, now + 8_000);
    expect(stepLabel(state, now + 22_000)).toBe("Converting: write · 0:16");
    state = step(state, "retrying", { code: "failed", message: "crash" });
    expect(stepLabel(state, now + 22_000)).toBe("Retrying (attempt 2) · 0:16");
    state = step(state, "warning", { code: "lossy", message: "Lost a footnote" });
    expect(state.warnings).toEqual([{ code: "lossy", message: "Lost a footnote" }]);
    expect(stepLabel(step(state, "succeeded"), now)).toBe("Ready");
    expect(step(state, "failed", { code: "input_io", message: "bad" }).failure).toEqual({
      code: "input_io",
      message: "bad",
    });
    expect(stepLabel(step(state, "expired", { reason: "retention" }), now)).toBe(
      "Deleted (expired)",
    );
    expect(step(state, "expired", { reason: "deleted" }).phase).toBe("deleted");
    expect(step(state, "something-new")).toBe(state);
  });

  it("says Reconnecting… instead of failing while a stream drops", () => {
    const state: CloudJobState = { ...INITIAL_JOB_STATE, phase: "converting", reconnecting: true };
    expect(stepLabel(state, now)).toBe("Reconnecting…");
    expect(step(state, "progress", { stage: "read" }).reconnecting).toBe(false);
  });

  it("announces step changes only, and failures assertively", () => {
    const uploading: CloudJobState = { ...INITIAL_JOB_STATE, phase: "uploading", total: 10 };
    expect(announcementFor(INITIAL_JOB_STATE, uploading, "report.pdf")).toEqual({
      polite: "Uploading report.pdf",
    });
    expect(announcementFor(uploading, { ...uploading, sent: 5 }, "report.pdf")).toBeNull();
    const failed = { ...uploading, phase: "failed" as const };
    expect(announcementFor(uploading, failed, "report.pdf")?.assertive).toContain("failed");
    const ready: CloudJobState = {
      ...INITIAL_JOB_STATE,
      phase: "succeeded",
      outputs: [{ id: "o1", filename: "report.docx" } as CloudJobState["outputs"][number]],
    };
    expect(announcementFor({ ...ready, outputs: [] }, ready, "report.pdf")).toEqual({
      polite: "report.docx is ready to download",
    });
  });

  it("formats elapsed time, expiry and retention", () => {
    expect(formatElapsed(16_400)).toBe("0:16");
    expect(formatElapsed(3_723_000)).toBe("1:02:03");
    const expiry = new Date(now + 58 * 60_000).toISOString();
    expect(formatExpiry(expiry, now)).toBe(
      `Deletes automatically at ${formatClock(expiry)} (in 58 min)`,
    );
    expect(formatExpiry(expiry, now + 59 * 60_000)).toBe(
      `Deleted automatically at ${formatClock(expiry)}`,
    );
    expect(formatRetention(3600)).toBe("1 hour");
    expect(formatRetention(1800)).toBe("30 minutes");
    expect(formatRetention(7200)).toBe("2 hours");
    expect(formatRetention(3 * 86_400)).toBe("3 days");
  });
});

describe("cloud error copy", () => {
  it("names the size limit and offers sign-in and on-device conversion", () => {
    const view = describeCloudError(new AriadApiError(413, "limit_exceeded", "too big"), CONTEXT);
    expect(view.message).toBe("This file is 14 MB. The anonymous plan accepts files up to 10 MB.");
    expect(view.actions).toEqual(["sign-in", "use-device"]);
    expect(view.technical).toBe("too big (limit_exceeded)");
  });

  it("explains quotas, rate limits and Turnstile in plain words", () => {
    expect(
      describeCloudError(new AriadApiError(429, "quota_exceeded", "x"), CONTEXT).message,
    ).toContain("today's 20 cloud conversions");
    const limited = describeCloudError(new AriadApiError(429, "rate_limited", "x", 7), CONTEXT);
    expect(limited.message).toBe("Too many requests. Try again in 7 s.");
    expect(limited.retryAfter).toBe(7);
    expect(
      describeCloudError(new AriadApiError(403, "turnstile_failed", "x"), CONTEXT).actions,
    ).toEqual(["retry"]);
    expect(
      describeCloudError(new AriadApiError(422, "unsupported_route", "x"), CONTEXT).message,
    ).toBe("PDF to Word (DOCX) is not available on this server.");
  });

  it("says whether the file is on the server after a failure", () => {
    const expiry = "2026-10-25T12:45:00Z";
    const offline = describeCloudError(new TypeError("Failed to fetch"), {
      ...CONTEXT,
      serverExpiresAt: expiry,
    });
    expect(offline.message).toBe(`Your file is safe on the server until ${formatClock(expiry)}.`);
    expect(describeCloudError(new TypeError("Failed to fetch"), CONTEXT).message).toBe(
      "Your file has not been uploaded.",
    );
    const failed = describeJobFailure(
      { code: "failed", message: "engine trap at /work/x" },
      { ...CONTEXT, serverExpiresAt: expiry },
    );
    expect(failed.message).toBe("The conversion failed: the engine stopped before it finished.");
    expect(failed.message).not.toContain("/work");
    expect(failed.serverCopy).toContain(formatClock(expiry));
    expect(failed.actions).toEqual(["retry", "delete"]);
  });

  it("asks a signed-in user to sign in again after a 401", () => {
    const view = describeCloudError(new AriadApiError(401, "unauthorized", "expired token"), {
      ...CONTEXT,
      signedIn: true,
    });
    expect(view.title).toBe("Your session has ended");
    expect(view.actions).toEqual(["sign-in"]);
  });
});
