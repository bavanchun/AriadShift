// HTTP layer: CORS, a body limit, and the client address Better Auth
// rate-limits by. Everything under /api/auth goes to Better Auth. The OAuth
// discovery documents and the sign-in and consent pages are served here too,
// outside Better Auth's base path.

import { type Context, Hono, type MiddlewareHandler } from "hono";
import { bodyLimit } from "hono/body-limit";
import { cors } from "hono/cors";
import { BASE_PATH, CLIENT_IP_HEADER, OAUTH_CONSENT_PATH, OAUTH_SIGN_IN_PATH } from "./auth.ts";
import type { AuthServiceConfig } from "./config.ts";
import { normalizeOAuthPath } from "./oauth-lock.ts";
import {
  ASSET_PATH,
  ASSETS,
  consentPage,
  PAGE_HEADERS,
  type PageOptions,
  signInPage,
} from "./pages.ts";

/** Largest accepted request body; auth payloads are small JSON documents. */
export const MAX_BODY_BYTES = 64 * 1024;

/** RFC 8414 and OpenID Connect discovery documents, at the issuer's root. */
export const DISCOVERY_DOCUMENTS = ["oauth-authorization-server", "openid-configuration"] as const;
export type DiscoveryDocument = (typeof DISCOVERY_DOCUMENTS)[number];

/** Same caching as Better Auth's own discovery responses. */
const METADATA_CACHE_CONTROL =
  "public, max-age=15, stale-while-revalidate=15, stale-if-error=86400";

/** The OAuth token endpoint. */
export const TOKEN_PATH = `${BASE_PATH}/oauth2/token`;

/** The RFC 7591 dynamic client registration endpoint. */
export const REGISTRATION_PATH = `${BASE_PATH}/oauth2/register`;

/**
 * Largest accepted client registration. Anyone may register without an
 * account and every registration is a stored row, so the body is capped far
 * below MAX_BODY_BYTES; real MCP client metadata is well under 2 KiB.
 */
export const MAX_REGISTRATION_BODY_BYTES = 8 * 1024;

/**
 * Endpoints OAuth clients call from other origins (MCP Inspector runs in a
 * browser). None of them reads the session cookie, so they get CORS for any
 * origin without credentials, while the rest of /api/auth stays limited to
 * the trusted web origins.
 */
export const PUBLIC_OAUTH_PATHS: ReadonlySet<string> = new Set([
  REGISTRATION_PATH,
  TOKEN_PATH,
  `${BASE_PATH}/oauth2/revoke`,
  `${BASE_PATH}/oauth2/userinfo`,
  `${BASE_PATH}/jwks`,
]);

export interface AppDeps {
  config: Pick<AuthServiceConfig, "trustedOrigins" | "trustedClientIpHeader"> &
    Partial<Pick<AuthServiceConfig, "oauthDisabledReason">>;
  handler: (request: Request) => Promise<Response>;
  /** Absent when remote MCP OAuth is disabled. */
  metadata?: (request: Request, document: DiscoveryDocument) => Promise<object>;
  pages: PageOptions;
  /** The socket's peer address, when the runtime exposes it. */
  peerAddress: (c: Context) => string | undefined;
}

/**
 * The address to rate-limit by: the single value of the trusted proxy header
 * when one is configured, else the socket peer.
 */
export function clientAddress(
  headers: Headers,
  peer: string | undefined,
  trustedHeader: string | undefined,
): string | undefined {
  if (trustedHeader) {
    const value = headers.get(trustedHeader)?.trim();
    // Several values (a list or repeated headers) mean the proxy did not
    // overwrite the header, so it is not trusted.
    if (value && !value.includes(",")) return value;
  }
  return peer;
}

/**
 * ariad-server accepts Bearer tokens only and does not verify DPoP proofs, so
 * DPoP is not advertised. A DPoP header sent to the token endpoint anyway is
 * dropped, and the client receives an ordinary Bearer token (RFC 9449 lets
 * an authorization server do that).
 */
export function withoutDpop(metadata: object): Record<string, unknown> {
  const { dpop_signing_alg_values_supported: _dpop, ...rest } = metadata as Record<string, unknown>;
  return rest;
}

/** Copies the request with the client-address header set to `address` only. */
function forwarded(request: Request, path: string, address: string | undefined): Request {
  const headers = new Headers(request.headers);
  headers.delete(CLIENT_IP_HEADER);
  if (address) headers.set(CLIENT_IP_HEADER, address);
  if (path === TOKEN_PATH) headers.delete("dpop");
  const hasBody = request.method !== "GET" && request.method !== "HEAD";
  return new Request(request.url, {
    method: request.method,
    headers,
    body: hasBody ? request.body : undefined,
    signal: request.signal,
    // Node's fetch needs this to stream a request body.
    ...(hasBody ? { duplex: "half" } : {}),
  } as RequestInit);
}

export function createApp({ config, handler, metadata, pages, peerAddress }: AppDeps): Hono {
  const app = new Hono();
  const routes = `${BASE_PATH}/*`;

  const sessionCors = cors({
    origin: config.trustedOrigins,
    credentials: true,
    allowMethods: ["GET", "POST", "OPTIONS"],
    allowHeaders: ["Content-Type", "Authorization", "X-Captcha-Response"],
    // The JWT plugin returns a token in this header on get-session.
    exposeHeaders: ["set-auth-jwt"],
    maxAge: 600,
  });
  const publicCors = cors({
    origin: "*",
    allowMethods: ["GET", "POST", "OPTIONS"],
    allowHeaders: ["Content-Type", "Authorization", "MCP-Protocol-Version"],
    maxAge: 600,
  });
  const corsByPath: MiddlewareHandler = (c, next) =>
    PUBLIC_OAUTH_PATHS.has(normalizeOAuthPath(c.req.path))
      ? publicCors(c, next)
      : sessionCors(c, next);

  app.use(routes, corsByPath);
  app.use(routes, async (c, next) => {
    // Reject registration aliases before body limits or the provider can
    // interpret them differently and bypass the exact-path cap.
    const path = normalizeOAuthPath(c.req.path);
    if (path === REGISTRATION_PATH && c.req.path !== path) {
      return c.json({ message: "not found" }, 404);
    }
    return next();
  });
  app.use(REGISTRATION_PATH, async (c, next) => {
    const request = c.req.raw;
    if (!request.body) return next();
    // RFC 7591 section 3.2.2 error shape, so OAuth clients can report it.
    const tooLarge = () =>
      c.json(
        {
          error: "invalid_client_metadata",
          error_description: `client metadata exceeds ${MAX_REGISTRATION_BODY_BYTES} bytes`,
        },
        413,
      );
    if (Number(request.headers.get("content-length")) > MAX_REGISTRATION_BODY_BYTES) {
      void request.body.cancel().catch(() => {});
      return tooLarge();
    }

    // Retained and forwarded bytes are capped at 8,192. Node HTTP/1 may
    // deliver a larger socket chunk in one read; drop it without copying
    // into this buffer and cancel the stream. Content-Length is only an
    // early rejection hint, never proof of size.
    const bytes = new Uint8Array(MAX_REGISTRATION_BODY_BYTES);
    let size = 0;
    const reader = request.body.getReader();
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        if (value.byteLength > MAX_REGISTRATION_BODY_BYTES - size) {
          void reader.cancel().catch(() => {});
          return tooLarge();
        }
        bytes.set(value, size);
        size += value.byteLength;
      }
    } finally {
      reader.releaseLock();
    }

    // The provider receives only the bounded, already-read body, with its
    // actual length rather than caller-supplied framing headers.
    const headers = new Headers(request.headers);
    headers.delete("transfer-encoding");
    headers.set("content-length", String(size));
    c.req.raw = new Request(request, { headers, body: bytes.subarray(0, size) });
    return next();
  });
  app.use(
    routes,
    bodyLimit({
      maxSize: MAX_BODY_BYTES,
      onError: (c) => c.json({ message: "request body too large" }, 413),
    }),
  );
  app.on(["GET", "POST"], routes, (c) => {
    const address = clientAddress(c.req.raw.headers, peerAddress(c), config.trustedClientIpHeader);
    return handler(forwarded(c.req.raw, normalizeOAuthPath(c.req.path), address));
  });

  if (metadata && !config.oauthDisabledReason) {
    // The issuer is this origin without a path, so RFC 8414 puts the
    // documents at the root rather than under Better Auth's base path.
    app.use("/.well-known/*", publicCors);
    for (const document of DISCOVERY_DOCUMENTS) {
      app.get(`/.well-known/${document}`, async (c) => {
        const body = withoutDpop(await metadata(c.req.raw, document));
        return c.json(body, 200, { "Cache-Control": METADATA_CACHE_CONTROL });
      });
    }

    app.get(OAUTH_SIGN_IN_PATH, (c) => c.html(signInPage(pages), 200, PAGE_HEADERS));
    app.get(OAUTH_CONSENT_PATH, (c) => c.html(consentPage(), 200, PAGE_HEADERS));
    app.get(`${ASSET_PATH}/:name`, (c) => {
      const name = c.req.param("name");
      const asset = Object.hasOwn(ASSETS, name) ? ASSETS[name] : undefined;
      if (!asset) return c.json({ message: "not found" }, 404);
      return c.body(asset.body, 200, {
        "Content-Type": asset.type,
        "Cache-Control": "public, max-age=300",
        "X-Content-Type-Options": "nosniff",
      });
    });
  }

  app.notFound((c) => c.json({ message: "not found" }, 404));
  return app;
}
