import { describe, expect, it } from "vitest";
import { AriadApiError, createAriadClient } from "../src/index";
import { apiError, BASE_URL, JOB_ID, jobBody, json, mockFetch } from "./mock-fetch";

describe("listJobs", () => {
  it("reads a page of the account's jobs with the credentials", async () => {
    const page = {
      jobs: [{ ...jobBody("succeeded"), filename: "report.md" }],
      next_cursor: "opaque-cursor",
    };
    const mock = mockFetch(() => json(page));
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { apiKey: "ak_test" },
      fetch: mock.fetch,
    });

    const result = await client.listJobs({ limit: 20, status: "succeeded" });

    expect(result).toEqual(page);
    expect(result.jobs[0]?.id).toBe(JOB_ID);
    const request = mock.requests[0];
    expect(request?.method).toBe("GET");
    expect(request?.headers.get("x-api-key")).toBe("ak_test");
    const url = new URL(request?.url ?? "");
    expect(url.pathname).toBe("/v1/jobs");
    expect(Object.fromEntries(url.searchParams)).toEqual({ limit: "20", status: "succeeded" });
  });

  it("passes the cursor unchanged and sends no query when called without options", async () => {
    const mock = mockFetch(
      () => json({ jobs: [], next_cursor: null }),
      () => json({ jobs: [], next_cursor: null }),
    );
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { bearer: "token" },
      fetch: mock.fetch,
    });

    expect(await client.listJobs()).toEqual({ jobs: [], next_cursor: null });
    await client.listJobs({ cursor: "a-b_c" });

    expect(mock.requests[0]?.url).toBe(`${BASE_URL}/v1/jobs`);
    expect(new URL(mock.requests[1]?.url ?? "").searchParams.get("cursor")).toBe("a-b_c");
  });

  it("surfaces a refused anonymous list as an AriadApiError", async () => {
    const mock = mockFetch(() =>
      apiError(401, "unauthorized", "listing jobs requires credentials"),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    const error = await client.listJobs().catch((e: unknown) => e);

    expect(error).toBeInstanceOf(AriadApiError);
    expect(error).toMatchObject({ status: 401, code: "unauthorized" });
  });
});
