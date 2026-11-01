// `GET /v1/jobs` feature detection: a deployment without the route falls back,
// a listed page is validated, and real failures surface as API errors.
import { AriadApiError } from "@ariadshift/sdk";
import { type AuthApi, AuthError } from "@/components/account/auth-api";
import { createJobsApi, JOB_PAGE_SIZE, listJobs } from "@/components/account/jobs-api";

type GetResult = { data?: unknown; error?: unknown; response?: Response };

function fakeClient(result: GetResult) {
  const get = vi.fn(async (_options: unknown) => result);
  return { client: { raw: { get } } as unknown as Parameters<typeof listJobs>[0], get };
}

const JOB = {
  id: "job-1",
  status: "succeeded",
  upload_id: "upload-1",
  from: "pdf",
  to: "docx",
  profile: "editable",
  pack: "docling",
  route: [],
  warnings: [],
  attempts: 1,
  created_at: "2026-10-25T12:00:00Z",
  expires_at: "2026-10-25T13:00:00Z",
  filename: "report.pdf",
};

describe("listJobs", () => {
  it("asks for the first page through the SDK client", async () => {
    const { client, get } = fakeClient({
      data: { jobs: [JOB], next_cursor: "next" },
      response: new Response(null, { status: 200 }),
    });
    expect(await listJobs(client)).toEqual({ supported: true, jobs: [JOB], nextCursor: "next" });
    expect(get).toHaveBeenCalledWith(
      expect.objectContaining({ url: "/v1/jobs", query: { limit: JOB_PAGE_SIZE } }),
    );
  });

  it("passes the cursor of a later page and reads the last page", async () => {
    const { client, get } = fakeClient({
      data: { jobs: [], next_cursor: null },
      response: new Response(null, { status: 200 }),
    });
    expect(await listJobs(client, { cursor: "next" })).toEqual({
      supported: true,
      jobs: [],
      nextCursor: null,
    });
    expect(get).toHaveBeenCalledWith(
      expect.objectContaining({ query: { limit: JOB_PAGE_SIZE, cursor: "next" } }),
    );
  });

  it.each([404, 405, 501])("reports a deployment without the route (%i)", async (status) => {
    const { client } = fakeClient({ error: {}, response: new Response(null, { status }) });
    expect(await listJobs(client)).toEqual({ supported: false });
  });

  it("throws the API error of a refused list", async () => {
    const { client } = fakeClient({
      error: { error: { code: "unauthorized", message: "credentials rejected" } },
      response: new Response(null, { status: 401 }),
    });
    const failure = await listJobs(client).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(AriadApiError);
    expect(failure).toMatchObject({ status: 401, code: "unauthorized" });
  });

  it("rejects a body that is not a job list", async () => {
    const { client } = fakeClient({
      data: { jobs: [{ id: 1 }] },
      response: new Response(null, { status: 200 }),
    });
    await expect(listJobs(client)).rejects.toThrow(/unexpected body/);
  });

  it("rethrows a request that got no response", async () => {
    const { client } = fakeClient({ error: new TypeError("Failed to fetch") });
    await expect(listJobs(client)).rejects.toThrow("Failed to fetch");
  });

  it("refuses signed-in requests without sending an anonymous call and preserves 401, 503, 429 and network errors", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockImplementation(async () => {
      throw new Error("unexpected anonymous fetch");
    });
    try {
      const unauthorizedAuth = {
        apiToken: vi.fn(async () => {
          throw new AuthError("Your session has ended. Sign in again.", 401, "unauthorized");
        }),
      } as unknown as AuthApi;
      const unauthorizedApi = createJobsApi("http://localhost:3902", unauthorizedAuth, true);
      const unauthorizedFailure = await unauthorizedApi.listJobs().catch((error: unknown) => error);
      expect(unauthorizedFailure).toBeInstanceOf(AriadApiError);
      expect(unauthorizedFailure).toMatchObject({
        status: 401,
        code: "unauthorized",
        message: "Your session has ended. Sign in again.",
      });

      const unavailableAuth = {
        apiToken: vi.fn(async () => {
          throw new AuthError(
            "The server cannot verify your sign-in right now. Try again shortly.",
            503,
            "unavailable",
          );
        }),
      } as unknown as AuthApi;
      const unavailableApi = createJobsApi("http://localhost:3902", unavailableAuth, true);
      const unavailableFailure = await unavailableApi.listJobs().catch((error: unknown) => error);
      expect(unavailableFailure).toBeInstanceOf(AriadApiError);
      expect(unavailableFailure).toMatchObject({
        status: 503,
        code: "unavailable",
        message: "The server cannot verify your sign-in right now. Try again shortly.",
      });

      const rateLimitedAuth = {
        apiToken: vi.fn(async () => {
          throw new AuthError("Too many requests. Try again shortly.", 429, "rate_limited");
        }),
      } as unknown as AuthApi;
      const rateLimitedApi = createJobsApi("http://localhost:3902", rateLimitedAuth, true);
      const rateLimitedFailure = await rateLimitedApi.listJobs().catch((error: unknown) => error);
      expect(rateLimitedFailure).toBeInstanceOf(AriadApiError);
      expect(rateLimitedFailure).toMatchObject({
        status: 429,
        code: "rate_limited",
        message: "Too many requests. Try again shortly.",
      });

      const networkAuth = {
        apiToken: vi.fn(async () => {
          throw new AuthError(
            "The sign-in service cannot be reached. Check your connection and try again.",
            undefined,
            "network_error",
          );
        }),
      } as unknown as AuthApi;
      const networkApi = createJobsApi("http://localhost:3902", networkAuth, true);
      const networkFailure = await networkApi.listJobs().catch((error: unknown) => error);
      expect(networkFailure).toBeInstanceOf(AriadApiError);
      expect(networkFailure).toMatchObject({
        status: 503,
        code: "unavailable",
        message: "The sign-in service cannot be reached. Check your connection and try again.",
      });

      const malformedAuth = {
        apiToken: vi.fn(async () => {
          throw new AuthError("invalid token", 200, "invalid_token_response");
        }),
      } as unknown as AuthApi;
      const malformedApi = createJobsApi("http://localhost:3902", malformedAuth, true);
      const malformedFailure = await malformedApi.getJob("job-1").catch((error: unknown) => error);
      expect(malformedFailure).toBeInstanceOf(AriadApiError);
      expect(malformedFailure).toMatchObject({
        code: "unavailable",
        message: "The server cannot verify your sign-in right now. Try again shortly.",
      });
      expect(fetchSpy).not.toHaveBeenCalled();
    } finally {
      fetchSpy.mockRestore();
    }
  });
});
