import { describe, expect, it, vi } from "vitest";
import { createAriadClient } from "../src/index";
import { apiError, BASE_URL, json, mockFetch, UPLOAD_ID, uploadBody } from "./mock-fetch";

const body = new TextEncoder().encode("hello");
const options = { filename: "report.md", sha256: uploadBody("pending").sha256 };
const noContent = () => new Response(null, { status: 204 });

describe("cleanup credential deadline", () => {
  it.each([
    ["bearer", "refused", "resolve"],
    ["bearer", "refused", "reject"],
    ["bearer", "canceled", "resolve"],
    ["bearer", "canceled", "reject"],
    ["apiKey", "refused", "resolve"],
    ["apiKey", "refused", "reject"],
    ["apiKey", "canceled", "resolve"],
    ["apiKey", "canceled", "reject"],
  ] as const)(
    "bounds stalled %s credential refresh after a %s transfer and handles late %s",
    async (kind, stage, late) => {
      vi.useFakeTimers();
      // Native timeout signals use an internal clock; bridge it to the fake clock
      // so the regression also exercises signal expiry in the old implementation.
      vi.spyOn(AbortSignal, "timeout").mockImplementation((ms) => {
        const controller = new AbortController();
        setTimeout(() => controller.abort(new DOMException("deadline", "TimeoutError")), ms);
        return controller.signal;
      });
      let releaseRefresh: (value: string) => void = () => {};
      let rejectRefresh: (reason: unknown) => void = () => {};
      const refresh = new Promise<string>((resolve, reject) => {
        releaseRefresh = resolve;
        rejectRefresh = reject;
      });
      let cleanupStarted: () => void = () => {};
      const started = new Promise<void>((resolve) => {
        cleanupStarted = resolve;
      });
      const credential = crypto.randomUUID();
      let calls = 0;
      const lookup = () => {
        if (++calls === 1) return credential;
        cleanupStarted();
        return refresh;
      };
      const caller = new AbortController();
      const mock = mockFetch(
        () => json(uploadBody("pending"), 201),
        () => {
          if (stage === "canceled") {
            caller.abort(new DOMException("transfer canceled", "AbortError"));
            throw caller.signal.reason;
          }
          return apiError(403, "forbidden", "transfer refused");
        },
        noContent,
      );
      const client = createAriadClient({
        baseUrl: BASE_URL,
        credentials: kind === "bearer" ? { bearer: lookup } : { apiKey: lookup },
        fetch: mock.fetch,
      });
      let observed: unknown;
      const outcome = client
        .upload(body, { ...options, signal: caller.signal })
        .catch((error: unknown) => {
          observed = error;
          return error;
        });
      try {
        await started;
        await vi.advanceTimersByTimeAsync(9_999);
        expect(observed).toBeUndefined();
        await vi.advanceTimersByTimeAsync(1);
        if (stage === "canceled") expect(observed).toBe(caller.signal.reason);
        else
          expect(observed).toMatchObject({
            status: 403,
            code: "forbidden",
            message: "transfer refused",
          });
        expect(calls).toBe(2);
        expect(mock.requests.map((request) => request.method)).toEqual(["POST", "PUT"]);
        expect(vi.getTimerCount()).toBe(0);
      } finally {
        if (late === "resolve") releaseRefresh(credential);
        else rejectRefresh(new Error("late credential refresh failure"));
        await outcome;
        await vi.runAllTimersAsync();
        vi.restoreAllMocks();
        vi.useRealTimers();
      }
      expect(mock.requests.map((request) => request.method)).toEqual(["POST", "PUT"]);
    },
  );
});

describe("cleanup fetch deadline", () => {
  it.each(["resolve", "reject"])(
    "bounds a non-cooperative cleanup fetch with late %s",
    async (late) => {
      vi.useFakeTimers();
      vi.spyOn(AbortSignal, "timeout").mockImplementation((ms) => {
        const controller = new AbortController();
        setTimeout(() => controller.abort(new DOMException("deadline", "TimeoutError")), ms);
        return controller.signal;
      });
      let release: (response: Response) => void = () => {};
      let reject: (reason: unknown) => void = () => {};
      const response = new Promise<Response>((resolve, fail) => {
        release = resolve;
        reject = fail;
      });
      let cleanupStarted: () => void = () => {};
      const started = new Promise<void>((resolve) => {
        cleanupStarted = resolve;
      });
      const failure = new TypeError("transfer disconnected");
      const mock = mockFetch(
        () => json(uploadBody("pending"), 201),
        () => {
          throw failure;
        },
        () => {
          cleanupStarted();
          return response;
        },
      );
      const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });
      let observed: unknown;
      const outcome = client.upload(body, options).catch((error: unknown) => {
        observed = error;
      });
      try {
        await started;
        await vi.advanceTimersByTimeAsync(9_999);
        expect(observed).toBeUndefined();
        await vi.advanceTimersByTimeAsync(1);
        expect(observed).toBe(failure);
        expect(mock.requests.at(-1)?.signal.aborted).toBe(true);
        expect(vi.getTimerCount()).toBe(0);
      } finally {
        if (late === "resolve") release(noContent());
        else reject(new TypeError("late cleanup failure"));
        await outcome;
        await vi.runAllTimersAsync();
        vi.restoreAllMocks();
        vi.useRealTimers();
      }
      expect(observed).toBe(failure);
    },
  );
});

describe("failed upload cleanup", () => {
  it("deletes a refused transfer using the API URL and refreshed credentials", async () => {
    const mock = mockFetch(
      () => json(uploadBody("pending"), 201),
      () => apiError(403, "forbidden", "the URL expired"),
      noContent,
    );
    let token = 0;
    const client = createAriadClient({
      baseUrl: `${BASE_URL}//`,
      credentials: { bearer: () => `token-${++token}` },
      fetch: mock.fetch,
    });

    await expect(client.upload(body, options)).rejects.toMatchObject({
      status: 403,
      code: "forbidden",
    });
    expect(mock.requests.map((r) => r.method)).toEqual(["POST", "PUT", "DELETE"]);
    expect(mock.requests[2]?.url).toBe(`${BASE_URL}/v1/uploads/${UPLOAD_ID}`);
    expect(mock.requests[2]?.headers.get("authorization")).toBe("Bearer token-2");
    expect(mock.requests[1]?.headers.get("authorization")).toBeNull();
    expect(mock.pending()).toBe(0);
  });

  it("awaits completion and deletes the upload if verification refuses it", async () => {
    const mock = mockFetch(
      () => json(uploadBody("pending"), 201),
      noContent,
      () => apiError(422, "invalid_request", "the file does not match its SHA-256"),
      noContent,
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(client.upload(body, options)).rejects.toMatchObject({
      status: 422,
      code: "invalid_request",
    });
    expect(mock.requests.map((r) => r.method)).toEqual(["POST", "PUT", "POST", "DELETE"]);
    expect(mock.requests[3]?.url).toBe(`${BASE_URL}/v1/uploads/${UPLOAD_ID}`);
  });

  it.each(["transfer", "completion"])(
    "cleans up an aborted %s with an independent signal",
    async (stage) => {
      const controller = new AbortController();
      const abort = () => {
        controller.abort();
        throw controller.signal.reason;
      };
      const mock = mockFetch(
        () => json(uploadBody("pending"), 201),
        ...(stage === "completion" ? [noContent] : []),
        abort,
        noContent,
      );
      const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

      const error = await client
        .upload(body, { ...options, signal: controller.signal })
        .catch((e: unknown) => e);
      expect(error).toBe(controller.signal.reason);
      const cleanup = mock.requests.at(-1);
      expect(cleanup?.method).toBe("DELETE");
      expect(cleanup?.signal.aborted).toBe(false);
      expect(cleanup?.signal).not.toBe(controller.signal);
      expect(mock.requests[0]?.signal.aborted).toBe(true);
    },
  );

  it.each(["HTTP", "network"])(
    "preserves the original error after a cleanup %s failure",
    async (kind) => {
      const failure = new TypeError("transfer disconnected");
      const mock = mockFetch(
        () => json(uploadBody("pending"), 201),
        () => {
          throw failure;
        },
        () => {
          if (kind === "network") throw new TypeError("cleanup disconnected");
          return apiError(500, "internal", "cleanup failed");
        },
      );
      const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

      await expect(client.upload(body, options)).rejects.toBe(failure);
      expect(mock.requests.at(-1)?.method).toBe("DELETE");
      expect(mock.pending()).toBe(0);
    },
  );

  it("bounds cleanup at ten seconds and preserves the original error on timeout", async () => {
    const failure = new TypeError("transfer disconnected");
    const mock = mockFetch(
      () => json(uploadBody("pending"), 201),
      () => {
        throw failure;
      },
      (request) =>
        new Promise<Response>((_, reject) => {
          request.signal.addEventListener("abort", () => reject(request.signal.reason), {
            once: true,
          });
        }),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(client.upload(body, options)).rejects.toBe(failure);
    expect(mock.requests.at(-1)?.method).toBe("DELETE");
    expect(mock.requests.at(-1)?.signal.aborted).toBe(true);
    expect(mock.requests.at(-1)?.signal.reason).toMatchObject({ name: "TimeoutError" });
  }, 20_000);

  it("keeps successful uploads to create, transfer and completion with caller signals", async () => {
    const mock = mockFetch(
      () => json(uploadBody("pending"), 201),
      noContent,
      () => json(uploadBody("completed")),
    );
    const controller = new AbortController();
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(
      client.upload(body, { ...options, signal: controller.signal }),
    ).resolves.toMatchObject({ status: "completed" });
    expect(mock.requests.map((r) => r.method)).toEqual(["POST", "PUT", "POST"]);
    controller.abort();
    expect(mock.requests.every((r) => r.signal.aborted)).toBe(true);
  });

  it("does not delete an upload when creation returns no presigned request", async () => {
    const mock = mockFetch(() => json(uploadBody("completed"), 201));
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(client.upload(body, options)).rejects.toThrow("has no presigned request");
    expect(mock.requests.map((r) => r.method)).toEqual(["POST"]);
  });

  it("does not clean up when creation fails before returning an upload", async () => {
    const mock = mockFetch(() => apiError(429, "quota_exceeded", "upload allowance exhausted"));
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(client.upload(body, options)).rejects.toMatchObject({ code: "quota_exceeded" });
    expect(mock.requests).toHaveLength(1);
  });
});
