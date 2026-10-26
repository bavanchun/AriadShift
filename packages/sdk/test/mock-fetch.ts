// A scripted fetch: each call takes the next handler and is recorded.

export type Handler = (request: Request) => Response | Promise<Response>;

export interface MockFetch {
  fetch: typeof fetch;
  /** Every request made, in order. */
  requests: Request[];
  /** Handlers not used yet. */
  pending: () => number;
}

export function mockFetch(...handlers: Handler[]): MockFetch {
  const queue = [...handlers];
  const requests: Request[] = [];
  const fetchImpl = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const request = input instanceof Request ? input : new Request(input, init);
    requests.push(request);
    const handler = queue.shift();
    if (handler === undefined) {
      throw new Error(`unexpected request ${request.method} ${request.url}`);
    }
    return handler(request);
  };
  return { fetch: fetchImpl as typeof fetch, requests, pending: () => queue.length };
}

export function json(body: unknown, status = 200, headers: Record<string, string> = {}): Response {
  return Response.json(body, { status, headers });
}

export function apiError(status: number, code: string, message: string, headers = {}): Response {
  return json({ error: { code, message } }, status, headers);
}

/** A `text/event-stream` response delivering `chunks` one by one. */
export function sse(...chunks: string[]): Response {
  const encoder = new TextEncoder();
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) {
        controller.enqueue(encoder.encode(chunk));
      }
      controller.close();
    },
  });
  return new Response(body, { headers: { "content-type": "text/event-stream" } });
}

/** A readable stream of `chunks`, for the parser tests. */
export function streamOf(...chunks: string[]): ReadableStream<Uint8Array> {
  const body = sse(...chunks).body;
  if (body === null) {
    throw new Error("no body");
  }
  return body;
}

export const BASE_URL = "https://api.example.test";
export const UPLOAD_ID = "6f1c1f5e-1b0e-4c39-9d43-2d1f0c9a1a01";
export const JOB_ID = "0b6a3c55-8f0e-4b8a-a2b0-3f8d5c7e9a12";

export function uploadBody(status: "pending" | "completed", overrides: object = {}) {
  return {
    id: UPLOAD_ID,
    status,
    filename: "report.md",
    size: 5,
    sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
    created_at: "2026-10-25T10:00:00Z",
    expires_at: "2026-10-25T11:00:00Z",
    format: status === "completed" ? "md" : null,
    upload:
      status === "pending"
        ? {
            method: "PUT",
            url: "https://storage.example.test/bucket/inputs/abc?X-Amz-Signature=sig",
            headers: { "content-type": "application/octet-stream" },
            expires_at: "2026-10-25T10:15:00Z",
          }
        : null,
    ...overrides,
  };
}

export function jobBody(status: string) {
  return {
    id: JOB_ID,
    status,
    upload_id: UPLOAD_ID,
    from: "md",
    to: "docx",
    profile: "faithful",
    pack: "core",
    route: [],
    warnings: [],
    attempts: 1,
    created_at: "2026-10-25T10:00:00Z",
    expires_at: "2026-10-25T11:00:00Z",
  };
}

/** One SSE message carrying a `JobEvent`. */
export function eventMessage(id: number, kind: string, data: object = {}): string {
  const event = { id, job_id: JOB_ID, kind, data, created_at: "2026-10-25T10:00:00Z" };
  return `id: ${id}\nevent: ${kind}\ndata: ${JSON.stringify(event)}\n\n`;
}
