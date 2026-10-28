// The /v1 client cloud mode uses: the SDK client, plus the operations that
// only newer API contracts define. `GET /v1/limits` and `DELETE
// /v1/uploads/{id}` are detected by whether the SDK generated from this
// checkout's OpenAPI document exports them, so the UI works against both an
// older and a newer server contract.
import * as sdk from "@ariadshift/sdk";
import {
  type AriadClient,
  type Capabilities,
  capabilities as capabilitiesOperation,
  createAriadClient,
  errorFromBody,
} from "@ariadshift/sdk";

/** One allowance of the caller's plan (`GET /v1/limits`). */
export interface Quota {
  limit?: number | null;
  remaining?: number | null;
  used: number;
}

/** The caller's plan limits, quotas and retention (`GET /v1/limits`). */
export interface PlanLimits {
  plan: string;
  authenticated: boolean;
  retention_s: number;
  requests_per_minute?: number | null;
  limits: {
    max_input_bytes?: number | null;
    max_memory_mb?: number | null;
    max_pages?: number | null;
    timeout_s?: number | null;
  };
  quotas: {
    uploads_per_day: Quota;
    jobs_per_day: Quota;
    input_bytes_per_day: Quota;
    pending_uploads: Quota;
    concurrent_jobs: Quota;
  };
}

type OperationResult = { data?: unknown; error?: unknown; response?: Response };
type Operation = (options: {
  client: AriadClient["raw"];
  path?: Record<string, string>;
  signal?: AbortSignal;
}) => Promise<OperationResult>;

/** A generated operation by its id, when this SDK build has it. */
export function findOperation(
  name: string,
  operations: Record<string, unknown> = sdk as unknown as Record<string, unknown>,
): Operation | undefined {
  const operation = operations[name];
  return typeof operation === "function" ? (operation as Operation) : undefined;
}

/** The SDK's own unwrapping: data on success, an `AriadApiError` otherwise. */
async function unwrap<T>(pending: Promise<OperationResult>): Promise<T> {
  const { data, error, response } = await pending;
  if (response === undefined) {
    throw error instanceof Error ? error : new Error(String(error));
  }
  if (!response.ok || error !== undefined) {
    throw errorFromBody(response, error);
  }
  return data as T;
}

/** A connection attempt of a job's event stream, for the "Reconnecting…" state. */
export interface EventConnection {
  jobId: string;
  /** 1 for the first connection, higher for reconnections. */
  attempt: number;
  state: "connecting" | "open" | "failed";
}

export interface CloudApi {
  readonly client: AriadClient;
  /** Whether the server contract has `GET /v1/limits`. */
  readonly supportsLimits: boolean;
  /** Whether the server contract has `DELETE /v1/uploads/{id}`. */
  readonly supportsUploadDelete: boolean;
  /** The caller's plan limits, or `null` when the contract has no limits endpoint. */
  limits(signal?: AbortSignal): Promise<PlanLimits | null>;
  /** Deletes an upload no job uses; returns `false` when the contract cannot. */
  deleteUpload(uploadId: string, signal?: AbortSignal): Promise<boolean>;
  capabilities(signal?: AbortSignal): Promise<Capabilities>;
  /** Observes event-stream connections; returns the unsubscribe function. */
  onEventConnection(listener: (connection: EventConnection) => void): () => void;
}

export interface CloudApiOptions {
  baseUrl: string;
  /** The current JWT, or `undefined` for anonymous requests. */
  bearer?: () => Promise<string | undefined>;
  fetch?: typeof fetch;
  /** Generated operations to detect optional endpoints in; the SDK by default. */
  operations?: Record<string, unknown>;
}

const EVENTS_PATH = /\/v1\/jobs\/([^/]+)\/events$/;

function requestUrl(input: RequestInfo | URL): string {
  if (typeof input === "string") {
    return input;
  }
  return input instanceof URL ? input.href : input.url;
}

export function createCloudApi(options: CloudApiOptions): CloudApi {
  const baseFetch: typeof fetch = options.fetch ?? ((input, init) => globalThis.fetch(input, init));
  const listeners = new Set<(connection: EventConnection) => void>();
  const attempts = new Map<string, number>();
  const emit = (connection: EventConnection) => {
    for (const listener of listeners) {
      listener(connection);
    }
  };

  // The SDK reconnects event streams on its own; counting the connections it
  // opens per job is how the UI learns that a stream dropped.
  const observingFetch: typeof fetch = async (input, init) => {
    const match = EVENTS_PATH.exec(new URL(requestUrl(input)).pathname);
    if (!match?.[1]) {
      return baseFetch(input, init);
    }
    const jobId = decodeURIComponent(match[1]);
    const attempt = (attempts.get(jobId) ?? 0) + 1;
    attempts.set(jobId, attempt);
    emit({ jobId, attempt, state: "connecting" });
    try {
      const response = await baseFetch(input, init);
      emit({ jobId, attempt, state: response.ok ? "open" : "failed" });
      return response;
    } catch (error) {
      emit({ jobId, attempt, state: "failed" });
      throw error;
    }
  };

  const client = createAriadClient({
    baseUrl: options.baseUrl,
    credentials: options.bearer ? { bearer: options.bearer } : undefined,
    fetch: observingFetch,
  });
  const getLimits = findOperation("getLimits", options.operations);
  const deleteUpload = findOperation("deleteUpload", options.operations);

  return {
    client,
    supportsLimits: getLimits !== undefined,
    supportsUploadDelete: deleteUpload !== undefined,

    async limits(signal) {
      if (!getLimits) {
        return null;
      }
      return unwrap<PlanLimits>(getLimits({ client: client.raw, signal }));
    },

    async deleteUpload(uploadId, signal) {
      if (!deleteUpload) {
        return false;
      }
      await unwrap<unknown>(deleteUpload({ client: client.raw, path: { id: uploadId }, signal }));
      return true;
    },

    capabilities(signal) {
      return unwrap<Capabilities>(capabilitiesOperation({ client: client.raw, signal }));
    },

    onEventConnection(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

/**
 * The document formats `from` reaches on this server, following the
 * capability edges through interchange formats. Interchange formats such as
 * `ariad-ir+json` are steps, not targets, so only listed formats are returned.
 */
export function cloudTargets(capabilities: Capabilities, from: string): string[] {
  const documents = new Set(capabilities.formats.map((format) => format.id));
  const next = new Map<string, string[]>();
  for (const edge of capabilities.edges) {
    next.set(edge.from, [...(next.get(edge.from) ?? []), edge.to]);
  }
  const seen = new Set([from]);
  const queue = [from];
  while (queue.length > 0) {
    const current = queue.shift() as string;
    for (const to of next.get(current) ?? []) {
      if (!seen.has(to)) {
        seen.add(to);
        queue.push(to);
      }
    }
  }
  seen.delete(from);
  return [...seen].filter((id) => documents.has(id));
}

/** The registry id of a file by its name, from the server's format list. */
export function formatFromName(capabilities: Capabilities, name: string): string | null {
  const lower = name.toLowerCase();
  let best: { id: string; length: number } | null = null;
  for (const format of capabilities.formats) {
    for (const extension of format.extensions) {
      const suffix = `.${extension.toLowerCase().replace(/^\./, "")}`;
      if (lower.endsWith(suffix) && (!best || suffix.length > best.length)) {
        best = { id: format.id, length: suffix.length };
      }
    }
  }
  return best?.id ?? null;
}
