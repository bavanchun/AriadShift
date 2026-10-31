import { describe, expect, it } from "vitest";
import { AriadApiError, createAriadClient } from "../src/index";
import { apiError, BASE_URL, json, mockFetch, UPLOAD_ID } from "./mock-fetch";

const planLimits = {
  plan: "anonymous",
  authenticated: false,
  retention_s: 3600,
  requests_per_minute: 30,
  limits: { max_input_bytes: 10_485_760, max_pages: 50, timeout_s: 120, max_memory_mb: null },
  quotas: {
    uploads_per_day: { limit: 20, used: 1, remaining: 19 },
    jobs_per_day: { limit: 20, used: 0, remaining: 20 },
    input_bytes_per_day: { limit: 104_857_600, used: 5, remaining: 104_857_595 },
    pending_uploads: { limit: 1, used: 0, remaining: 1 },
    concurrent_jobs: { limit: null, used: 0, remaining: null },
  },
};

describe("deleteUpload", () => {
  it("uses the API URL, credentials and signal and accepts repeated 204 responses", async () => {
    const mock = mockFetch(
      ...Array.from({ length: 2 }, () => () => new Response(null, { status: 204 })),
    );
    const controller = new AbortController();
    const client = createAriadClient({
      baseUrl: `${BASE_URL}/`,
      credentials: { apiKey: "ak_test" },
      fetch: mock.fetch,
    });

    await expect(client.deleteUpload(UPLOAD_ID, controller.signal)).resolves.toBeUndefined();
    await expect(client.deleteUpload(UPLOAD_ID, controller.signal)).resolves.toBeUndefined();
    expect(mock.requests).toHaveLength(2);
    controller.abort();
    for (const request of mock.requests) {
      expect(request.method).toBe("DELETE");
      expect(request.url).toBe(`${BASE_URL}/v1/uploads/${UPLOAD_ID}`);
      expect(request.headers.get("x-api-key")).toBe("ak_test");
      expect(request.signal.aborted).toBe(true);
    }
  });

  it("surfaces a conflict for an upload used by a job", async () => {
    const mock = mockFetch(() => apiError(409, "conflict", "a job uses the upload"));
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });
    const error = await client.deleteUpload(UPLOAD_ID).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(AriadApiError);
    expect(error).toMatchObject({ status: 409, code: "conflict" });
  });
});

describe("limits", () => {
  it("preserves nullable limits and quotas for anonymous callers", async () => {
    const mock = mockFetch(() => json(planLimits));
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });
    expect(await client.limits()).toEqual(planLimits);
    expect(mock.requests[0]?.method).toBe("GET");
    expect(mock.requests[0]?.url).toBe(`${BASE_URL}/v1/limits`);
    expect(mock.requests[0]?.headers.get("authorization")).toBeNull();
    expect(mock.requests[0]?.headers.get("x-api-key")).toBeNull();
  });

  it("passes signed-in credentials and caller cancellation", async () => {
    const signedIn = { ...planLimits, plan: "free", authenticated: true };
    const mock = mockFetch(() => json(signedIn));
    const controller = new AbortController();
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { bearer: "jwt" },
      fetch: mock.fetch,
    });
    expect(await client.limits(controller.signal)).toEqual(signedIn);
    expect(mock.requests[0]?.headers.get("authorization")).toBe("Bearer jwt");
    controller.abort();
    expect(mock.requests[0]?.signal.aborted).toBe(true);
  });
});
