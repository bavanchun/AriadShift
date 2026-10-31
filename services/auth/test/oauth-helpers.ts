import { createHash, randomBytes } from "node:crypto";
import type { BetterAuthOptions } from "better-auth";
import { memoryAdapter } from "better-auth/adapters/memory";
import { getAuthTables } from "better-auth/db";
import { createApp } from "../src/app.ts";
import { buildAuthOptions, createAuth, discoveryDocuments } from "../src/auth.ts";
import { parseConfig } from "../src/config.ts";
import { pageOptions } from "../src/pages.ts";

export const AUTH = "http://localhost:3001";
export const RESOURCE = "http://localhost:8080/mcp";
export const REDIRECT_URI = "http://127.0.0.1:33418/callback";

export const config = parseConfig({
  DATABASE_URL: "postgres://unused@127.0.0.1:1/none",
  BETTER_AUTH_SECRET: "0123456789abcdef0123456789abcdef",
  BETTER_AUTH_URL: AUTH,
  AUTH_ALLOW_SIGN_UP: "true",
  BETTER_AUTH_TRUSTED_ORIGINS: "http://localhost:3000",
  ARIAD_PUBLIC_URL: "http://localhost:8080",
});

export function createService(
  database?: NonNullable<BetterAuthOptions["database"]>,
  rateLimit = false,
) {
  const probe = buildAuthOptions(config, memoryAdapter({}));
  const db: Record<string, unknown[]> = Object.fromEntries(
    Object.values(getAuthTables(probe)).map((table) => [table.modelName, []]),
  );
  const options = buildAuthOptions(config, database ?? memoryAdapter(db));
  // Rate limits are covered elsewhere; the flow makes many quick requests.
  const auth = createAuth({ ...options, rateLimit: { ...options.rateLimit, enabled: rateLimit } });
  const app = createApp({
    config,
    handler: (request) => auth.handler(request),
    metadata: discoveryDocuments(auth),
    pages: pageOptions(config),
    peerAddress: () => "127.0.0.1",
  });
  return { app, db, auth };
}

export type App = ReturnType<typeof createService>["app"];

/** Cookies this browser holds for the auth origin. */
export class Jar {
  readonly cookies = new Map<string, string>();

  store(response: Response): void {
    for (const line of response.headers.getSetCookie()) {
      const pair = line.split(";")[0] ?? "";
      const index = pair.indexOf("=");
      if (index > 0) this.cookies.set(pair.slice(0, index).trim(), pair.slice(index + 1));
    }
  }

  header(): Record<string, string> {
    if (this.cookies.size === 0) return {};
    return {
      cookie: [...this.cookies].map(([name, value]) => `${name}=${value}`).join("; "),
    };
  }
}

/** What pages/oauth.js sends as `oauth_query`. */
export function signedQuery(pageUrl: string): string {
  const query = new URL(pageUrl).searchParams;
  const signed = new Set(query.getAll("ba_param"));
  const out = new URLSearchParams();
  for (const [key, value] of query) {
    if (key === "sig" || key === "ba_param" || signed.has(key)) out.append(key, value);
  }
  return out.toString();
}

export function pkce() {
  const verifier = randomBytes(32).toString("base64url");
  const challenge = createHash("sha256").update(verifier).digest("base64url");
  return { verifier, challenge };
}

export function decode(token: string) {
  const [header, payload, signature] = token.split(".");
  return {
    header: JSON.parse(Buffer.from(header as string, "base64url").toString()),
    payload: JSON.parse(Buffer.from(payload as string, "base64url").toString()),
    signed: `${header}.${payload}`,
    signature: Buffer.from(signature as string, "base64url"),
  };
}

export function audiences(aud: unknown): string[] {
  return Array.isArray(aud) ? aud : [aud as string];
}

export async function json(response: Response): Promise<Record<string, unknown>> {
  return (await response.json()) as Record<string, unknown>;
}

export function post(app: App, path: string, body: unknown, headers: Record<string, string> = {}) {
  return app.request(`${AUTH}${path}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      accept: "application/json",
      origin: AUTH,
      ...headers,
    },
    body: JSON.stringify(body),
  });
}

export async function token(
  app: App,
  fields: Record<string, string>,
  headers: Record<string, string> = {},
) {
  return app.request(`${AUTH}/api/auth/oauth2/token`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded", ...headers },
    body: new URLSearchParams(fields).toString(),
  });
}

export function authorizeUrl(clientId: string, extra: Record<string, string | undefined>): string {
  const url = new URL(`${AUTH}/api/auth/oauth2/authorize`);
  const params: Record<string, string | undefined> = {
    response_type: "code",
    client_id: clientId,
    redirect_uri: REDIRECT_URI,
    scope: "openid offline_access",
    state: "state-1",
    resource: RESOURCE,
    ...extra,
  };
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined) url.searchParams.set(key, value);
  }
  return url.toString();
}

/** Real account/client/browser setup shared by memory and PostgreSQL tests. */
export async function browserFlow(app: App) {
  const email = `flow-${randomBytes(8).toString("hex")}@example.com`;
  const password = "correct horse battery staple";
  const signup = await post(app, "/api/auth/sign-up/email", { name: "Flow", email, password });
  if (signup.status !== 200) throw new Error(`sign-up failed: ${signup.status}`);
  const register = await post(app, "/api/auth/oauth2/register", {
    client_name: "Security test",
    redirect_uris: [REDIRECT_URI],
    token_endpoint_auth_method: "none",
    grant_types: ["authorization_code", "refresh_token"],
    response_types: ["code"],
  });
  if (![200, 201].includes(register.status))
    throw new Error(`registration failed: ${register.status}`);
  const clientId = (await json(register)).client_id as string;
  const jar = new Jar();
  const login = await post(app, "/api/auth/sign-in/email", { email, password });
  if (login.status !== 200) throw new Error(`sign-in failed: ${login.status}`);
  jar.store(login);
  async function authorization(
    accept = true,
    mutate = false,
  ): Promise<{ consent: Response; fields: Record<string, string>; callback?: URL }> {
    const proof = pkce();
    const res = await app.request(
      authorizeUrl(clientId, {
        code_challenge: proof.challenge,
        code_challenge_method: "S256",
        prompt: "consent",
      }),
      { headers: jar.header() },
    );
    if (res.status !== 302) throw new Error(`authorization failed: ${res.status}`);
    const location = res.headers.get("location") ?? "";
    const query = new URLSearchParams(signedQuery(location));
    if (mutate) query.set("redirect_uri", "https://attacker.example/callback");
    const consent = await post(
      app,
      "/api/auth/oauth2/consent",
      {
        accept,
        oauth_query: query.toString(),
      },
      jar.header(),
    );
    if (mutate) return { consent, fields: {}, callback: undefined };
    if (consent.status !== 200) throw new Error(`consent failed: ${consent.status}`);
    const callback = new URL((await json(consent)).url as string);
    return {
      consent,
      callback,
      fields: {
        grant_type: "authorization_code",
        client_id: clientId,
        code: callback.searchParams.get("code") ?? "",
        redirect_uri: REDIRECT_URI,
        code_verifier: proof.verifier,
        resource: RESOURCE,
      },
    };
  }
  async function issue() {
    const { fields } = await authorization();
    const res = await token(app, fields);
    if (res.status !== 200) throw new Error(`token issuance failed: ${res.status}`);
    return json(res);
  }
  function refresh(refreshToken: string) {
    return token(app, {
      grant_type: "refresh_token",
      client_id: clientId,
      refresh_token: refreshToken,
      resource: RESOURCE,
    });
  }
  return { clientId, jar, authorization, issue, refresh };
}
