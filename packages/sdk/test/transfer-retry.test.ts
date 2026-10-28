import { afterEach, describe, expect, it, vi } from "vitest";
import { createAriadClient, retryAfterSeconds } from "../src/index";
import { apiError, BASE_URL, mockFetch, uploadBody } from "./mock-fetch";

const signedFixture = uploadBody("pending").upload;
if (!signedFixture) throw new Error("fixture has no presigned request");
const presigned = {
  ...signedFixture,
  expires_at: new Date(Date.now() + 15 * 60 * 1000).toISOString(),
};
const bytes = () => new TextEncoder().encode("hello");
const busy = () => apiError(429, "rate_limited", "busy", { "retry-after": "0" });
afterEach(() => vi.useRealTimers());

describe("presigned transfer retry", () => {
  it.each(["Blob", "ArrayBuffer", "Uint8Array"])(
    "retries 429 then 408 with the same %s body and request",
    async (kind) => {
      const body =
        kind === "Blob" ? new Blob([bytes()]) : kind === "ArrayBuffer" ? bytes().buffer : bytes();
      const controller = new AbortController();
      const seen: RequestInit[] = [];
      const mock = mockFetch(
        busy,
        () => apiError(408, "transfer_timeout", "timeout", { "retry-after": "0" }),
        () => new Response(null),
      );
      const client = createAriadClient({
        baseUrl: BASE_URL,
        credentials: { bearer: "private" },
        fetch: async (input, init) => {
          if (!init) throw new Error("transfer requires request options");
          seen.push(init);
          return mock.fetch(input, init);
        },
      });
      await client.transfer(presigned, body, controller.signal);
      expect(mock.requests).toHaveLength(3);
      for (const request of mock.requests) {
        expect(request.url).toBe(presigned.url);
        expect(request.method).toBe("PUT");
        expect(request.headers.get("content-type")).toBe("application/octet-stream");
        expect(request.headers.has("authorization")).toBe(false);
        expect(await request.text()).toBe("hello");
      }
      for (const init of seen) {
        expect(init.body).toBe(body);
        expect(init.headers).toBe(presigned.headers);
        expect(init.signal).toBe(controller.signal);
      }
    },
  );

  it("exhausts three busy attempts and preserves the final error", async () => {
    const mock = mockFetch(busy, busy, busy);
    await expect(
      createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch }).transfer(presigned, bytes()),
    ).rejects.toMatchObject({ status: 429, code: "rate_limited", retryAfter: 0 });
    expect(mock.requests).toHaveLength(3);
    expect(mock.pending()).toBe(0);
  });

  it("honors Retry-After and cancels response bodies before retry and success", async () => {
    vi.useFakeTimers();
    const cancelled: string[] = [];
    const response = (name: string, status: number, headers = {}) =>
      new Response(
        new ReadableStream({
          cancel() {
            cancelled.push(name);
          },
        }),
        { status, headers },
      );
    const mock = mockFetch(
      () => response("busy", 429, { "retry-after": "2" }),
      () => response("done", 200),
    );
    const transfer = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch }).transfer(
      presigned,
      bytes(),
    );
    await vi.advanceTimersByTimeAsync(1999);
    expect(mock.requests).toHaveLength(1);
    expect(cancelled).toEqual(["busy"]);
    await vi.advanceTimersByTimeAsync(1);
    await transfer;
    expect(mock.requests).toHaveLength(2);
    expect(cancelled).toEqual(["busy", "done"]);
  });

  it("waits one second when Retry-After is absent", async () => {
    vi.useFakeTimers();
    const mock = mockFetch(
      () => apiError(408, "transfer_timeout", "timeout"),
      () => new Response(null),
    );
    const transfer = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch }).transfer(
      presigned,
      bytes(),
    );
    await vi.advanceTimersByTimeAsync(999);
    expect(mock.requests).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    await transfer;
    expect(mock.requests).toHaveLength(2);
  });

  it.each([
    { name: "seven-day minimum", value: "604800", retryAfter: 604800 },
    { name: "timer-overflow minimum", value: "2147484", retryAfter: 2147484 },
    { name: "digits converting to Infinity", value: "9".repeat(400), retryAfter: undefined },
  ])("stops without a timer for $name and preserves the refusal", async ({ value, retryAfter }) => {
    vi.useFakeTimers();
    for (const status of [429, 408]) {
      const mock = mockFetch(
        () => apiError(status, "storage_busy", "original refusal", { "retry-after": value }),
        () => new Response(null),
      );
      let error: unknown;
      const transfer = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch })
        .transfer(presigned, bytes())
        .catch((cause) => {
          error = cause;
        });
      await vi.advanceTimersByTimeAsync(0);
      expect(error).toMatchObject({
        status,
        code: "storage_busy",
        message: "original refusal",
        retryAfter,
      });
      expect(mock.requests).toHaveLength(1);
      expect(vi.getTimerCount()).toBe(0);
      await transfer;
    }
  });

  it.each(["-1", "NaN", "Infinity", "1.5", "9007199254740992", "0".repeat(400)])(
    "rejects an invalid or unsafe Retry-After value (%#)",
    async (value) => {
      vi.useFakeTimers();
      expect(
        retryAfterSeconds(apiError(429, "busy", "busy", { "retry-after": value })),
      ).toBeUndefined();
      const mock = mockFetch(() =>
        apiError(429, "busy", "invalid delay", { "retry-after": value }),
      );
      let error: unknown;
      const transfer = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch })
        .transfer(presigned, bytes())
        .catch((cause) => {
          error = cause;
        });
      await vi.advanceTimersByTimeAsync(0);
      expect(error).toMatchObject({ status: 429, code: "busy", retryAfter: undefined });
      expect(mock.requests).toHaveLength(1);
      expect(vi.getTimerCount()).toBe(0);
      await transfer;
    },
  );

  it("accepts safe whole seconds including zero and the safe integer boundary", () => {
    for (const value of ["0", " 2 ", "9007199254740991"]) {
      expect(retryAfterSeconds(apiError(429, "busy", "busy", { "retry-after": value }))).toBe(
        Number(value),
      );
    }
  });

  it("allows a sixty-second minimum within the waiting budget", async () => {
    vi.useFakeTimers();
    const mock = mockFetch(
      () => apiError(429, "busy", "busy", { "retry-after": "60" }),
      () => new Response(null),
    );
    const transfer = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch }).transfer(
      presigned,
      bytes(),
    );
    await vi.advanceTimersByTimeAsync(59_999);
    expect(mock.requests).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    await transfer;
    expect(mock.requests).toHaveLength(2);
  });

  it("stops when a second minimum exceeds the remaining total waiting budget", async () => {
    vi.useFakeTimers();
    const mock = mockFetch(
      () => apiError(429, "busy", "first refusal", { "retry-after": "40" }),
      () => apiError(408, "transfer_timeout", "second refusal", { "retry-after": "21" }),
      () => new Response(null),
    );
    let error: unknown;
    const transfer = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch })
      .transfer(presigned, bytes())
      .catch((cause) => {
        error = cause;
      });
    await vi.advanceTimersByTimeAsync(39_999);
    expect(mock.requests).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(error).toMatchObject({
      status: 408,
      code: "transfer_timeout",
      message: "second refusal",
      retryAfter: 21,
    });
    expect(mock.requests).toHaveLength(2);
    expect(vi.getTimerCount()).toBe(0);
    await transfer;
  });

  it.each(["expiring", "expired", "invalid"])(
    "does not wait until or past an %s signed URL's expiry",
    async (expiry) => {
      vi.useFakeTimers();
      const mock = mockFetch(() => apiError(429, "busy", "refused", { "retry-after": "2" }));
      const request = {
        ...presigned,
        expires_at:
          expiry === "invalid"
            ? "not a date"
            : new Date(Date.now() + (expiry === "expiring" ? 2_000 : -1)).toISOString(),
      };
      let error: unknown;
      const transfer = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch })
        .transfer(request, bytes())
        .catch((cause) => {
          error = cause;
        });
      await vi.advanceTimersByTimeAsync(0);
      expect(error).toMatchObject({ status: 429, code: "busy", message: "refused", retryAfter: 2 });
      expect(mock.requests).toHaveLength(1);
      expect(vi.getTimerCount()).toBe(0);
      await transfer;
    },
  );

  it("uses a one-second default delay and cancels during that wait", async () => {
    vi.useFakeTimers();
    const controller = new AbortController();
    const mock = mockFetch(
      () => apiError(408, "transfer_timeout", "timeout"),
      () => new Response(null),
    );
    const transfer = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch }).transfer(
      presigned,
      bytes(),
      controller.signal,
    );
    const rejected = expect(transfer).rejects.toBe("cancelled");
    await vi.advanceTimersByTimeAsync(999);
    expect(mock.requests).toHaveLength(1);
    controller.abort("cancelled");
    await rejected;
    await vi.runAllTimersAsync();
    expect(mock.requests).toHaveLength(1);
  });

  it.each([400, 403, 409, 422, 500, 503])("does not retry status %s", async (status) => {
    const mock = mockFetch(() => apiError(status, "refused", "refused"));
    await expect(
      createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch }).transfer(presigned, bytes()),
    ).rejects.toMatchObject({ status });
    expect(mock.requests).toHaveLength(1);
  });

  it("does not retry a body outside the reusable byte contract", async () => {
    const mock = mockFetch(busy);
    const body = new FormData();
    body.set("data", "hello");
    await expect(
      createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch }).transfer(
        presigned,
        body as unknown as Uint8Array<ArrayBuffer>,
      ),
    ).rejects.toMatchObject({ status: 429 });
    expect(mock.requests).toHaveLength(1);
  });

  it("does not retry a network failure", async () => {
    const mock = mockFetch(() => {
      throw new TypeError("network unavailable");
    });
    await expect(
      createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch }).transfer(presigned, bytes()),
    ).rejects.toThrow("network unavailable");
    expect(mock.requests).toHaveLength(1);
  });
});
