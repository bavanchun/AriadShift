// Hand-written layer over the generated /v1 client: credentials, the upload
// flow, job events with resumption, downloads and deletion. Requests to the API
// go through the generated operations; presigned URLs are followed as returned
// and never receive credentials, since with S3 storage they point at the object
// store rather than at `/v1/blobs`.
import { errorFromBody, errorFromResponse, retryAfterSeconds } from "./errors";
import {
  type CreateJobRequest,
  completeUpload as completeUploadOperation,
  createJob as createJobOperation,
  createUpload as createUploadOperation,
  deleteJob as deleteJobOperation,
  getJob as getJobOperation,
  type Job,
  type JobEvent,
  jobOutputs as jobOutputsOperation,
  type OutputFile,
  type Outputs,
  type PresignedRequest,
  type Upload,
} from "./generated";
import { type Client, createClient } from "./generated/client";
import { sha256Hex } from "./hash";
import { parseSse } from "./sse";

/** A credential value, or a function returning the current one (for refreshed tokens). */
export type Secret = string | (() => string | undefined | Promise<string | undefined>);

/** How the client authenticates. Omit it for anonymous use. */
export type Credentials =
  /** A JWT from the auth service, sent as `Authorization: Bearer`. */
  | { bearer: Secret }
  /** An API key from the auth service, sent as `x-api-key`. */
  | { apiKey: Secret };

export interface AriadClientOptions {
  /** The API origin, for example `https://api.example.com`. */
  baseUrl: string;
  credentials?: Credentials;
  /** A fetch implementation; defaults to the global `fetch`. */
  fetch?: typeof fetch;
}

/** File bodies the upload helper can send with a known length. */
export type UploadBody = Blob | ArrayBuffer | Uint8Array<ArrayBuffer>;

export interface UploadOptions {
  /** Original file name; defaults to `File.name`. */
  filename?: string;
  /** Lowercase hex SHA-256 of the body; computed in chunks when omitted. */
  sha256?: string;
  /** Called while the body is hashed, with the bytes hashed so far and the total. */
  onHashProgress?: (hashed: number, total: number) => void;
  /** Cloudflare Turnstile token for anonymous callers, when the deployment requires one. */
  turnstileToken?: string;
  signal?: AbortSignal;
}

export interface CreateJobOptions {
  uploadId: string;
  /** Target format id or extension, for example `docx`. */
  to: string;
  profile?: CreateJobRequest["profile"];
  signal?: AbortSignal;
}

export interface JobEventsOptions {
  /** Resume after this event id instead of replaying from the start. */
  lastEventId?: number | string;
  signal?: AbortSignal;
  /** Consecutive failed connections before giving up. Default 8. */
  maxRetries?: number;
  /** First reconnection delay; doubles per consecutive failure. Default 1000 ms. */
  retryDelayMs?: number;
  /** Upper bound of the reconnection delay. Default 30000 ms. */
  maxRetryDelayMs?: number;
}

export interface DownloadOptions {
  signal?: AbortSignal;
  /** Check the SHA-256 of the downloaded bytes. The size is always checked. */
  verifySha256?: boolean;
}

/** Event kinds a job emits, in the order they can occur. */
export type JobEventKind =
  | "queued"
  | "started"
  | "progress"
  | "warning"
  | "retrying"
  | "succeeded"
  | "failed"
  | "canceled"
  | "expired";

/** Kinds after which a job's event stream ends. */
export const FINAL_JOB_EVENT_KINDS: ReadonlySet<string> = new Set<JobEventKind>([
  "succeeded",
  "failed",
  "canceled",
  "expired",
]);

const FINAL_JOB_STATUSES: ReadonlySet<string> = FINAL_JOB_EVENT_KINDS;

/** Responses worth retrying when following events. */
const RETRYABLE_STATUSES = new Set([429, 500, 502, 503, 504]);

export interface AriadClient {
  /** The generated client, for calling any generated operation directly. */
  readonly raw: Client;
  /** Starts an upload: returns the pending upload and its presigned request. */
  createUpload(
    request: { filename: string; size: number; sha256: string },
    options?: Pick<UploadOptions, "turnstileToken" | "signal">,
  ): Promise<Upload>;
  /** Sends a body to the presigned request of a pending upload. */
  transfer(presigned: PresignedRequest, body: UploadBody, signal?: AbortSignal): Promise<void>;
  /** Verifies a transferred upload's size and hash. */
  completeUpload(uploadId: string, signal?: AbortSignal): Promise<Upload>;
  /** Creates the upload, transfers the body and completes it. */
  upload(body: UploadBody, options?: UploadOptions): Promise<Upload>;
  createJob(options: CreateJobOptions): Promise<Job>;
  getJob(jobId: string, signal?: AbortSignal): Promise<Job>;
  /**
   * Follows a job's events until its final event. Reconnects after dropped
   * connections with `Last-Event-ID`, so no event is lost or repeated.
   */
  events(jobId: string, options?: JobEventsOptions): AsyncGenerator<JobEvent, void, undefined>;
  /** Lists a succeeded job's outputs with fresh presigned URLs (valid five minutes). */
  outputs(jobId: string, signal?: AbortSignal): Promise<Outputs>;
  /** Downloads an output through its presigned URL. */
  download(
    output: Pick<OutputFile, "url" | "size" | "sha256">,
    options?: DownloadOptions,
  ): Promise<Blob>;
  /** Opens an output's presigned URL as a byte stream, for large files. */
  downloadStream(
    output: Pick<OutputFile, "url">,
    signal?: AbortSignal,
  ): Promise<ReadableStream<Uint8Array>>;
  /** Cancels the job if it is still running and deletes its input and outputs now. */
  deleteJob(jobId: string, signal?: AbortSignal): Promise<void>;
}

type OperationResult<T> = { data?: T; error?: unknown; response?: Response };

/** Returns the data of a generated operation's result, or throws its error. */
async function unwrap<T>(pending: Promise<OperationResult<T>>): Promise<T> {
  const { data, error, response } = await pending;
  if (response === undefined) {
    // The request never got a response (network failure or abort).
    throw error instanceof Error ? error : new Error(String(error));
  }
  if (!response.ok || error !== undefined) {
    throw errorFromBody(response, error);
  }
  return data as T;
}

async function resolveSecret(secret: Secret): Promise<string | undefined> {
  return typeof secret === "function" ? await secret() : secret;
}

/** The headers that carry `credentials`, resolved for one request. */
async function authHeaders(credentials: Credentials | undefined): Promise<Record<string, string>> {
  if (credentials === undefined) {
    return {};
  }
  if ("bearer" in credentials) {
    const token = await resolveSecret(credentials.bearer);
    return token ? { authorization: `Bearer ${token}` } : {};
  }
  const key = await resolveSecret(credentials.apiKey);
  return key ? { "x-api-key": key } : {};
}

function bodySize(body: UploadBody): number {
  return body instanceof Blob ? body.size : body.byteLength;
}

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(signal.reason);
      return;
    }
    const onAbort = () => {
      clearTimeout(timer);
      reject(signal?.reason);
    };
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}

/** Creates a client for one AriadShift API deployment. */
export function createAriadClient(options: AriadClientOptions): AriadClient {
  const baseUrl = options.baseUrl.replace(/\/+$/, "");
  const credentials = options.credentials;
  // Resolve the global lazily so it is called with the right `this` and can be replaced.
  const fetchImpl: typeof fetch = options.fetch ?? ((input, init) => globalThis.fetch(input, init));

  const raw = createClient({ baseUrl, fetch: fetchImpl });
  raw.interceptors.request.use(async (request) => {
    for (const [name, value] of Object.entries(await authHeaders(credentials))) {
      request.headers.set(name, value);
    }
    return request;
  });

  const client: AriadClient = {
    raw,

    createUpload(request, uploadOptions = {}) {
      const turnstile = uploadOptions.turnstileToken;
      return unwrap<Upload>(
        createUploadOperation({
          client: raw,
          body: request,
          ...(turnstile ? { headers: { "cf-turnstile-response": turnstile } } : {}),
          signal: uploadOptions.signal,
        }),
      );
    },

    async transfer(presigned, body, signal) {
      const response = await fetchImpl(presigned.url, {
        method: presigned.method,
        headers: presigned.headers,
        body,
        signal,
      });
      if (!response.ok) {
        throw await errorFromResponse(response, "transfer_failed");
      }
      await response.body?.cancel();
    },

    completeUpload(uploadId, signal) {
      return unwrap<Upload>(
        completeUploadOperation({ client: raw, path: { id: uploadId }, signal }),
      );
    },

    async upload(body, uploadOptions = {}) {
      const filename = uploadOptions.filename ?? (body instanceof File ? body.name : undefined);
      if (!filename) {
        throw new TypeError("upload needs a filename for bodies that are not a File");
      }
      const sha256 =
        uploadOptions.sha256 ??
        (await sha256Hex(body, {
          signal: uploadOptions.signal,
          onProgress: uploadOptions.onHashProgress,
        }));
      const created = await client.createUpload(
        { filename, size: bodySize(body), sha256 },
        uploadOptions,
      );
      if (!created.upload) {
        throw new Error(`upload ${created.id} has no presigned request (status ${created.status})`);
      }
      await client.transfer(created.upload, body, uploadOptions.signal);
      return client.completeUpload(created.id, uploadOptions.signal);
    },

    createJob({ uploadId, to, profile, signal }) {
      const body: CreateJobRequest = { upload_id: uploadId, to };
      if (profile !== undefined) {
        body.profile = profile;
      }
      return unwrap<Job>(createJobOperation({ client: raw, body, signal }));
    },

    getJob(jobId, signal) {
      return unwrap<Job>(getJobOperation({ client: raw, path: { id: jobId }, signal }));
    },

    async *events(jobId, eventOptions = {}) {
      const { signal } = eventOptions;
      const maxRetries = eventOptions.maxRetries ?? 8;
      const maxDelay = eventOptions.maxRetryDelayMs ?? 30_000;
      let baseDelay = eventOptions.retryDelayMs ?? 1_000;
      let lastEventId =
        eventOptions.lastEventId === undefined ? undefined : String(eventOptions.lastEventId);
      let failures = 0;
      const url = `${baseUrl}/v1/jobs/${encodeURIComponent(jobId)}/events`;

      // Waits before the next connection, or throws once retries are exhausted.
      const backOff = async (cause: unknown, retryAfter?: number) => {
        failures += 1;
        if (failures > maxRetries) {
          throw cause;
        }
        const delay =
          retryAfter !== undefined
            ? retryAfter * 1_000
            : Math.min(baseDelay * 2 ** (failures - 1), maxDelay);
        await sleep(delay, signal);
      };

      for (;;) {
        signal?.throwIfAborted();
        const headers: Record<string, string> = {
          accept: "text/event-stream",
          ...(await authHeaders(credentials)),
        };
        if (lastEventId !== undefined) {
          headers["last-event-id"] = lastEventId;
        }

        let response: Response;
        try {
          response = await fetchImpl(url, { headers, signal });
        } catch (error) {
          if (signal?.aborted) {
            throw error;
          }
          await backOff(error);
          continue;
        }

        if (!response.ok) {
          const error = await errorFromResponse(response);
          if (!RETRYABLE_STATUSES.has(response.status)) {
            throw error;
          }
          await backOff(error, retryAfterSeconds(response));
          continue;
        }
        if (response.body === null) {
          await backOff(new Error("the event stream has no body"));
          continue;
        }

        try {
          for await (const message of parseSse(response.body)) {
            if (message.retry !== undefined) {
              baseDelay = message.retry;
            }
            const event = JSON.parse(message.data) as JobEvent;
            lastEventId = message.id ?? String(event.id);
            failures = 0;
            yield event;
            if (FINAL_JOB_EVENT_KINDS.has(event.kind)) {
              return;
            }
            signal?.throwIfAborted();
          }
        } catch (error) {
          if (signal?.aborted || error instanceof SyntaxError) {
            throw error;
          }
          await backOff(error);
          continue;
        }

        // The server closed the stream before a final event. It does that when
        // the job ended before this connection (every event already delivered)
        // or when it shuts down; only the job's status tells them apart.
        const job = await client.getJob(jobId, signal);
        if (FINAL_JOB_STATUSES.has(job.status)) {
          return;
        }
        await backOff(new Error("the event stream closed before the job finished"));
      }
    },

    outputs(jobId, signal) {
      return unwrap<Outputs>(jobOutputsOperation({ client: raw, path: { id: jobId }, signal }));
    },

    async download(output, downloadOptions = {}) {
      const response = await fetchImpl(output.url, { signal: downloadOptions.signal });
      if (!response.ok) {
        throw await errorFromResponse(response, "download_failed");
      }
      const blob = await response.blob();
      if (blob.size !== output.size) {
        throw new Error(`downloaded ${blob.size} bytes, expected ${output.size}`);
      }
      if (
        downloadOptions.verifySha256 &&
        (await sha256Hex(blob, { signal: downloadOptions.signal })) !== output.sha256.toLowerCase()
      ) {
        throw new Error("the downloaded file does not match its SHA-256");
      }
      return blob;
    },

    async downloadStream(output, signal) {
      const response = await fetchImpl(output.url, { signal });
      if (!response.ok) {
        throw await errorFromResponse(response, "download_failed");
      }
      if (response.body === null) {
        throw new Error("the download has no body");
      }
      return response.body;
    },

    async deleteJob(jobId, signal) {
      await unwrap<unknown>(deleteJobOperation({ client: raw, path: { id: jobId }, signal }));
    },
  };
  return client;
}
