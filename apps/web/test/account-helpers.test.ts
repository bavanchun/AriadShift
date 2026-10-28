import type { Job } from "@ariadshift/sdk";
import { keyExpiry } from "@/components/account/api-keys-panel";
import {
  forgetJob,
  JOB_HISTORY_KEY,
  type JobHistoryEntry,
  MAX_JOB_HISTORY,
  readJobHistory,
  rememberJob,
} from "@/components/account/job-history";
import {
  apiErrorCopy,
  canRetry,
  expiryLine,
  formatElapsed,
  formatRelativeUntil,
  isActive,
  jobErrorCopy,
  jobStepLabel,
} from "@/components/account/job-status";
import { safeNextPath } from "@/components/account/sign-in-panel";
import { totpSecret } from "@/components/account/two-factor-section";
import { displayHost, resolveCloudConfig } from "@/lib/cloud/config";

const NOW = Date.parse("2026-10-25T12:00:00Z");

function job(overrides: Partial<Job> = {}): Job {
  return {
    id: "5f0e8d3a-2c4b-4e71-8b6a-9d1c3e2f4a57",
    status: "queued",
    upload_id: "0b9f2c4e-5d1a-4f3b-9a43-7c2e1d6f8a10",
    from: "markdown",
    to: "docx",
    profile: "editable",
    pack: "core",
    route: [],
    warnings: [],
    error: null,
    attempts: 0,
    created_at: "2026-10-25T11:30:00Z",
    started_at: null,
    finished_at: null,
    expires_at: "2026-10-25T12:58:00Z",
    ...overrides,
  };
}

class MemoryStorage {
  readonly items = new Map<string, string>();
  getItem(key: string) {
    return this.items.get(key) ?? null;
  }
  setItem(key: string, value: string) {
    this.items.set(key, value);
  }
}

function entry(id: string, createdAt: string, overrides: Partial<JobHistoryEntry> = {}) {
  return {
    id,
    filename: `${id}.pdf`,
    owner: null,
    createdAt,
    expiresAt: "2026-10-25T13:00:00Z",
    ...overrides,
  };
}

describe("resolveCloudConfig", () => {
  it("needs both origins", () => {
    expect(resolveCloudConfig({})).toBeNull();
    expect(resolveCloudConfig({ apiUrl: "https://api.example.com" })).toBeNull();
    expect(resolveCloudConfig({ authUrl: "https://auth.example.com" })).toBeNull();
  });

  it("keeps the API path prefix while keeping only the origin of the auth URL", () => {
    expect(
      resolveCloudConfig({
        apiUrl: " https://example.com/api/ ",
        authUrl: "http://localhost:3001/api/auth",
      }),
    ).toEqual({ apiUrl: "https://example.com/api", authUrl: "http://localhost:3001" });
  });

  it("rejects other schemes and garbage", () => {
    expect(
      resolveCloudConfig({ apiUrl: "ftp://api.example.com", authUrl: "https://a.example.com" }),
    ).toBeNull();
    expect(
      resolveCloudConfig({ apiUrl: "not a url", authUrl: "https://a.example.com" }),
    ).toBeNull();
  });

  it("shows the host with its port", () => {
    expect(displayHost("http://localhost:8080")).toBe("localhost:8080");
  });
});

describe("job step labels", () => {
  it("names each status once", () => {
    expect(jobStepLabel(job({ status: "queued" }), {}, NOW)).toBe("Queued");
    expect(jobStepLabel(job({ status: "running" }), {}, NOW)).toBe("Converting");
    expect(jobStepLabel(job({ status: "running" }), { stage: "write" }, NOW)).toBe(
      "Converting: write",
    );
    expect(jobStepLabel(job({ status: "running" }), { retryAttempt: 2 }, NOW)).toBe(
      "Retrying (attempt 2)",
    );
    expect(jobStepLabel(job({ status: "succeeded" }), {}, NOW)).toBe("Ready");
    expect(jobStepLabel(job({ status: "failed" }), {}, NOW)).toBe("Failed");
    expect(jobStepLabel(job({ status: "canceled" }), {}, NOW)).toBe("Canceled");
  });

  it("says when the page is polling instead of streaming", () => {
    expect(jobStepLabel(job({ status: "running" }), { reconnecting: true }, NOW)).toBe(
      "Converting · Reconnecting…",
    );
  });

  it("tells a deletion from a retention expiry", () => {
    expect(jobStepLabel(job({ status: "expired" }), {}, NOW)).toBe("Deleted");
    expect(
      jobStepLabel(job({ status: "expired", expires_at: "2026-10-25T11:59:00Z" }), {}, NOW),
    ).toBe("Deleted (expired)");
  });

  it("marks only queued and running as active", () => {
    expect(isActive("queued")).toBe(true);
    expect(isActive("running")).toBe(true);
    expect(isActive("succeeded")).toBe(false);
    expect(isActive("expired")).toBe(false);
  });
});

describe("time formatting", () => {
  it("formats elapsed time", () => {
    expect(formatElapsed(16_400)).toBe("0:16");
    expect(formatElapsed(725_000)).toBe("12:05");
    expect(formatElapsed(3_723_000)).toBe("1:02:03");
    expect(formatElapsed(-5)).toBe("0:00");
  });

  it("formats the time until expiry", () => {
    expect(formatRelativeUntil(30_000)).toBe("in less than a minute");
    expect(formatRelativeUntil(58 * 60_000)).toBe("in 58 min");
    expect(formatRelativeUntil(120 * 60_000)).toBe("in 2 h");
    expect(formatRelativeUntil(125 * 60_000)).toBe("in 2 h 5 min");
  });

  it("states the expiry of a kept, deleted and expired job", () => {
    expect(expiryLine(job(), NOW)).toMatch(/^Deletes automatically at .+ \(in 58 min\)\.$/);
    expect(expiryLine(job({ status: "expired" }), NOW)).toBe(
      "Deleted from the server. The input and output files are gone.",
    );
    expect(expiryLine(job({ status: "expired", expires_at: "2026-10-25T11:00:00Z" }), NOW)).toMatch(
      /^Deleted automatically at /,
    );
  });
});

describe("canRetry", () => {
  it("allows failed and canceled jobs whose input is kept", () => {
    expect(canRetry(job({ status: "failed", error: { code: "failed", message: "x" } }), NOW)).toBe(
      true,
    );
    expect(canRetry(job({ status: "canceled" }), NOW)).toBe(true);
  });

  it("refuses other statuses, expired inputs and document failures", () => {
    expect(canRetry(job({ status: "succeeded" }), NOW)).toBe(false);
    expect(canRetry(job({ status: "running" }), NOW)).toBe(false);
    expect(canRetry(job({ status: "failed", expires_at: "2026-10-25T11:00:00Z" }), NOW)).toBe(
      false,
    );
    for (const code of ["limit_exceeded", "unsupported_route", "input_io"]) {
      expect(canRetry(job({ status: "failed", error: { code, message: "x" } }), NOW)).toBe(false);
    }
  });
});

describe("error copy", () => {
  it("maps job error codes to plain language", () => {
    expect(jobErrorCopy("limit_exceeded")).toContain("more than your plan allows");
    expect(jobErrorCopy("unsupported_route")).toContain("cannot convert between these formats");
    expect(jobErrorCopy("storage")).toContain("could not store the result");
    expect(jobErrorCopy(undefined)).toContain("The conversion failed");
  });

  it("maps API errors, including network failures", () => {
    expect(apiErrorCopy({ status: 401, code: "unauthorized" })).toBe(
      "Your session has ended. Sign in again.",
    );
    expect(apiErrorCopy({ status: 410, code: "expired" })).toBe("This file was already deleted.");
    expect(apiErrorCopy({ status: 429, code: "rate_limited", retryAfter: 7 })).toBe(
      "Too many requests. Try again in 7 s.",
    );
    expect(apiErrorCopy(new TypeError("Failed to fetch"))).toContain("cannot be reached");
    expect(apiErrorCopy(new Error("downloaded 3 bytes, expected 4"))).toBe(
      "The transfer did not complete. Try again.",
    );
    expect(apiErrorCopy({ status: 500, code: "internal" })).toContain("could not complete");
  });
});

describe("job history", () => {
  it("records jobs newest first and replaces an entry with the same id", () => {
    const storage = new MemoryStorage();
    rememberJob(entry("a", "2026-10-25T10:00:00Z"), storage);
    rememberJob(entry("b", "2026-10-25T11:00:00Z"), storage);
    rememberJob(entry("a", "2026-10-25T10:00:00Z", { filename: "renamed.pdf" }), storage);
    const history = readJobHistory(storage, NOW);
    expect(history.map((item) => item.id)).toEqual(["b", "a"]);
    expect(history[1]?.filename).toBe("renamed.pdf");
  });

  it("forgets one job locally", () => {
    const storage = new MemoryStorage();
    rememberJob(entry("a", "2026-10-25T10:00:00Z"), storage);
    rememberJob(entry("b", "2026-10-25T11:00:00Z"), storage);
    forgetJob("b", storage);
    expect(readJobHistory(storage, NOW).map((item) => item.id)).toEqual(["a"]);
  });

  it("ignores malformed data and drops entries the API has pruned", () => {
    const storage = new MemoryStorage();
    storage.setItem(JOB_HISTORY_KEY, "{not json");
    expect(readJobHistory(storage, NOW)).toEqual([]);
    storage.setItem(
      JOB_HISTORY_KEY,
      JSON.stringify([
        { id: 1 },
        entry("old", "2026-09-01T10:00:00Z", { expiresAt: "2026-09-01T11:00:00Z" }),
        entry("kept", "2026-10-25T10:00:00Z", { owner: "user-1" }),
      ]),
    );
    expect(readJobHistory(storage, NOW).map((item) => item.id)).toEqual(["kept"]);
  });

  it("keeps at most the newest entries", () => {
    const storage = new MemoryStorage();
    for (let index = 0; index < MAX_JOB_HISTORY + 5; index += 1) {
      const created = new Date(Date.parse("2026-10-25T00:00:00Z") + index * 60_000).toISOString();
      rememberJob(entry(`job-${index}`, created), storage);
    }
    const history = readJobHistory(storage, NOW);
    expect(history).toHaveLength(MAX_JOB_HISTORY);
    expect(history[0]?.id).toBe(`job-${MAX_JOB_HISTORY + 4}`);
  });

  it("survives storage that throws", () => {
    const broken = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    expect(readJobHistory(broken, NOW)).toEqual([]);
    expect(() => rememberJob(entry("a", "2026-10-25T10:00:00Z"), broken)).not.toThrow();
    expect(readJobHistory(null, NOW)).toEqual([]);
  });
});

describe("sign-in helpers", () => {
  it("returns only to same-origin paths", () => {
    expect(safeNextPath("/jobs")).toBe("/jobs");
    expect(safeNextPath("/api-keys?x=1")).toBe("/api-keys?x=1");
    expect(safeNextPath("/jobs?status=running#top")).toBe("/jobs?status=running#top");
    expect(safeNextPath(null)).toBe("/account");
    expect(safeNextPath("https://evil.example")).toBe("/account");
    expect(safeNextPath("//evil.example")).toBe("/account");
    expect(safeNextPath("/\\evil.example")).toBe("/account");
    expect(safeNextPath("/\t/evil.example")).toBe("/account");
    expect(safeNextPath("/\n/evil.example")).toBe("/account");
    expect(safeNextPath("/\r/evil.example")).toBe("/account");
    expect(safeNextPath(new URLSearchParams("next=%2F%09%2Fevil.example").get("next"))).toBe(
      "/account",
    );
    expect(safeNextPath("/%09/evil.example")).toBe("/account");
    expect(safeNextPath("/%0a/evil.example")).toBe("/account");
    expect(safeNextPath("/%0d/evil.example")).toBe("/account");
    expect(safeNextPath("/%2f/evil.example")).toBe("/account");
    expect(safeNextPath("/%5cevil.example")).toBe("/account");
    expect(safeNextPath("/..//evil.example")).toBe("/account");
  });

  it("extracts the TOTP setup key in groups of four", () => {
    expect(
      totpSecret(
        "otpauth://totp/AriadShift:ada%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=AriadShift",
      ),
    ).toBe("JBSW Y3DP EHPK 3PXP");
    expect(totpSecret("otpauth://totp/x")).toBeNull();
    expect(totpSecret("not a uri")).toBeNull();
  });

  it("describes API key expiry", () => {
    const key = {
      id: "k",
      name: "ci",
      start: "ashift_ab",
      createdAt: "2026-10-01T00:00:00Z",
      enabled: true,
    };
    expect(keyExpiry({ ...key, expiresAt: null }, NOW)).toBe("Never expires");
    expect(keyExpiry({ ...key, expiresAt: "2026-12-01T00:00:00Z" }, NOW)).toMatch(/^Expires /);
    expect(keyExpiry({ ...key, expiresAt: "2026-10-02T00:00:00Z" }, NOW)).toMatch(/^Expired /);
  });
});
