// HTTP layer: CORS for the web app, a body limit, and the client address
// Better Auth rate-limits by. Everything under /api/auth goes to Better Auth.

import { type Context, Hono } from "hono";
import { bodyLimit } from "hono/body-limit";
import { cors } from "hono/cors";
import { BASE_PATH, CLIENT_IP_HEADER } from "./auth.ts";
import type { AuthServiceConfig } from "./config.ts";

/** Largest accepted request body; auth payloads are small JSON documents. */
export const MAX_BODY_BYTES = 64 * 1024;

export interface AppDeps {
  config: Pick<AuthServiceConfig, "trustedOrigins" | "trustedClientIpHeader">;
  handler: (request: Request) => Promise<Response>;
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

/** Copies the request with the client-address header set to `address` only. */
function withClientAddress(request: Request, address: string | undefined): Request {
  const headers = new Headers(request.headers);
  headers.delete(CLIENT_IP_HEADER);
  if (address) headers.set(CLIENT_IP_HEADER, address);
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

export function createApp({ config, handler, peerAddress }: AppDeps): Hono {
  const app = new Hono();
  const routes = `${BASE_PATH}/*`;

  app.use(
    routes,
    cors({
      origin: config.trustedOrigins,
      credentials: true,
      allowMethods: ["GET", "POST", "OPTIONS"],
      allowHeaders: ["Content-Type", "Authorization"],
      // The JWT plugin returns a token in this header on get-session.
      exposeHeaders: ["set-auth-jwt"],
      maxAge: 600,
    }),
  );
  app.use(
    routes,
    bodyLimit({
      maxSize: MAX_BODY_BYTES,
      onError: (c) => c.json({ message: "request body too large" }, 413),
    }),
  );
  app.on(["GET", "POST"], routes, (c) => {
    const address = clientAddress(
      c.req.raw.headers,
      peerAddress(c),
      config.trustedClientIpHeader,
    );
    return handler(withClientAddress(c.req.raw, address));
  });

  app.notFound((c) => c.json({ message: "not found" }, 404));
  return app;
}
