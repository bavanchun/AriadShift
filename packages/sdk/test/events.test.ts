import { describe, expect, it } from "vitest";
import { createAriadClient, type JobEvent, parseSse, type SseMessage } from "../src/index";
import {
  apiError,
  BASE_URL,
  eventMessage,
  JOB_ID,
  jobBody,
  json,
  mockFetch,
  sse,
  streamOf,
} from "./mock-fetch";

async function collect<T>(iterable: AsyncIterable<T>): Promise<T[]> {
  const items: T[] = [];
  for await (const item of iterable) {
    items.push(item);
  }
  return items;
}

const kinds = (events: JobEvent[]) => events.map((event) => `${event.id}:${event.kind}`);
const fast = { retryDelayMs: 1, maxRetryDelayMs: 2 };

describe("parseSse", () => {
  it("parses fields, comments, multi-line data and every line ending", async () => {
    const messages = await collect(
      parseSse(
        streamOf(
          ": keep-alive\r\n",
          "id: 1\r\nevent: progress\r\ndata: {\"a\":1}\r\n\r\n",
          "data: line one\rdata:line two\r\rretry: 2500\n",
          "event: warning\ndata: x\n\n",
        ),
      ),
    );

    expect(messages).toEqual<SseMessage[]>([
      { event: "progress", data: '{"a":1}', id: "1" },
      { event: "message", data: "line one\nline two", id: "1" },
      { event: "warning", data: "x", id: "1", retry: 2500 },
    ]);
  });

  it("handles fields and CRLF pairs split across chunks", async () => {
    const messages = await collect(
      parseSse(streamOf("id: 4", "2\r", "\nda", "ta: {}\r", "\n\r", "\n")),
    );

    expect(messages).toEqual([{ event: "message", data: "{}", id: "42" }]);
  });

  it("drops a final message that never ended and messages without data", async () => {
    const messages = await collect(parseSse(streamOf("id: 7\n\n", "data: partial")));

    expect(messages).toEqual([]);
  });
});

describe("events", () => {
  it("yields events until the final one and sends credentials", async () => {
    const mock = mockFetch(() =>
      sse(
        eventMessage(1, "queued"),
        ": keep-alive\n\n",
        eventMessage(2, "started"),
        eventMessage(3, "progress", { stage: "write" }),
        eventMessage(4, "succeeded"),
        eventMessage(5, "progress"),
      ),
    );
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { bearer: "jwt" },
      fetch: mock.fetch,
    });

    const events = await collect(client.events(JOB_ID));

    expect(kinds(events)).toEqual(["1:queued", "2:started", "3:progress", "4:succeeded"]);
    expect(events[2]?.data).toEqual({ stage: "write" });
    const request = mock.requests[0];
    expect(request?.url).toBe(`${BASE_URL}/v1/jobs/${JOB_ID}/events`);
    expect(request?.headers.get("authorization")).toBe("Bearer jwt");
    expect(request?.headers.get("accept")).toBe("text/event-stream");
    expect(request?.headers.get("last-event-id")).toBeNull();
  });

  it("resumes with Last-Event-ID after the stream closes early", async () => {
    const mock = mockFetch(
      () => sse(eventMessage(1, "queued"), eventMessage(2, "started")),
      () => json(jobBody("running")),
      () => sse(eventMessage(3, "progress"), eventMessage(4, "failed")),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    const events = await collect(client.events(JOB_ID, fast));

    expect(kinds(events)).toEqual(["1:queued", "2:started", "3:progress", "4:failed"]);
    expect(mock.requests[1]?.url).toBe(`${BASE_URL}/v1/jobs/${JOB_ID}`);
    expect(mock.requests[2]?.headers.get("last-event-id")).toBe("2");
  });

  it("starts after a caller-supplied event id", async () => {
    const mock = mockFetch(() => sse(eventMessage(10, "canceled")));
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    const events = await collect(client.events(JOB_ID, { lastEventId: 9 }));

    expect(kinds(events)).toEqual(["10:canceled"]);
    expect(mock.requests[0]?.headers.get("last-event-id")).toBe("9");
  });

  it("ends quietly when the job finished before this connection", async () => {
    const mock = mockFetch(
      () => sse(),
      () => json(jobBody("succeeded")),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    expect(await collect(client.events(JOB_ID, { lastEventId: 4 }))).toEqual([]);
    expect(mock.pending()).toBe(0);
  });

  it("retries rate limits and network failures", async () => {
    const mock = mockFetch(
      () => apiError(429, "rate_limited", "too many streams", { "retry-after": "0" }),
      () => {
        throw new TypeError("fetch failed");
      },
      () => apiError(503, "unavailable", "identity verification is unavailable"),
      () => sse(eventMessage(1, "expired")),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    const events = await collect(client.events(JOB_ID, fast));

    expect(kinds(events)).toEqual(["1:expired"]);
    expect(mock.requests).toHaveLength(4);
  });

  it("does not retry a missing job or rejected credentials", async () => {
    const missing = mockFetch(() => apiError(404, "not_found", "no such job"));
    await expect(
      collect(createAriadClient({ baseUrl: BASE_URL, fetch: missing.fetch }).events(JOB_ID, fast)),
    ).rejects.toMatchObject({ status: 404, code: "not_found" });

    const rejected = mockFetch(() => apiError(401, "unauthorized", "credentials rejected"));
    await expect(
      collect(
        createAriadClient({
          baseUrl: BASE_URL,
          credentials: { apiKey: "revoked" },
          fetch: rejected.fetch,
        }).events(JOB_ID, fast),
      ),
    ).rejects.toMatchObject({ status: 401 });
  });

  it("gives up after maxRetries consecutive failures", async () => {
    const unavailable = () => apiError(503, "unavailable", "down");
    const mock = mockFetch(unavailable, unavailable, unavailable);
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(
      collect(client.events(JOB_ID, { ...fast, maxRetries: 2 })),
    ).rejects.toMatchObject({ status: 503 });
    expect(mock.requests).toHaveLength(3);
  });

  it("stops when the caller aborts", async () => {
    const controller = new AbortController();
    const mock = mockFetch(() => sse(eventMessage(1, "queued"), eventMessage(2, "started")));
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    const seen: JobEvent[] = [];
    const run = (async () => {
      for await (const event of client.events(JOB_ID, { ...fast, signal: controller.signal })) {
        seen.push(event);
        controller.abort();
      }
    })();

    await expect(run).rejects.toMatchObject({ name: "AbortError" });
    expect(kinds(seen)).toEqual(["1:queued"]);
    expect(mock.requests).toHaveLength(1);
  });
});
