// @vitest-environment jsdom
import type { DownloadStreamTarget, Job, JobEvent, OutputFile } from "@ariadshift/sdk";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { AccountShell } from "@/components/account/account-shell";
import type { AccountUser, AuthApi } from "@/components/account/auth-api";
import { JOB_HISTORY_KEY, type JobHistoryEntry } from "@/components/account/job-history";
import type { JobListPage, JobsApi, ListedJob } from "@/components/account/jobs-api";
import { JobsPanel } from "@/components/account/jobs-panel";
import { STREAM_DOWNLOAD_THRESHOLD_BYTES } from "@/lib/cloud/cloud-job";
import { resolveCloudConfig } from "@/lib/cloud/config";

vi.mock("next/navigation", () => ({ usePathname: () => "/jobs" }));

const CONFIG = { apiUrl: "http://localhost:3902", authUrl: "http://localhost:3901" };
const USER: AccountUser = {
  id: "user-1",
  name: "Ada",
  email: "ada@example.com",
  twoFactorEnabled: false,
};

const minutes = (count: number) => new Date(Date.now() + count * 60_000).toISOString();

function job(id: string, overrides: Partial<Job> = {}): Job {
  return {
    id,
    status: "succeeded",
    upload_id: `upload-${id}`,
    from: "pdf",
    to: "docx",
    profile: "editable",
    pack: "docling",
    route: [],
    warnings: [],
    error: null,
    attempts: 1,
    created_at: minutes(-5),
    started_at: minutes(-4),
    finished_at: minutes(-3),
    expires_at: minutes(55),
    ...overrides,
  };
}

function entry(id: string, owner: string | null = "user-1"): JobHistoryEntry {
  return { id, filename: `${id}.pdf`, owner, createdAt: minutes(-5), expiresAt: minutes(55) };
}

const OUTPUT: OutputFile = {
  id: "output-1",
  filename: "report.docx",
  format: "docx",
  media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  size: 185_000,
  sha256: "00",
  url: "http://localhost:3903/blob/output-1",
  expires_at: minutes(5),
};

function apiError(status: number, code: string): Error {
  return Object.assign(new Error(code), { status, code });
}

type FakeJobs = { [K in keyof JobsApi]: ReturnType<typeof vi.fn> } & JobsApi;

function fakeJobs(jobs: Record<string, Job | (() => Job)>): FakeJobs {
  const read = (id: string) => {
    const value = jobs[id];
    if (!value) {
      throw apiError(404, "not_found");
    }
    return typeof value === "function" ? value() : value;
  };
  return {
    getJob: vi.fn(async (id: string) => read(id)),
    events: vi.fn(async function* () {}),
    outputs: vi.fn(async (id: string) => ({ job_id: id, outputs: [OUTPUT] })),
    download: vi.fn(async () => new Blob(["docx bytes"])),
    deleteJob: vi.fn(async () => undefined),
    createJob: vi.fn(async () => job("retried", { status: "queued", attempts: 0 })),
    // A deployment without `GET /v1/jobs`, unless a test lists jobs.
    listJobs: vi.fn(async (): Promise<JobListPage> => ({ supported: false })),
  } as unknown as FakeJobs;
}

function fakeAuth(user: AccountUser | null): AuthApi {
  return {
    getSession: vi.fn(async () => user),
    apiToken: vi.fn(async () => "jwt"),
  } as unknown as AuthApi;
}

function seed(...entries: JobHistoryEntry[]) {
  localStorage.setItem(JOB_HISTORY_KEY, JSON.stringify(entries));
}

function renderJobs(api: JobsApi, user: AccountUser | null = USER, pollIntervalMs?: number) {
  const createApi = vi.fn(() => api);
  render(
    <AccountShell config={CONFIG} auth={fakeAuth(user)}>
      <JobsPanel createApi={createApi} pollIntervalMs={pollIntervalMs} />
    </AccountShell>,
  );
  return createApi;
}

async function row(filename: string): Promise<HTMLElement> {
  return screen.findByRole("listitem", { name: filename });
}

beforeEach(() => {
  localStorage.clear();
  URL.createObjectURL = vi.fn(() => "blob:report");
  URL.revokeObjectURL = vi.fn();
  vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("jobs page", () => {
  it("does not list account jobs from the API when signed out", async () => {
    seed(entry("anonymous", null));
    const api = fakeJobs({ anonymous: job("anonymous") });
    renderJobs(api, null);
    expect(await row("anonymous.pdf")).toBeTruthy();
    expect(api.listJobs).not.toHaveBeenCalled();
  });

  it("lists only the signed-in account's jobs from this browser", async () => {
    seed(entry("mine"), entry("anonymous", null), entry("other", "user-2"));
    const createApi = renderJobs(fakeJobs({ mine: job("mine") }));
    expect(await row("mine.pdf")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "1 job" })).toBeTruthy();
    expect(screen.queryByText("anonymous.pdf")).toBeNull();
    expect(screen.queryByText("other.pdf")).toBeNull();
    expect(createApi).toHaveBeenLastCalledWith(true);
  });

  it("shows anonymous jobs without credentials when signed out", async () => {
    seed(entry("mine"), entry("anonymous", null));
    const createApi = renderJobs(fakeJobs({ anonymous: job("anonymous") }), null);
    expect(await row("anonymous.pdf")).toBeTruthy();
    expect(screen.queryByText("mine.pdf")).toBeNull();
    const signIn = screen.getAllByRole("link", { name: "Sign in" });
    expect(signIn.map((link) => link.getAttribute("href"))).toContain("/sign-in?next=%2Fjobs");
    expect(createApi).toHaveBeenLastCalledWith(false);
  });

  it("says how to start a job when the list is empty", async () => {
    renderJobs(fakeJobs({}));
    expect(await screen.findByText(/No cloud jobs yet/)).toBeTruthy();
    expect(screen.getByRole("link", { name: "converter" }).getAttribute("href")).toBe("/app");
  });

  it("shows a finished job's output, expiry and download", async () => {
    seed(entry("done"));
    const api = fakeJobs({ done: job("done") });
    renderJobs(api);
    const item = await row("done.pdf");
    expect(await within(item).findByText("Ready")).toBeTruthy();
    expect(within(item).getByText(/^Deletes automatically at .+\(in 5\d min\)\.$/)).toBeTruthy();
    expect(within(item).getByText(/→/)).toBeTruthy();

    const download = await within(item).findByRole("button", { name: "Download report.docx" });
    fireEvent.click(download);
    await waitFor(() =>
      expect(api.download).toHaveBeenCalledWith(
        OUTPUT,
        expect.objectContaining({ verifySha256: true }),
      ),
    );
    // Fresh presigned URLs are requested for the download itself.
    expect(api.outputs).toHaveBeenCalledTimes(2);
    expect(HTMLAnchorElement.prototype.click).toHaveBeenCalled();
  });

  it("streams a large output through downloadToStream without calling download()", async () => {
    seed(entry("done"));
    const largeOutput: OutputFile = {
      ...OUTPUT,
      size: STREAM_DOWNLOAD_THRESHOLD_BYTES + 1,
    };
    const api = fakeJobs({ done: job("done") });
    api.outputs.mockResolvedValue({ job_id: "done", outputs: [largeOutput] });
    api.downloadToStream = vi.fn(async (_out: OutputFile, target: DownloadStreamTarget) => {
      await target.write(new TextEncoder().encode("streamed docx"));
      await target.close();
    });

    renderJobs(api);
    const item = await row("done.pdf");
    const download = await within(item).findByRole("button", { name: "Download report.docx" });
    fireEvent.click(download);

    await waitFor(() =>
      expect(api.downloadToStream).toHaveBeenCalledWith(
        largeOutput,
        expect.anything(),
        expect.objectContaining({ verifySha256: true }),
      ),
    );
    expect(api.download).not.toHaveBeenCalled();
    expect(HTMLAnchorElement.prototype.click).toHaveBeenCalled();
  });

  it("deletes a job now and leaves a focused tombstone", async () => {
    seed(entry("done"));
    const api = fakeJobs({ done: job("done") });
    renderJobs(api);
    const item = await row("done.pdf");
    fireEvent.click(
      await within(item).findByRole("button", { name: "Delete now done.pdf from the server" }),
    );
    const tombstone = await within(item).findByText(
      "Deleted from localhost:3902. The input and output files are gone.",
    );
    expect(api.deleteJob).toHaveBeenCalledWith("done");
    await waitFor(() => expect(document.activeElement).toBe(tombstone));
    expect(within(item).getByText("Deleted")).toBeTruthy();
    expect(within(item).queryByRole("button", { name: /Delete now/ })).toBeNull();
    expect(within(item).queryByRole("button", { name: /Download/ })).toBeNull();
  });

  it("retries a failed job from its kept upload and focuses the new row", async () => {
    seed(entry("broken"));
    const failed = job("broken", {
      status: "failed",
      error: { code: "failed", message: "engine exited with status 1" },
    });
    const api = fakeJobs({
      broken: failed,
      retried: job("retried", { status: "queued", attempts: 0 }),
    });
    renderJobs(api);
    const item = await row("broken.pdf");
    expect(await within(item).findByText("Failed")).toBeTruthy();
    expect(within(item).getByText(/The conversion failed/)).toBeTruthy();
    expect(within(item).getByText("failed: engine exited with status 1")).toBeTruthy();

    fireEvent.click(within(item).getByRole("button", { name: "Retry broken.pdf" }));
    await waitFor(() =>
      expect(api.createJob).toHaveBeenCalledWith({
        uploadId: "upload-broken",
        to: "docx",
        profile: "editable",
      }),
    );
    const stored = JSON.parse(localStorage.getItem(JOB_HISTORY_KEY) ?? "[]") as JobHistoryEntry[];
    expect(stored.map((saved) => saved.id)).toContain("retried");
    expect(stored.find((saved) => saved.id === "retried")?.filename).toBe("broken.pdf");

    await waitFor(() =>
      expect(screen.getAllByRole("listitem", { name: "broken.pdf" })).toHaveLength(2),
    );
    await waitFor(() => expect(document.activeElement?.textContent).toContain("Queued"));
  });

  it("does not offer a retry for failures the document causes", async () => {
    seed(entry("big"));
    renderJobs(
      fakeJobs({
        big: job("big", { status: "failed", error: { code: "limit_exceeded", message: "pages" } }),
      }),
    );
    const item = await row("big.pdf");
    expect(await within(item).findByText(/more than your plan allows/)).toBeTruthy();
    expect(within(item).queryByRole("button", { name: /Retry/ })).toBeNull();
  });

  it("follows a running job over its event stream", async () => {
    seed(entry("live"));
    let finished = false;
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const api = fakeJobs({
      live: () => job("live", finished ? {} : { status: "queued", started_at: null }),
    });
    const event = (id: number, kind: string, data: Record<string, unknown>): JobEvent => ({
      id,
      job_id: "live",
      kind,
      data,
      created_at: minutes(0),
    });
    api.events.mockImplementation(async function* () {
      yield event(1, "queued", { pack: "docling" });
      yield event(2, "started", { attempt: 1 });
      yield event(3, "progress", { stage: "write" });
      await gate;
      finished = true;
      yield event(4, "succeeded", { outputs: 1, bytes: 185000, elapsed_ms: 900 });
    });
    renderJobs(api);
    const item = await row("live.pdf");
    expect(await within(item).findByText("Converting: write")).toBeTruthy();
    expect(within(item).getByText(/^· \d+:\d\d$/)).toBeTruthy();

    release();
    expect(await within(item).findByText("Ready")).toBeTruthy();
    expect(await screen.findByText("live.pdf is converted and ready to download.")).toBeTruthy();
  });

  it("polls when the event stream fails", async () => {
    seed(entry("flaky"));
    let calls = 0;
    const api = fakeJobs({
      flaky: () => {
        calls += 1;
        return job("flaky", calls < 2 ? { status: "running" } : {});
      },
    });
    api.events.mockImplementation(
      // biome-ignore lint/correctness/useYield: the stream fails before any event.
      async function* () {
        throw new TypeError("network error");
      },
    );
    renderJobs(api, USER, 20);
    const item = await row("flaky.pdf");
    expect(await within(item).findByText("Ready")).toBeTruthy();
    expect(api.getJob.mock.calls.length).toBeGreaterThanOrEqual(2);
  });

  it("offers to remove a job the server no longer knows", async () => {
    seed(entry("gone"), entry("kept"));
    renderJobs(fakeJobs({ kept: job("kept") }));
    const item = await row("gone.pdf");
    expect(await within(item).findByText("This job is no longer on the server.")).toBeTruthy();
    fireEvent.click(within(item).getByRole("button", { name: "Remove from this list (gone.pdf)" }));
    await waitFor(() => expect(screen.queryByText("gone.pdf")).toBeNull());
    const stored = JSON.parse(localStorage.getItem(JOB_HISTORY_KEY) ?? "[]") as JobHistoryEntry[];
    expect(stored.map((saved) => saved.id)).toEqual(["kept"]);
    expect(document.activeElement).toBe(screen.getByRole("heading", { name: "1 job" }));
  });

  it("shows an expired job without download or delete", async () => {
    seed(entry("old"));
    renderJobs(fakeJobs({ old: job("old", { status: "expired", expires_at: minutes(-1) }) }));
    const item = await row("old.pdf");
    expect(await within(item).findByText("Deleted (expired)")).toBeTruthy();
    expect(within(item).queryByRole("button", { name: /Delete now|Download/ })).toBeNull();
  });

  it("explains a refused download", async () => {
    seed(entry("done"));
    const api = fakeJobs({ done: job("done") });
    renderJobs(api);
    const item = await row("done.pdf");
    const download = await within(item).findByRole("button", { name: "Download report.docx" });
    api.outputs.mockRejectedValueOnce(apiError(410, "expired"));
    fireEvent.click(download);
    expect((await within(item).findByRole("alert")).textContent).toBe(
      "This file was already deleted.",
    );
  });
});

function listed(id: string, overrides: Partial<ListedJob> = {}): ListedJob {
  return { ...job(id), filename: `${id}.pdf`, ...overrides };
}

describe("jobs page with the account's job list", () => {
  it("lists the account's jobs from the API with nothing stored in this browser", async () => {
    const api = fakeJobs({});
    api.listJobs.mockResolvedValue({
      supported: true,
      jobs: [listed("server-done"), listed("server-failed", { status: "failed" })],
      nextCursor: null,
    });
    renderJobs(api);

    const done = await row("server-done.pdf");
    expect(await within(done).findByText("Ready")).toBeTruthy();
    expect(within(await row("server-failed.pdf")).getByText("Failed")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "2 jobs" })).toBeTruthy();
    expect(screen.getByText(/from any device/)).toBeTruthy();
    expect(api.listJobs).toHaveBeenCalledTimes(1);
    // The list already carries each job, so rows do not read them again.
    expect(api.getJob).not.toHaveBeenCalled();
    // Server rows cannot be removed locally: the list would bring them back.
    expect(screen.queryByRole("button", { name: /Remove from this list/ })).toBeNull();
    expect(localStorage.getItem(JOB_HISTORY_KEY)).toBeNull();
  });

  it("names a listed job from this browser's record when the API gives no file name", async () => {
    seed(entry("named"));
    const api = fakeJobs({});
    api.listJobs.mockResolvedValue({
      supported: true,
      jobs: [
        listed("named", { filename: undefined }),
        listed("abcdef1234", { filename: undefined }),
      ],
      nextCursor: null,
    });
    renderJobs(api);
    expect(await row("named.pdf")).toBeTruthy();
    expect(await row("Job abcdef12")).toBeTruthy();
  });

  it("falls back to this browser's jobs when the API cannot list jobs", async () => {
    seed(entry("local"));
    const api = fakeJobs({ local: job("local") });
    renderJobs(api);
    expect(await row("local.pdf")).toBeTruthy();
    expect(screen.getByText(/cannot list your account's jobs yet/)).toBeTruthy();
    expect(
      await screen.findByRole("button", { name: "Remove from this list (local.pdf)" }),
    ).toBeTruthy();
  });

  it("explains a failed list, keeps this browser's jobs and tries again", async () => {
    seed(entry("local"));
    const api = fakeJobs({ local: job("local") });
    api.listJobs
      .mockRejectedValueOnce(apiError(500, "internal"))
      .mockResolvedValueOnce({ supported: true, jobs: [listed("server")], nextCursor: null });
    renderJobs(api);
    expect(await screen.findByText(/Cannot load your jobs from localhost:3902/)).toBeTruthy();
    expect(await row("local.pdf")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await row("server.pdf")).toBeTruthy();
    expect(screen.queryByText("local.pdf")).toBeNull();
    expect(api.listJobs).toHaveBeenCalledTimes(2);
  });

  it("loads the next page on request", async () => {
    const api = fakeJobs({});
    api.listJobs
      .mockResolvedValueOnce({ supported: true, jobs: [listed("first")], nextCursor: "page-2" })
      .mockResolvedValueOnce({ supported: true, jobs: [listed("second")], nextCursor: null });
    renderJobs(api);
    expect(await row("first.pdf")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Show more jobs" }));
    expect(await row("second.pdf")).toBeTruthy();
    expect(api.listJobs).toHaveBeenLastCalledWith({ cursor: "page-2" });
    expect(screen.getByRole("heading", { name: "2 jobs" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Show more jobs" })).toBeNull();
  });

  it("shows a retried job at the top of the account's list", async () => {
    const api = fakeJobs({ retried: job("retried", { status: "queued", attempts: 0 }) });
    api.listJobs.mockResolvedValue({
      supported: true,
      jobs: [listed("broken", { status: "failed", error: { code: "failed", message: "exit 1" } })],
      nextCursor: null,
    });
    renderJobs(api);
    const item = await row("broken.pdf");
    fireEvent.click(await within(item).findByRole("button", { name: "Retry broken.pdf" }));
    await waitFor(() =>
      expect(screen.getAllByRole("listitem", { name: "broken.pdf" })).toHaveLength(2),
    );
    expect(screen.getAllByRole("listitem", { name: "broken.pdf" })[0]?.textContent).toContain(
      "Queued",
    );
  });

  it("preserves the API path prefix when the jobs page calls /v1/jobs", async () => {
    const requestedUrls: string[] = [];
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
      requestedUrls.push(url);
      return new Response(
        JSON.stringify({
          jobs: [listed("prefixed")],
          next_cursor: null,
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    });
    try {
      const config = resolveCloudConfig({
        apiUrl: "https://example.com/api",
        authUrl: "https://auth.example.com",
      });
      render(
        <AccountShell config={config} auth={fakeAuth(USER)}>
          <JobsPanel />
        </AccountShell>,
      );
      expect(await row("prefixed.pdf")).toBeTruthy();
      expect(requestedUrls).toContain("https://example.com/api/v1/jobs?limit=50");
    } finally {
      fetchSpy.mockRestore();
    }
  });

  it("shows the sign-in-again state and never calls /v1 anonymously when token refresh fails", async () => {
    const requestedUrls: string[] = [];
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
      requestedUrls.push(url);
      return new Response(JSON.stringify({ jobs: [], next_cursor: null }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    });
    try {
      const expiredAuth: AuthApi = {
        getSession: vi.fn(async () => USER),
        apiToken: vi.fn(async () => {
          throw Object.assign(new Error("unauthorized"), { status: 401, code: "unauthorized" });
        }),
      } as unknown as AuthApi;
      render(
        <AccountShell config={CONFIG} auth={expiredAuth}>
          <JobsPanel />
        </AccountShell>,
      );
      expect(await screen.findByText(/Your session has ended\. Sign in again\./)).toBeTruthy();
      expect(requestedUrls).toEqual([]);
    } finally {
      fetchSpy.mockRestore();
    }
  });
});
