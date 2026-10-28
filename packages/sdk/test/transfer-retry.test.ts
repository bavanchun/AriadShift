import { afterEach, describe, expect, it, vi } from "vitest";
import { createAriadClient } from "../src/index";
import { apiError, BASE_URL, mockFetch, uploadBody } from "./mock-fetch";

const presigned = uploadBody("pending").upload;
if (!presigned) throw new Error("fixture has no presigned request");
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
