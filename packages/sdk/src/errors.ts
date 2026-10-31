/** An error response from the AriadShift API or from a presigned storage URL. */
export class AriadApiError extends Error {
  override readonly name = "AriadApiError";

  constructor(
    /** HTTP status code. */
    readonly status: number,
    /** Stable machine-readable code from the error body, for example `not_found`. */
    readonly code: string,
    message: string,
    /** Safe whole seconds from a valid `Retry-After` header, when available. */
    readonly retryAfter?: number,
  ) {
    super(message);
  }
}

/** Reads `Retry-After` as whole seconds; HTTP dates are not used by the API. */
export function retryAfterSeconds(response: Response): number | undefined {
  const value = response.headers.get("retry-after")?.trim();
  // A safe integer has at most 16 decimal digits; reject oversized input first.
  if (value === undefined || value.length > 16 || !/^\d+$/.test(value)) {
    return undefined;
  }
  const seconds = Number(value);
  return Number.isSafeInteger(seconds) ? seconds : undefined;
}

/** Builds an error from a parsed `ErrorBody`, a raw body, or nothing. */
export function errorFromBody(
  response: Response,
  body: unknown,
  fallbackCode = "http_error",
): AriadApiError {
  const detail =
    typeof body === "object" && body !== null && "error" in body
      ? (body as { error?: { code?: unknown; message?: unknown } }).error
      : undefined;
  const code = typeof detail?.code === "string" ? detail.code : fallbackCode;
  const message =
    typeof detail?.message === "string"
      ? detail.message
      : `${response.status} ${response.statusText}`.trim();
  return new AriadApiError(response.status, code, message, retryAfterSeconds(response));
}

/** Reads an error response body as JSON when possible (storage may answer in XML). */
export async function errorFromResponse(
  response: Response,
  fallbackCode = "http_error",
): Promise<AriadApiError> {
  let body: unknown;
  try {
    const text = await response.text();
    body = text ? JSON.parse(text) : undefined;
  } catch {
    body = undefined;
  }
  return errorFromBody(response, body, fallbackCode);
}
