import { describe, expect, it } from "vitest";
import { AriadApiError, createAriadClient, sha256Hex } from "../src/index";
import {
  apiError,
  BASE_URL,
  JOB_ID,
  jobBody,
  json,
  mockFetch,
  UPLOAD_ID,
  uploadBody,
} from "./mock-fetch";

const HELLO_SHA256 = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
const hello = () => new TextEncoder().encode("hello") as Uint8Array<ArrayBuffer>;

describe("credentials", () => {
  it("sends a bearer token from a provider on every API request", async () => {
    let calls = 0;
    const mock = mockFetch(
      () => json(jobBody("running")),
      () => json(jobBody("succeeded")),
    );
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { bearer: () => `token-${++calls}` },
      fetch: mock.fetch,
    });

    expect((await client.getJob(JOB_ID)).status).toBe("running");
    expect((await client.getJob(JOB_ID)).status).toBe("succeeded");

    expect(mock.requests.map((r) => r.headers.get("authorization"))).toEqual([
      "Bearer token-1",
      "Bearer token-2",
    ]);
    expect(mock.requests[0]?.url).toBe(`${BASE_URL}/v1/jobs/${JOB_ID}`);
    expect(mock.requests[0]?.headers.get("x-api-key")).toBeNull();
  });

  it("sends an API key as x-api-key", async () => {
    const mock = mockFetch(() => json(jobBody("queued")));
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { apiKey: "ak_test" },
      fetch: mock.fetch,
    });

    await client.getJob(JOB_ID);

    expect(mock.requests[0]?.headers.get("x-api-key")).toBe("ak_test");
    expect(mock.requests[0]?.headers.get("authorization")).toBeNull();
  });

  it("sends no credentials for anonymous use or an empty provider", async () => {
    const mock = mockFetch(
      () => json(jobBody("queued")),
      () => json(jobBody("queued")),
    );
    await createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch }).getJob(JOB_ID);
    await createAriadClient({
      baseUrl: BASE_URL,
      credentials: { bearer: async () => undefined },
      fetch: mock.fetch,
    }).getJob(JOB_ID);

    for (const request of mock.requests) {
      expect(request.headers.get("authorization")).toBeNull();
      expect(request.headers.get("x-api-key")).toBeNull();
    }
  });

  it("ignores trailing slashes in the base URL", async () => {
    const mock = mockFetch(() => json(jobBody("queued")));
    await createAriadClient({ baseUrl: `${BASE_URL}//`, fetch: mock.fetch }).getJob(JOB_ID);
    expect(mock.requests[0]?.url).toBe(`${BASE_URL}/v1/jobs/${JOB_ID}`);
  });
});

describe("errors", () => {
  it("turns an error body into an AriadApiError with Retry-After", async () => {
    const mock = mockFetch(() =>
      apiError(429, "rate_limited", "too many requests", { "retry-after": "7" }),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    const error = await client.getJob(JOB_ID).catch((e: unknown) => e);

    expect(error).toBeInstanceOf(AriadApiError);
    expect(error).toMatchObject({
      status: 429,
      code: "rate_limited",
      message: "too many requests",
      retryAfter: 7,
    });
  });

  it("keeps the status when the error body is not JSON", async () => {
    const mock = mockFetch(() => new Response("upstream down", { status: 502 }));
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(client.getJob(JOB_ID)).rejects.toMatchObject({
      status: 502,
      code: "http_error",
    });
  });

  it("rethrows network failures unchanged", async () => {
    const failure = new TypeError("fetch failed");
    const client = createAriadClient({
      baseUrl: BASE_URL,
      fetch: async () => {
        throw failure;
      },
    });

    await expect(client.getJob(JOB_ID)).rejects.toBe(failure);
  });
});

describe("upload", () => {
  it("creates, PUTs to the returned URL without credentials, then completes", async () => {
    const mock = mockFetch(
      () => json(uploadBody("pending"), 201),
      () => new Response(null, { status: 200 }),
      () => json(uploadBody("completed")),
    );
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { bearer: "jwt" },
      fetch: mock.fetch,
    });

    const upload = await client.upload(hello(), {
      filename: "report.md",
      turnstileToken: "turnstile-token",
    });

    expect(upload).toMatchObject({ id: UPLOAD_ID, status: "completed", format: "md" });
    const [create, put, complete] = mock.requests;

    expect(create?.method).toBe("POST");
    expect(create?.url).toBe(`${BASE_URL}/v1/uploads`);
    expect(create?.headers.get("authorization")).toBe("Bearer jwt");
    expect(create?.headers.get("cf-turnstile-response")).toBe("turnstile-token");
    expect(await create?.json()).toEqual({ filename: "report.md", size: 5, sha256: HELLO_SHA256 });

    expect(put?.method).toBe("PUT");
    expect(put?.url).toBe("https://storage.example.test/bucket/inputs/abc?X-Amz-Signature=sig");
    expect(put?.headers.get("content-type")).toBe("application/octet-stream");
    expect(put?.headers.get("authorization")).toBeNull();
    expect(put?.headers.get("x-api-key")).toBeNull();
    expect(await put?.text()).toBe("hello");

    expect(complete?.method).toBe("POST");
    expect(complete?.url).toBe(`${BASE_URL}/v1/uploads/${UPLOAD_ID}/complete`);
    expect(complete?.headers.get("authorization")).toBe("Bearer jwt");
  });

  it("takes the filename from a File, reports hashing and sends no Turnstile header", async () => {
    const mock = mockFetch(
      () => json(uploadBody("pending"), 201),
      () => new Response(null, { status: 200 }),
      () => json(uploadBody("completed")),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });
    const hashed: Array<[number, number]> = [];

    await client.upload(new File(["hello"], "notes.md"), {
      onHashProgress: (done, total) => hashed.push([done, total]),
    });

    const create = mock.requests[0];
    expect(create?.headers.get("cf-turnstile-response")).toBeNull();
    expect(await create?.json()).toEqual({ filename: "notes.md", size: 5, sha256: HELLO_SHA256 });
    expect(hashed).toEqual([[5, 5]]);
  });

  it("requires a filename for bodies that are not a File", async () => {
    const mock = mockFetch();
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(client.upload(hello())).rejects.toThrow(TypeError);
    expect(mock.requests).toHaveLength(0);
  });

  it("reports a refused transfer and does not complete the upload", async () => {
    const mock = mockFetch(
      () => json(uploadBody("pending"), 201),
      () => apiError(403, "forbidden", "the URL is invalid or has expired"),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(
      client.upload(hello(), { filename: "report.md", sha256: HELLO_SHA256 }),
    ).rejects.toMatchObject({ status: 403, code: "forbidden" });
    expect(mock.requests).toHaveLength(3);
  });

  it("falls back to transfer_failed for non-JSON storage errors", async () => {
    const mock = mockFetch(
      () => new Response("<Error>SignatureDoesNotMatch</Error>", { status: 403 }),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });
    const presigned = uploadBody("pending").upload;
    if (!presigned) throw new Error("fixture has no presigned request");

    await expect(client.transfer(presigned, hello())).rejects.toMatchObject({
      status: 403,
      code: "transfer_failed",
    });
  });
});

describe("jobs", () => {
  it("creates a job from a completed upload", async () => {
    const mock = mockFetch(() => json(jobBody("queued"), 202));
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    const job = await client.createJob({ uploadId: UPLOAD_ID, to: "docx", profile: "editable" });

    expect(job.id).toBe(JOB_ID);
    expect(mock.requests[0]?.method).toBe("POST");
    expect(mock.requests[0]?.url).toBe(`${BASE_URL}/v1/jobs`);
    expect(await mock.requests[0]?.json()).toEqual({
      upload_id: UPLOAD_ID,
      to: "docx",
      profile: "editable",
    });
  });

  it("deletes a job", async () => {
    const mock = mockFetch(() => new Response(null, { status: 204 }));
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(client.deleteJob(JOB_ID)).resolves.toBeUndefined();
    expect(mock.requests[0]?.method).toBe("DELETE");
    expect(mock.requests[0]?.url).toBe(`${BASE_URL}/v1/jobs/${JOB_ID}`);
  });

  it("surfaces a refused delete", async () => {
    const mock = mockFetch(() => apiError(404, "not_found", "no such job"));
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(client.deleteJob(JOB_ID)).rejects.toMatchObject({
      status: 404,
      code: "not_found",
    });
  });
});

describe("download", () => {
  const output = {
    id: "1d2c3b4a-0000-4000-8000-000000000001",
    filename: "report.docx",
    format: "docx",
    media_type: "application/octet-stream",
    size: 5,
    sha256: HELLO_SHA256,
    url: "https://storage.example.test/bucket/outputs/abc?X-Amz-Signature=sig",
    expires_at: "2026-10-25T10:05:00Z",
  };

  it("lists outputs and downloads one without credentials", async () => {
    const mock = mockFetch(
      () => json({ job_id: JOB_ID, outputs: [output] }),
      () => new Response("hello"),
    );
    const client = createAriadClient({
      baseUrl: BASE_URL,
      credentials: { apiKey: "ak_test" },
      fetch: mock.fetch,
    });

    const { outputs } = await client.outputs(JOB_ID);
    const first = outputs[0];
    if (!first) throw new Error("no output");
    const blob = await client.download(first, { verifySha256: true });

    expect(await blob.text()).toBe("hello");
    expect(mock.requests[0]?.url).toBe(`${BASE_URL}/v1/jobs/${JOB_ID}/outputs`);
    expect(mock.requests[0]?.headers.get("x-api-key")).toBe("ak_test");
    expect(mock.requests[1]?.url).toBe(output.url);
    expect(mock.requests[1]?.headers.get("x-api-key")).toBeNull();
  });

  it("rejects a truncated or altered download", async () => {
    const mock = mockFetch(
      () => new Response("hell"),
      () => new Response("jello"),
    );
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(client.download(output)).rejects.toThrow(/expected 5/);
    await expect(client.download(output, { verifySha256: true })).rejects.toThrow(/SHA-256/);
  });

  it("reports an expired download URL", async () => {
    const mock = mockFetch(() => apiError(403, "forbidden", "the URL is invalid or has expired"));
    const client = createAriadClient({ baseUrl: BASE_URL, fetch: mock.fetch });

    await expect(client.downloadStream(output)).rejects.toMatchObject({
      status: 403,
      code: "forbidden",
    });
  });
});

describe("sha256Hex", () => {
  it("hashes blobs and byte arrays alike", async () => {
    expect(await sha256Hex(hello())).toBe(HELLO_SHA256);
    expect(await sha256Hex(new Blob(["hello"]))).toBe(HELLO_SHA256);
  });
});
