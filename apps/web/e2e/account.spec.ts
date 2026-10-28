import { expect, type Page, type Route, test } from "@playwright/test";

// The account, API key and jobs pages against a mocked auth service and /v1.
// The cloud origins are compiled into the build, so the mocked tests need a
// build made with
//   NEXT_PUBLIC_ARIAD_AUTH_URL=http://localhost:3901
//   NEXT_PUBLIC_ARIAD_API_URL=http://localhost:3902
// and the same variables set when Playwright runs. A build without them is
// checked for the "cloud not configured" state instead. Nothing listens on
// those ports: every request is answered by page.route.

const AUTH = process.env.NEXT_PUBLIC_ARIAD_AUTH_URL?.replace(/\/+$/, "");
const API = process.env.NEXT_PUBLIC_ARIAD_API_URL?.replace(/\/+$/, "");
const CONFIGURED = Boolean(AUTH && API);

const USER = { id: "user-1", name: "Ada", email: "ada@example.com", twoFactorEnabled: false };

interface ApiKey {
  id: string;
  name: string;
  start: string;
  createdAt: string;
  expiresAt: string | null;
  enabled: boolean;
}

interface MockState {
  signedIn: boolean;
  name: string;
  keys: ApiKey[];
  jobs: Record<string, Record<string, unknown>>;
  deleted: string[];
  bearer: (string | undefined)[];
  /** Serve `GET /v1/jobs`; otherwise answer 405 like a deployment without the route. */
  listJobs: boolean;
}

function corsHeaders(route: Route): Record<string, string> {
  const origin = route.request().headers().origin ?? "http://localhost:3100";
  return {
    "access-control-allow-origin": origin,
    "access-control-allow-credentials": "true",
    "access-control-allow-methods": "GET, POST, DELETE, OPTIONS",
    "access-control-allow-headers": "content-type, authorization, x-api-key, last-event-id",
    "access-control-expose-headers": "set-auth-jwt",
    vary: "origin",
  };
}

async function json(route: Route, body: unknown, status = 200) {
  await route.fulfill({
    status,
    headers: { ...corsHeaders(route), "content-type": "application/json" },
    body: JSON.stringify(body),
  });
}

function minutesFromNow(count: number): string {
  return new Date(Date.now() + count * 60_000).toISOString();
}

function job(id: string, status: string): Record<string, unknown> {
  return {
    id,
    status,
    upload_id: `upload-${id}`,
    from: "markdown",
    to: "docx",
    profile: "editable",
    pack: "core",
    route: [],
    warnings: [],
    error: null,
    attempts: 1,
    created_at: minutesFromNow(-2),
    started_at: minutesFromNow(-2),
    finished_at: status === "succeeded" ? minutesFromNow(-1) : null,
    expires_at: minutesFromNow(58),
  };
}

const OUTPUT_BYTES = "converted document";

async function mockCloud(page: Page, state: MockState) {
  await page.route(`${AUTH}/**`, async (route) => {
    const request = route.request();
    if (request.method() === "OPTIONS") {
      await route.fulfill({ status: 204, headers: corsHeaders(route) });
      return;
    }
    const path = new URL(request.url()).pathname.replace(/^\/api\/auth/, "");
    const body = request.postDataJSON?.() as Record<string, unknown> | null;
    switch (`${request.method()} ${path}`) {
      case "GET /get-session":
        return json(
          route,
          state.signedIn
            ? { session: { id: "session-1" }, user: { ...USER, name: state.name } }
            : null,
        );
      case "POST /sign-in/email":
        if (body?.password !== "correct horse battery") {
          return json(route, { message: "Invalid email or password" }, 401);
        }
        state.signedIn = true;
        return json(route, { redirect: false, token: "session-token", user: USER });
      case "POST /sign-out":
        state.signedIn = false;
        return json(route, { success: true });
      case "POST /update-user":
        state.name = String(body?.name ?? state.name);
        return json(route, { status: true });
      case "GET /passkey/list-user-passkeys":
        return json(route, []);
      case "GET /api-key/list":
        return json(route, { apiKeys: state.keys, total: state.keys.length });
      case "POST /api-key/create": {
        const key: ApiKey = {
          id: `key-${state.keys.length + 1}`,
          name: String(body?.name),
          start: "ashift_zz",
          createdAt: new Date().toISOString(),
          expiresAt: minutesFromNow(90 * 24 * 60),
          enabled: true,
        };
        state.keys.push(key);
        return json(route, { ...key, key: "ashift_zzTOPSECRETVALUE" });
      }
      case "POST /api-key/delete":
        state.keys = state.keys.filter((key) => key.id !== body?.keyId);
        return json(route, { success: true });
      case "GET /token":
        return state.signedIn
          ? json(route, { token: "jwt-for-tests" })
          : json(route, { message: "unauthorized" }, 401);
      default:
        return json(route, { message: "not found" }, 404);
    }
  });

  await page.route(`${API}/**`, async (route) => {
    const request = route.request();
    if (request.method() === "OPTIONS") {
      await route.fulfill({ status: 204, headers: corsHeaders(route) });
      return;
    }
    const path = new URL(request.url()).pathname;
    state.bearer.push(request.headers().authorization);
    if (path === "/v1/jobs" && request.method() === "GET") {
      if (!state.listJobs) {
        await route.fulfill({ status: 405, headers: corsHeaders(route) });
        return;
      }
      return json(route, {
        jobs: Object.values(state.jobs).map((current) => ({
          ...current,
          filename: `${String(current.id)}.md`,
        })),
        next_cursor: null,
      });
    }
    const match = /^\/v1\/jobs\/([^/]+)(\/events|\/outputs)?$/.exec(path);
    if (path === "/v1/blobs/output-1") {
      await route.fulfill({
        status: 200,
        headers: { ...corsHeaders(route), "content-type": "application/octet-stream" },
        body: OUTPUT_BYTES,
      });
      return;
    }
    const id = match?.[1];
    const current = id ? state.jobs[id] : undefined;
    if (!id || !current) {
      return json(route, { error: { code: "not_found", message: "job not found" } }, 404);
    }
    if (request.method() === "DELETE" && !match?.[2]) {
      state.deleted.push(id);
      current.status = "expired";
      await route.fulfill({ status: 204, headers: corsHeaders(route) });
      return;
    }
    if (match?.[2] === "/events") {
      // The whole stream at once: the job finishes while the page follows it.
      current.status = "succeeded";
      const events = [
        ["started", { attempt: 1 }],
        ["progress", { stage: "write" }],
        ["succeeded", { outputs: 1, bytes: OUTPUT_BYTES.length, elapsed_ms: 900 }],
      ]
        .map(([kind, data], index) => {
          const event = { id: index + 1, job_id: id, kind, data, created_at: minutesFromNow(0) };
          return `event: ${kind}\nid: ${index + 1}\ndata: ${JSON.stringify(event)}\n\n`;
        })
        .join("");
      await route.fulfill({
        status: 200,
        headers: { ...corsHeaders(route), "content-type": "text/event-stream" },
        body: events,
      });
      return;
    }
    if (match?.[2] === "/outputs") {
      return json(route, {
        job_id: id,
        outputs: [
          {
            id: "output-1",
            filename: "notes.docx",
            format: "docx",
            media_type: "application/octet-stream",
            size: OUTPUT_BYTES.length,
            sha256: "0",
            url: `${API}/v1/blobs/output-1`,
            expires_at: minutesFromNow(5),
          },
        ],
      });
    }
    return json(route, current);
  });
}

function newState(overrides: Partial<MockState> = {}): MockState {
  return {
    signedIn: true,
    name: USER.name,
    keys: [],
    jobs: {},
    deleted: [],
    bearer: [],
    listJobs: false,
    ...overrides,
  };
}

test.describe("without cloud origins", () => {
  test.skip(CONFIGURED, "this build has cloud origins");

  test("the account pages say the cloud is not configured", async ({ page }) => {
    const external: string[] = [];
    page.on("request", (request) => {
      if (!request.url().startsWith("http://localhost:3100")) {
        external.push(request.url());
      }
    });
    for (const path of ["/account", "/api-keys", "/jobs", "/sign-in"]) {
      await page.goto(path);
      await expect(page.getByText("Cloud is not configured")).toBeVisible();
    }
    expect(external).toEqual([]);
  });
});

test.describe("with a mocked cloud", () => {
  test.skip(!CONFIGURED, "build with NEXT_PUBLIC_ARIAD_AUTH_URL and NEXT_PUBLIC_ARIAD_API_URL");

  test("signs in with the keyboard and returns to the requested page", async ({ page }) => {
    const state = newState({ signedIn: false });
    await mockCloud(page, state);
    await page.goto("/sign-in?next=%2Fapi-keys");
    await page.getByLabel("Email").fill("ada@example.com");
    await page.getByLabel("Password").fill("wrong password");
    await page.getByLabel("Password").press("Enter");
    await expect(page.locator("form").getByRole("alert")).toContainText(
      "Invalid email or password",
    );

    await page.getByLabel("Password").fill("correct horse battery");
    await page.getByLabel("Password").press("Enter");
    await expect(page).toHaveURL(/\/api-keys$/);
    await expect(page.getByRole("heading", { level: 1, name: "API keys" })).toBeVisible();
  });

  test("creates an API key shown once and revokes it", async ({ page }) => {
    const state = newState();
    await mockCloud(page, state);
    await page.goto("/api-keys");
    await expect(page.getByText("No API keys yet.")).toBeVisible();

    const create = page.getByRole("button", { name: "Create API key" });
    await create.focus();
    await page.keyboard.press("Enter");
    const form = page.getByRole("dialog", { name: "Create an API key" });
    await expect(form.getByLabel("Name")).toBeFocused();
    await page.keyboard.type("CI");
    await page.keyboard.press("Enter");

    const secret = page.getByRole("dialog", { name: "Your new API key" });
    await expect(secret.getByLabel("API key")).toHaveValue("ashift_zzTOPSECRETVALUE");
    await expect(secret.getByLabel("API key")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(create).toBeFocused();
    await expect(page.getByText("ashift_zzTOPSECRETVALUE")).toHaveCount(0);
    await expect(
      page.getByRole("region", { name: "Your keys" }).getByText("CI", { exact: true }),
    ).toBeVisible();

    await page.getByRole("button", { name: "Revoke API key CI" }).click();
    const confirm = page.getByRole("alertdialog", { name: "Revoke CI?" });
    await expect(confirm.getByRole("button", { name: "Cancel" })).toBeFocused();
    await confirm.getByRole("button", { name: "Revoke key" }).click();
    await expect(page.getByText("No API keys yet.")).toBeVisible();
    expect(state.keys).toEqual([]);
  });

  test("saves the profile name and signs out", async ({ page }) => {
    const state = newState();
    await mockCloud(page, state);
    await page.goto("/account");
    const profile = page.getByRole("region", { name: "Profile" });
    await profile.getByLabel("Name").fill("Ada Lovelace");
    await profile.getByRole("button", { name: "Save name" }).click();
    await expect(profile.getByText("Name saved.")).toBeVisible();
    expect(state.name).toBe("Ada Lovelace");

    await page.getByRole("button", { name: "Sign out" }).click();
    await expect(page).toHaveURL(/\/sign-in$/);
    expect(state.signedIn).toBe(false);
  });

  test("follows, downloads and deletes cloud jobs", async ({ page }) => {
    const state = newState({
      jobs: { "job-done": job("job-done", "succeeded"), "job-live": job("job-live", "running") },
    });
    await mockCloud(page, state);
    await page.addInitScript(
      (entries) => localStorage.setItem("ariadshift-cloud-jobs", JSON.stringify(entries)),
      ["job-done", "job-live"].map((id, index) => ({
        id,
        filename: `${id}.md`,
        owner: USER.id,
        createdAt: minutesFromNow(-2 - index),
        expiresAt: minutesFromNow(58),
      })),
    );
    await page.goto("/jobs");

    const live = page.getByRole("listitem", { name: "job-live.md" });
    await expect(live.getByText("Ready")).toBeVisible();

    const done = page.getByRole("listitem", { name: "job-done.md" });
    await expect(done.getByText(/Deletes automatically at .+\(in 5\d min\)/)).toBeVisible();
    const downloading = page.waitForEvent("download");
    await done.getByRole("button", { name: "Download notes.docx" }).click();
    const download = await downloading;
    expect(download.suggestedFilename()).toBe("notes.docx");

    await done.getByRole("button", { name: "Delete now job-done.md from the server" }).click();
    const tombstone = done.getByText(
      "Deleted from localhost:3902. The input and output files are gone.",
    );
    await expect(tombstone).toBeFocused();
    expect(state.deleted).toEqual(["job-done"]);
    expect(state.bearer.filter(Boolean).every((value) => value === "Bearer jwt-for-tests")).toBe(
      true,
    );
  });

  test("lists the account's jobs from the API in a fresh browser", async ({ page }) => {
    const state = newState({
      listJobs: true,
      jobs: { "job-done": job("job-done", "succeeded"), "job-live": job("job-live", "running") },
    });
    await mockCloud(page, state);
    // Nothing is seeded into localStorage: the API is the only source of the list.
    await page.goto("/jobs");

    await expect(page.getByRole("heading", { name: "2 jobs" })).toBeVisible();
    await expect(page.getByText(/from any device/)).toBeVisible();
    const live = page.getByRole("listitem", { name: "job-live.md" });
    await expect(live.getByText("Ready")).toBeVisible();
    const done = page.getByRole("listitem", { name: "job-done.md" });
    const downloading = page.waitForEvent("download");
    await done.getByRole("button", { name: "Download notes.docx" }).click();
    expect((await downloading).suggestedFilename()).toBe("notes.docx");
    await expect(page.getByRole("button", { name: /Remove from this list/ })).toHaveCount(0);
    expect(await page.evaluate(() => localStorage.getItem("ariadshift-cloud-jobs"))).toBeNull();
    expect(state.bearer.filter(Boolean).every((value) => value === "Bearer jwt-for-tests")).toBe(
      true,
    );
  });
});
