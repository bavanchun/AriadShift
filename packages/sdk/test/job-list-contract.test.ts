import { describe, expect, it } from "vitest";
import { AriadApiError, createAriadClient } from "../src/index";
import { apiError, BASE_URL, jobBody, json, mockFetch } from "./mock-fetch";

describe("job list contract", () => {
  it("passes filters and an opaque pagination cursor unchanged", async () => {
    const cursor = "2026-10-28T10:00:00Z+id/a?b&c=1";
    const first = { jobs: [{ ...jobBody("failed"), filename: "report.md" }], next_cursor: cursor };
    const last = { jobs: [{ ...jobBody("failed"), id: "older-job" }], next_cursor: null };
    const mock = mockFetch(
      () => json(first),
      () => json(last),
    );
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { apiKey: "ak_test" },
      fetch: mock.fetch,
    });

    const page = await client.listJobs({ status: "failed", limit: 1 });
    expect(page).toEqual(first);
    expect(
      await client.listJobs({ status: "failed", limit: 1, cursor: page.next_cursor ?? undefined }),
    ).toEqual(last);
    expect(Object.fromEntries(new URL(mock.requests[0]?.url ?? "").searchParams)).toEqual({
      status: "failed",
      limit: "1",
    });
    expect(Object.fromEntries(new URL(mock.requests[1]?.url ?? "").searchParams)).toEqual({
      status: "failed",
      limit: "1",
      cursor,
    });
    expect(
      mock.requests.every((r) => r.method === "GET" && r.headers.get("x-api-key") === "ak_test"),
    ).toBe(true);
  });

  it.each([undefined, {}, { limit: undefined, cursor: undefined, status: undefined }])(
    "leaves defaults to the server for options %j",
    async (options) => {
      const page = { jobs: [], next_cursor: null };
      const mock = mockFetch(() => json(page));
      const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });
      expect(await client.listJobs(options)).toEqual(page);
      expect(mock.requests[0]?.url).toBe(`${BASE_URL}/v1/jobs`);
    },
  );

  it("surfaces a refused limit as a typed error without changing the request", async () => {
    const mock = mockFetch(() =>
      apiError(422, "invalid_request", "limit must be between 1 and 100"),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });
    const error = await client.listJobs({ limit: 0 }).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(AriadApiError);
    expect(error).toMatchObject({ status: 422, code: "invalid_request" });
    expect(new URL(mock.requests[0]?.url ?? "").searchParams.get("limit")).toBe("0");
  });

  it("passes the bearer token and caller abort signal without serializing the signal", async () => {
    const controller = new AbortController();
    const mock = mockFetch(
      (request) =>
        new Promise<Response>((_, reject) => {
          request.signal.addEventListener("abort", () => reject(request.signal.reason), {
            once: true,
          });
          controller.abort();
        }),
    );
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { bearer: "jwt" },
      fetch: mock.fetch,
    });

    const error = await client.listJobs({ signal: controller.signal }).catch((e: unknown) => e);
    expect(error).toBe(controller.signal.reason);
    expect(mock.requests[0]?.url).toBe(`${BASE_URL}/v1/jobs`);
    expect(mock.requests[0]?.headers.get("authorization")).toBe("Bearer jwt");
    expect(mock.requests[0]?.signal.aborted).toBe(true);
  });
});
