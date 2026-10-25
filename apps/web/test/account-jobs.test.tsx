// @vitest-environment jsdom
import type { Job, JobEvent, OutputFile } from "@ariadshift/sdk";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { AccountShell } from "@/components/account/account-shell";
import type { AccountUser, AuthApi } from "@/components/account/auth-api";
import { JOB_HISTORY_KEY, type JobHistoryEntry } from "@/components/account/job-history";
import type { JobsApi } from "@/components/account/jobs-api";
import { JobsPanel } from "@/components/account/jobs-panel";

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
    await waitFor(() => expect(api.download).toHaveBeenCalledWith(OUTPUT));
    // Fresh presigned URLs are requested for the download itself.
    expect(api.outputs).toHaveBeenCalledTimes(2);
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
