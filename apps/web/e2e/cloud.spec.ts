// Cloud mode against a mocked API, auth service and object store. Cloud mode
// is configured at build time, so this spec runs only against a build made
// with NEXT_PUBLIC_ARIAD_API_URL set (any unresolvable origin works, for
// example https://api.cloud.test); every request to it is answered here.
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { expect, type Page, type Route, test } from "@playwright/test";

const API = process.env.NEXT_PUBLIC_ARIAD_API_URL?.replace(/\/+$/, "");
const AUTH = process.env.NEXT_PUBLIC_ARIAD_AUTH_URL?.replace(/\/+$/, "");
const SITE_KEY = process.env.NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY;
const STORAGE = "https://storage.cloud.test";

test.skip(!API, "needs a build with NEXT_PUBLIC_ARIAD_API_URL set");

const MARKDOWN = "# Cloud report\n\nConverted on the server.\n";
const OUTPUT = Buffer.from("PK fake docx bytes");
const UPLOAD_ID = "0b9f2c4e-5d1a-4f3b-9a43-7c2e1d6f8a10";
const JOB_ID = "5f0e8d3a-2c4b-4e71-8b6a-9d1c3e2f4a57";
const OUTPUT_ID = "c1a7e3f9-0b2d-4c6e-8f1a-3b5d7e9f2c48";

const CAPABILITIES = {
  version: "ariad-capabilities/0",
  generated_at: "2026-10-25T11:00:00Z",
  packs: ["core"],
  formats: [
    { id: "markdown", extensions: ["md", "markdown"], media_type: "text/markdown" },
    { id: "html", extensions: ["html", "htm"], media_type: "text/html" },
    { id: "docx", extensions: ["docx"], media_type: "application/octet-stream" },
  ],
  edges: [
    { from: "markdown", to: "ariad-ir+json", engine: "ariad-core", pack: "core", license: "MIT" },
    { from: "ariad-ir+json", to: "docx", engine: "pandoc", pack: "core", license: "GPL" },
    { from: "ariad-ir+json", to: "html", engine: "ariad-core", pack: "core", license: "MIT" },
  ],
};

const LIMITS = {
  plan: "anonymous",
  authenticated: false,
  retention_s: 3600,
  requests_per_minute: 30,
  limits: { max_input_bytes: 10 * 1024 * 1024, max_memory_mb: 1024, max_pages: 50, timeout_s: 120 },
  quotas: {
    uploads_per_day: { limit: 20, remaining: 19, used: 1 },
    jobs_per_day: { limit: 20, remaining: 19, used: 1 },
    input_bytes_per_day: { limit: 104857600, remaining: 104857000, used: 600 },
    pending_uploads: { limit: 1, remaining: 1, used: 0 },
    concurrent_jobs: { limit: 1, remaining: 1, used: 0 },
  },
};

const later = (minutes: number) => new Date(Date.now() + minutes * 60_000).toISOString();

interface Backend {
  requests: Array<{ method: string; url: string; headers: Record<string, string>; body: string }>;
  /** Answers `POST /v1/uploads` instead of the default; returns `false` to fall through. */
  createUpload?: (route: Route, origin: string) => Promise<boolean>;
}

/** The page origin, which CORS responses with credentials must echo. */
function originOf(route: Route): string {
  return route.request().headers().origin ?? "http://localhost:3100";
}

function cors(origin: string) {
  return {
    "access-control-allow-origin": origin,
    "access-control-allow-credentials": "true",
    "access-control-allow-methods": "GET, POST, PUT, DELETE, OPTIONS",
    "access-control-allow-headers":
      "authorization, content-type, cf-turnstile-response, last-event-id, x-amz-checksum-sha256",
    "access-control-expose-headers": "retry-after, set-auth-jwt, location, x-request-id",
  };
}

async function json(route: Route, origin: string, body: unknown, status = 200, extra = {}) {
  await route.fulfill({
    status,
    contentType: "application/json",
    headers: { ...cors(origin), ...extra },
    body: JSON.stringify(body),
  });
}

function sse(events: Array<[number, string, unknown]>): string {
  return events
    .map(
      ([id, kind, data]) =>
        `event: ${kind}\nid: ${id}\ndata: ${JSON.stringify({
          id,
          job_id: JOB_ID,
          kind,
          data,
          created_at: new Date().toISOString(),
        })}\n\n`,
    )
    .join("");
}

async function mockCloud(page: Page, signedIn = false): Promise<Backend> {
  const backend: Backend = { requests: [] };
  const job = (status: string) => ({
    id: JOB_ID,
    status,
    upload_id: UPLOAD_ID,
    from: "markdown",
    to: "docx",
    profile: "editable",
    pack: "core",
    route: [],
    warnings: [],
    error: null,
    attempts: 1,
    created_at: new Date().toISOString(),
    started_at: null,
    finished_at: null,
    expires_at: later(60),
  });
  const upload = (status: string) => ({
    id: UPLOAD_ID,
    status,
    filename: "report.md",
    size: Buffer.byteLength(MARKDOWN),
    sha256: createHash("sha256").update(MARKDOWN).digest("hex"),
    format: status === "completed" ? "markdown" : null,
    created_at: new Date().toISOString(),
    expires_at: later(status === "completed" ? 60 : 20),
    ...(status === "pending"
      ? {
          upload: {
            method: "PUT",
            url: `${STORAGE}/t/${UPLOAD_ID}`,
            headers: { "x-amz-checksum-sha256": "c2lnbmVk" },
            expires_at: later(15),
          },
        }
      : {}),
  });

  const record = async (route: Route) => {
    const request = route.request();
    backend.requests.push({
      method: request.method(),
      url: request.url(),
      headers: await request.allHeaders(),
      body: request.postData() ?? "",
    });
  };

  await page.route(`${API}/**`, async (route) => {
    const origin = originOf(route);
    const request = route.request();
    const { pathname } = new URL(request.url());
    const method = request.method();
    if (method === "OPTIONS") {
      await route.fulfill({ status: 204, headers: cors(origin) });
      return;
    }
    await record(route);
    if (method === "GET" && pathname === "/v1/capabilities") {
      return json(route, origin, CAPABILITIES);
    }
    if (method === "GET" && pathname === "/v1/limits") {
      return json(route, origin, LIMITS);
    }
    if (method === "POST" && pathname === "/v1/uploads") {
      if (await backend.createUpload?.(route, origin)) {
        return;
      }
      return json(route, origin, upload("pending"), 201);
    }
    if (method === "POST" && pathname === `/v1/uploads/${UPLOAD_ID}/complete`) {
      return json(route, origin, upload("completed"));
    }
    if (method === "DELETE" && pathname === `/v1/uploads/${UPLOAD_ID}`) {
      return route.fulfill({ status: 204, headers: cors(origin) });
    }
    if (method === "POST" && pathname === "/v1/jobs") {
      return json(route, origin, job("queued"), 202, { location: `/v1/jobs/${JOB_ID}` });
    }
    if (method === "GET" && pathname === "/v1/jobs") {
      return json(route, origin, { error: { code: "method_not_allowed" } }, 405);
    }
    if (method === "GET" && pathname === `/v1/jobs/${JOB_ID}/events`) {
      return route.fulfill({
        status: 200,
        contentType: "text/event-stream",
        headers: cors(origin),
        body: sse([
          [1, "queued", { pack: "core" }],
          [2, "started", { attempt: 1 }],
          [3, "progress", { stage: "write" }],
          [4, "succeeded", { outputs: 1, bytes: OUTPUT.length, elapsed_ms: 900 }],
        ]),
      });
    }
    if (method === "GET" && pathname === `/v1/jobs/${JOB_ID}`) {
      return json(route, origin, job("succeeded"));
    }
    if (method === "GET" && pathname === `/v1/jobs/${JOB_ID}/outputs`) {
      return json(route, origin, {
        job_id: JOB_ID,
        outputs: [
          {
            id: OUTPUT_ID,
            filename: "report.docx",
            format: "docx",
            media_type: "application/octet-stream",
            size: OUTPUT.length,
            sha256: createHash("sha256").update(OUTPUT).digest("hex"),
            url: `${STORAGE}/o/${OUTPUT_ID}`,
            expires_at: later(5),
          },
        ],
      });
    }
    if (method === "DELETE" && pathname === `/v1/jobs/${JOB_ID}`) {
      return route.fulfill({ status: 204, headers: cors(origin) });
    }
    return json(route, origin, { error: { code: "not_found", message: "no route" } }, 404);
  });

  await page.route(`${STORAGE}/**`, async (route) => {
    const origin = originOf(route);
    if (route.request().method() === "OPTIONS") {
      await route.fulfill({ status: 204, headers: cors(origin) });
      return;
    }
    await record(route);
    if (route.request().method() === "PUT") {
      await route.fulfill({ status: 200, headers: cors(origin), body: "" });
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/octet-stream",
      headers: cors(origin),
      body: OUTPUT,
    });
  });

  if (AUTH) {
    await page.route(`${AUTH}/**`, async (route) => {
      const origin = originOf(route);
      if (route.request().method() === "OPTIONS") {
        await route.fulfill({ status: 204, headers: cors(origin) });
        return;
      }
      await record(route);
      const { pathname } = new URL(route.request().url());
      if (pathname === "/api/auth/get-session") {
        return json(
          route,
          origin,
          signedIn
            ? {
                session: { id: "session-1" },
                user: { id: "user-1", name: "Ada", email: "ada@example.com" },
              }
            : null,
        );
      }
      if (signedIn && pathname === "/api/auth/token") {
        return json(route, origin, { token: "jwt-user-1" });
      }
      return json(route, origin, { code: "UNAUTHORIZED", message: "no session" }, 401);
    });
  }

  // A stand-in for Cloudflare's script that passes the check at once.
  await page.route("https://challenges.cloudflare.com/**", (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/javascript",
      body: `window.turnstile = {
        render(element, options) { setTimeout(() => options.callback("e2e-token"), 0); return "w"; },
        reset() {}, remove() {},
      };`,
    }),
  );
  return backend;
}

const apiRequests = (backend: Backend) =>
  backend.requests.filter((request) => API && request.url.startsWith(API));

for (const signedIn of [false, true]) {
  test(`keeps a converter-created job on Jobs when ${signedIn ? "signed in" : "anonymous"}`, async ({
    page,
  }) => {
    test.skip(!AUTH, "the account pages need an auth service URL");
    const backend = await mockCloud(page, signedIn);
    await stage(page);
    await chooseCloud(page);
    await page.getByTestId("cloud-start").click();
    await expect(page.getByTestId("cloud-result")).toBeVisible();
    await expect(page.getByRole("link", { name: "View cloud jobs" })).toHaveAttribute(
      "href",
      "/jobs",
    );
    // Exercise React's panel cleanup, rather than relying on a full-page unload.
    await page.getByRole("radio", { name: /On this device/ }).check();
    await expect(page.getByTestId("cloud-panel")).toHaveCount(0);
    await page.goto("/jobs");
    await expect(page).toHaveURL(/\/jobs/);
    await expect(page.getByRole("heading", { name: "report.md" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Download report.docx" })).toBeVisible();
    await expect(page.getByRole("button", { name: /Delete now.*report\.md/ })).toBeVisible();
    expect(backend.requests.some((request) => request.method === "DELETE")).toBe(false);
    const read = backend.requests.findLast(
      (request) => request.method === "GET" && request.url.endsWith(`/v1/jobs/${JOB_ID}`),
    );
    expect(read?.headers.authorization).toBe(signedIn ? "Bearer jwt-user-1" : undefined);
  });
}

async function stage(page: Page) {
  await page.goto("/app?to=docx");
  await page.locator("input[type=file]").setInputFiles({
    name: "report.md",
    mimeType: "text/markdown",
    buffer: Buffer.from(MARKDOWN),
  });
  await expect(page.getByTestId("input-name")).toHaveText("report.md");
}

async function chooseCloud(page: Page) {
  const host = new URL(API as string).host;
  await page.getByRole("radio", { name: new RegExp(`On ${host.replace(/\./g, "\\.")}`) }).check();
  await expect(page.getByTestId("cloud-panel")).toBeVisible();
  await expect(page.getByTestId("cloud-disclosure")).toContainText(
    `This file will be uploaded to ${host} for conversion.`,
  );
}

test("defaults to this device and uploads nothing before the explicit choice", async ({ page }) => {
  const backend = await mockCloud(page);
  await stage(page);
  await expect(page.getByRole("radio", { name: /On this device/ })).toBeChecked();
  await expect(page.getByTestId("device-indicator")).toContainText("Processed on this device.");
  await chooseCloud(page);
  await expect(page.getByTestId("cloud-start")).not.toHaveAttribute("aria-disabled", "true");
  expect(apiRequests(backend).some((request) => request.method !== "GET")).toBe(false);
  expect(backend.requests.some((request) => request.url.startsWith(STORAGE))).toBe(false);
});

test("uploads, converts, downloads and deletes with the keyboard", async ({ page }) => {
  const backend = await mockCloud(page);
  await stage(page);
  const host = new URL(API as string).host;

  // Choose the destination and start with the keyboard alone.
  await page.getByRole("radio", { name: /On this device/ }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("radio", { name: new RegExp(`On ${host}`) })).toBeChecked();
  await expect(page.getByLabel("Target format for report.md")).toHaveValue("docx");
  const start = page.getByTestId("cloud-start");
  await expect(start).not.toHaveAttribute("aria-disabled", "true");
  await start.focus();
  await page.keyboard.press("Enter");

  await expect(page.getByTestId("cloud-status")).toBeFocused();
  await expect(page.getByTestId("cloud-result")).toBeVisible();
  await expect(page.getByTestId("cloud-status")).toContainText("Ready");
  await expect(page.getByTestId("cloud-expiry")).toContainText("Deletes automatically at");
  await expect(page.getByTestId("cloud-announcer")).toContainText(
    "report.docx is ready to download",
  );

  const created = backend.requests.find(
    (request) => request.method === "POST" && request.url.endsWith("/v1/uploads"),
  );
  expect(JSON.parse(created?.body ?? "{}")).toEqual({
    filename: "report.md",
    size: Buffer.byteLength(MARKDOWN),
    sha256: createHash("sha256").update(MARKDOWN).digest("hex"),
  });
  if (SITE_KEY) {
    expect(created?.headers["cf-turnstile-response"]).toBe("e2e-token");
  }
  const put = backend.requests.find((request) => request.method === "PUT");
  expect(put?.url).toBe(`${STORAGE}/t/${UPLOAD_ID}`);
  expect(put?.headers["x-amz-checksum-sha256"]).toBe("c2lnbmVk");
  expect(put?.headers.authorization).toBeUndefined();
  const job = backend.requests.find(
    (request) => request.method === "POST" && request.url.endsWith("/v1/jobs"),
  );
  expect(JSON.parse(job?.body ?? "{}")).toMatchObject({ upload_id: UPLOAD_ID, to: "docx" });

  const download = page.waitForEvent("download");
  const button = page.getByRole("button", { name: "Download report.docx" });
  await button.focus();
  await page.keyboard.press("Enter");
  const saved = await download;
  expect(saved.suggestedFilename()).toBe("report.docx");
  expect(await readFile((await saved.path()) as string)).toEqual(OUTPUT);
  await expect(page).toHaveURL(/\/app/);

  const remove = page.getByRole("button", { name: "Delete report.md from the server now" });
  await remove.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("cloud-tombstone")).toBeFocused();
  await expect(page.getByTestId("cloud-tombstone")).toContainText(
    `Deleted from ${host}. The input and output files are gone.`,
  );
  expect(
    backend.requests.some(
      (request) => request.method === "DELETE" && request.url.endsWith(`/v1/jobs/${JOB_ID}`),
    ),
  ).toBe(true);
});

test("deletes after the download when asked", async ({ page }) => {
  const backend = await mockCloud(page);
  await stage(page);
  await chooseCloud(page);
  await page.getByTestId("cloud-start").click();
  await expect(page.getByTestId("cloud-result")).toBeVisible();
  await page.getByRole("checkbox", { name: /Delete after download/ }).check();
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Download report.docx" }).click();
  await download;
  await expect(page.getByTestId("cloud-tombstone")).toContainText("after your download started");
  expect(backend.requests.filter((request) => request.method === "DELETE")).toHaveLength(1);
});

test("shows a quota refusal and keeps the file for on-device conversion", async ({ page }) => {
  const backend = await mockCloud(page);
  backend.createUpload = async (route, origin) => {
    await json(
      route,
      origin,
      { error: { code: "quota_exceeded", message: "the plan's upload allowance is exhausted" } },
      429,
    );
    return true;
  };
  await stage(page);
  await chooseCloud(page);
  await page.getByTestId("cloud-start").click();
  const error = page.getByTestId("cloud-error");
  await expect(error).toContainText("Cloud allowance used up");
  await expect(error.getByRole("heading")).toBeFocused();
  await expect(page.getByTestId("cloud-alerts")).toContainText("Cloud allowance used up");
  await expect(page.getByTestId("cloud-status")).toContainText("Cloud allowance used up");
  expect(backend.requests.some((request) => request.method === "PUT")).toBe(false);

  await error.getByRole("button", { name: "Convert on this device" }).click();
  await expect(page.getByRole("radio", { name: /On this device/ })).toBeChecked();
  await expect(page.getByTestId("input-name")).toHaveText("report.md");
});

test("waits for Retry-After before a retry", async ({ page }) => {
  const backend = await mockCloud(page);
  let refused = false;
  backend.createUpload = async (route, origin) => {
    if (refused) {
      return false;
    }
    refused = true;
    await json(route, origin, { error: { code: "rate_limited", message: "slow down" } }, 429, {
      "retry-after": "2",
    });
    return true;
  };
  await stage(page);
  await chooseCloud(page);
  await page.getByTestId("cloud-start").click();
  const retry = page.getByRole("button", { name: /^Retry/ });
  await expect(retry).toHaveText(/Retry in \d s/);
  await expect(retry).toHaveAttribute("aria-disabled", "true");
  await expect(retry).toHaveText("Retry", { timeout: 5_000 });
  await expect(retry).not.toHaveAttribute("aria-disabled", "true");
});

test("says so when offline instead of uploading", async ({ page, context }) => {
  await mockCloud(page);
  await stage(page);
  await chooseCloud(page);
  await context.setOffline(true);
  await expect(page.getByTestId("cloud-offline")).toBeVisible();
  await expect(page.getByTestId("cloud-start")).toHaveAttribute("aria-disabled", "true");
  await expect(page.getByTestId("cloud-blocked")).toContainText("You are offline");
  await context.setOffline(false);
});

test("offers Process in Cloud for a format the browser cannot convert", async ({ page }) => {
  await mockCloud(page);
  await page.goto("/app?to=markdown");
  await page.locator("input[type=file]").setInputFiles({
    name: "page.rtf",
    mimeType: "application/rtf",
    buffer: Buffer.from("{\\rtf1 Hello}"),
  });
  await expect(page.getByTestId("desktop-handoff")).toBeVisible();
  await page.getByRole("button", { name: "Process in Cloud" }).click();
  await expect(page.getByTestId("cloud-panel")).toBeVisible();
});
