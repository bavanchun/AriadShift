// The OAuth authorization server end to end, in process: the real Better
// Auth plugins and HTTP layer over Better Auth's memory adapter. It follows
// what an MCP client does: discovery, dynamic registration, authorization
// with PKCE through the sign-in and consent steps, the token exchange and a
// refresh, and checks the access token the way ariad-server verifies it.

import { createPublicKey, type JsonWebKeyInput, verify } from "node:crypto";
import { runInNewContext } from "node:vm";
import { memoryAdapter } from "better-auth/adapters/memory";
import { getAuthTables } from "better-auth/db";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { createApp } from "../src/app.ts";
import { buildAuthOptions, createAuth, discoveryDocuments } from "../src/auth.ts";
import { parseConfig } from "../src/config.ts";
import { ASSETS, pageOptions } from "../src/pages.ts";
import {
  AUTH,
  audiences,
  authorizeUrl,
  createService,
  decode,
  Jar,
  json,
  pkce,
  post,
  REDIRECT_URI,
  RESOURCE,
  signedQuery,
  token,
} from "./oauth-helpers.ts";

describe("OAuth authorization for remote MCP", () => {
  const { app } = createService();
  const email = "flow@example.com";
  const password = "correct horse battery staple";
  let userId = "";
  let metadata: Record<string, unknown> = {};
  let clientId = "";

  beforeAll(async () => {
    const signUp = await post(app, "/api/auth/sign-up/email", { name: "Flow", email, password });
    expect(signUp.status).toBe(200);
    userId = ((await json(signUp)).user as { id: string }).id;
  });

  it("publishes discovery for the auth origin with S256 PKCE and no client credentials", async () => {
    const res = await app.request(`${AUTH}/.well-known/oauth-authorization-server`);
    expect(res.status).toBe(200);
    metadata = await json(res);
    expect(metadata.issuer).toBe(AUTH);
    expect(metadata.authorization_endpoint).toBe(`${AUTH}/api/auth/oauth2/authorize`);
    expect(metadata.token_endpoint).toBe(`${AUTH}/api/auth/oauth2/token`);
    expect(metadata.registration_endpoint).toBe(`${AUTH}/api/auth/oauth2/register`);
    expect(metadata.client_id_metadata_document_supported).toBe(true);
    expect(metadata.jwks_uri).toBe(`${AUTH}/api/auth/jwks`);
    expect(metadata.code_challenge_methods_supported).toEqual(["S256"]);
    expect(metadata.grant_types_supported).toEqual(["authorization_code", "refresh_token"]);
    expect(metadata.response_types_supported).toEqual(["code"]);
    expect(metadata.token_endpoint_auth_methods_supported).toContain("none");
    expect(metadata).not.toHaveProperty("dpop_signing_alg_values_supported");

    const oidc = await json(await app.request(`${AUTH}/.well-known/openid-configuration`));
    expect(oidc.issuer).toBe(AUTH);
    expect(oidc.registration_endpoint).toBe(`${AUTH}/api/auth/oauth2/register`);
    expect(oidc.client_id_metadata_document_supported).toBe(true);
    expect(oidc.code_challenge_methods_supported).toEqual(["S256"]);
  });

  it("registers a public client without an account (RFC 7591)", async () => {
    const res = await post(app, "/api/auth/oauth2/register", {
      client_name: "Flow test client",
      redirect_uris: [REDIRECT_URI],
      token_endpoint_auth_method: "none",
      grant_types: ["authorization_code", "refresh_token"],
      response_types: ["code"],
    });
    expect([200, 201]).toContain(res.status);
    const client = await json(res);
    expect(typeof client.client_id).toBe("string");
    expect(client).not.toHaveProperty("client_secret");
    expect(client.redirect_uris).toEqual([REDIRECT_URI]);
    clientId = client.client_id as string;
  });

  it("refuses to register a client for the client credentials grant", async () => {
    const res = await post(app, "/api/auth/oauth2/register", {
      client_name: "Machine client",
      redirect_uris: [REDIRECT_URI],
      grant_types: ["client_credentials"],
    });
    expect(res.status).toBeGreaterThanOrEqual(400);
  });

  it("rejects authorization requests without PKCE or with the plain method", async () => {
    const { challenge } = pkce();
    for (const extra of [
      {},
      { code_challenge: challenge },
      { code_challenge: challenge, code_challenge_method: "plain" },
    ]) {
      const res = await app.request(authorizeUrl(clientId, extra));
      expect(res.status, JSON.stringify(extra)).toBe(302);
      const location = new URL(res.headers.get("location") ?? "");
      expect(`${location.origin}${location.pathname}`).toBe(REDIRECT_URI);
      expect(location.searchParams.get("error")).toBe("invalid_request");
      expect(location.searchParams.get("state")).toBe("state-1");
    }
  });

  it("rejects resources other than the MCP endpoint (RFC 8707)", async () => {
    const { challenge } = pkce();
    const res = await app.request(
      authorizeUrl(clientId, {
        code_challenge: challenge,
        code_challenge_method: "S256",
        resource: "http://localhost:8080/v1",
      }),
    );
    expect(res.status).toBe(302);
    const location = new URL(res.headers.get("location") ?? "");
    expect(location.searchParams.get("error")).toBe("invalid_target");
  });

  it("signs in, asks for consent and issues a resource-bound access token", async () => {
    const jar = new Jar();
    const { verifier, challenge } = pkce();

    const authorize = await app.request(
      authorizeUrl(clientId, { code_challenge: challenge, code_challenge_method: "S256" }),
    );
    expect(authorize.status).toBe(302);
    const signInUrl = authorize.headers.get("location") ?? "";
    expect(signInUrl.startsWith(`${AUTH}/oauth/sign-in?`)).toBe(true);
    expect(new URL(signInUrl).searchParams.get("sig")).toBeTruthy();
    expect((await app.request(signInUrl)).status).toBe(200);

    // The sign-in page posts the credentials with the signed query.
    const signIn = await post(app, "/api/auth/sign-in/email", {
      email,
      password,
      oauth_query: signedQuery(signInUrl),
    });
    expect(signIn.status).toBe(200);
    jar.store(signIn);
    const afterSignIn = await json(signIn);
    expect(afterSignIn.redirect).toBe(true);
    const consentUrl = afterSignIn.url as string;
    expect(consentUrl.startsWith(`${AUTH}/oauth/consent?`)).toBe(true);
    expect(new URL(consentUrl).searchParams.get("client_id")).toBe(clientId);
    expect((await app.request(consentUrl)).status).toBe(200);

    const consent = await post(
      app,
      "/api/auth/oauth2/consent",
      { accept: true, oauth_query: signedQuery(consentUrl) },
      jar.header(),
    );
    expect(consent.status).toBe(200);
    const callback = new URL((await json(consent)).url as string);
    expect(`${callback.origin}${callback.pathname}`).toBe(REDIRECT_URI);
    expect(callback.searchParams.get("state")).toBe("state-1");
    expect(callback.searchParams.get("iss")).toBe(AUTH);
    const code = callback.searchParams.get("code") ?? "";
    expect(code).not.toBe("");

    const issued = await token(app, {
      grant_type: "authorization_code",
      code,
      redirect_uri: REDIRECT_URI,
      client_id: clientId,
      code_verifier: verifier,
      resource: RESOURCE,
    });
    expect(issued.status).toBe(200);
    const tokens = await json(issued);
    expect(tokens.token_type).toBe("Bearer");
    expect(tokens.expires_in).toBe(900);
    expect(typeof tokens.refresh_token).toBe("string");

    // The claims ariad-server checks: EdDSA key from the JWKS, issuer, the
    // MCP audience, the user as subject and a 15-minute expiry.
    const access = decode(tokens.access_token as string);
    expect(access.header.alg).toBe("EdDSA");
    expect(typeof access.header.kid).toBe("string");
    expect(access.payload.iss).toBe(AUTH);
    expect(audiences(access.payload.aud)).toContain(RESOURCE);
    expect(audiences(access.payload.aud)).not.toContain(AUTH);
    expect(access.payload.sub).toBe(userId);
    expect(access.payload.exp - access.payload.iat).toBe(900);
    expect(access.payload.client_id).toBe(clientId);

    const jwks = await json(await app.request(`${AUTH}/api/auth/jwks`));
    const jwk = (jwks.keys as Array<Record<string, string>>).find(
      (key) => key.kid === access.header.kid,
    );
    expect(jwk).toBeDefined();
    const key = createPublicKey({ key: jwk as JsonWebKeyInput["key"], format: "jwk" });
    expect(verify(null, Buffer.from(access.signed), key, access.signature)).toBe(true);

    // The session's ordinary API JWT keeps the auth origin as its audience,
    // so ariad-server's MCP endpoint refuses it.
    const apiJwt = await app.request(`${AUTH}/api/auth/token`, { headers: jar.header() });
    expect(apiJwt.status).toBe(200);
    const api = decode((await json(apiJwt)).token as string);
    expect(api.payload.aud).toBe(AUTH);
    expect(api.payload.sub).toBe(userId);

    // A refresh keeps the resource binding. A DPoP proof is ignored, so the
    // client still gets a Bearer token that ariad-server accepts.
    const refreshed = await token(
      app,
      {
        grant_type: "refresh_token",
        refresh_token: tokens.refresh_token as string,
        client_id: clientId,
        resource: RESOURCE,
      },
      { dpop: "not-a-proof" },
    );
    expect(refreshed.status).toBe(200);
    const renewed = await json(refreshed);
    expect(renewed.token_type).toBe("Bearer");
    const renewedAccess = decode(renewed.access_token as string);
    expect(audiences(renewedAccess.payload.aud)).toContain(RESOURCE);
    expect(renewedAccess.payload.sub).toBe(userId);
    expect(renewedAccess.payload).not.toHaveProperty("cnf");

    // Consent is remembered: the next authorization returns a code directly,
    // and that code is useless without the matching PKCE verifier.
    const second = pkce();
    const again = await app.request(
      authorizeUrl(clientId, { code_challenge: second.challenge, code_challenge_method: "S256" }),
      { headers: jar.header() },
    );
    expect(again.status).toBe(302);
    const secondCode = new URL(again.headers.get("location") ?? "").searchParams.get("code") ?? "";
    expect(secondCode).not.toBe("");
    const wrongVerifier = await token(app, {
      grant_type: "authorization_code",
      code: secondCode,
      redirect_uri: REDIRECT_URI,
      client_id: clientId,
      code_verifier: pkce().verifier,
      resource: RESOURCE,
    });
    expect(wrongVerifier.status).toBe(400);
    expect((await json(wrongVerifier)).error).toBe("invalid_grant");
  });

  it("refuses the client credentials grant at the token endpoint", async () => {
    const res = await token(app, { grant_type: "client_credentials", client_id: clientId });
    expect(res.status).toBe(400);
    expect((await json(res)).error).toBe("unsupported_grant_type");
  });
});

describe("OAuth sign-up during authorization", () => {
  const PROD_AUTH = "https://auth.example.com";
  const PROD_WEB = "https://app.example.com";
  const PROD_API = "https://api.example.com";
  const PROD_RESOURCE = "https://api.example.com/mcp";

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  function createTurnstileOAuthService(envOverrides: Record<string, string | undefined> = {}) {
    const turnstileConfig = parseConfig({
      DATABASE_URL: "postgres://unused@127.0.0.1:1/none",
      BETTER_AUTH_SECRET: "0123456789abcdef0123456789abcdef",
      BETTER_AUTH_URL: PROD_AUTH,
      BETTER_AUTH_TRUSTED_ORIGINS: PROD_WEB,
      ARIAD_PUBLIC_URL: PROD_API,
      AUTH_ALLOW_SIGN_UP: "true",
      AUTH_TURNSTILE_SECRET: "turnstile-secret",
      AUTH_TURNSTILE_SITE_KEY: "1x00000000000000000000AA",
      ...envOverrides,
    });
    const probe = buildAuthOptions(turnstileConfig, memoryAdapter({}));
    const db: Record<string, unknown[]> = Object.fromEntries(
      Object.values(getAuthTables(probe)).map((table) => [table.modelName, []]),
    );
    const options = buildAuthOptions(turnstileConfig, memoryAdapter(db));
    const auth = createAuth({
      ...options,
      rateLimit: { ...options.rateLimit, enabled: false },
    });
    const app = createApp({
      config: turnstileConfig,
      handler: (request) => auth.handler(request),
      metadata: discoveryDocuments(auth),
      pages: pageOptions(turnstileConfig),
      peerAddress: () => "203.0.113.10",
    });
    return { app, db, auth };
  }

  function postProd(
    app: ReturnType<typeof createTurnstileOAuthService>["app"],
    path: string,
    body: unknown,
    headers: Record<string, string> = {},
  ) {
    return app.request(`${PROD_AUTH}${path}`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        accept: "application/json",
        origin: PROD_AUTH,
        ...headers,
      },
      body: JSON.stringify(body),
    });
  }

  function postProdForm(
    app: ReturnType<typeof createTurnstileOAuthService>["app"],
    path: string,
    body: string,
    headers: Record<string, string> = {},
  ) {
    return app.request(`${PROD_AUTH}${path}`, {
      method: "POST",
      headers: {
        "content-type": "application/x-www-form-urlencoded",
        accept: "application/json",
        origin: PROD_AUTH,
        ...headers,
      },
      body,
    });
  }

  function tokenProd(
    app: ReturnType<typeof createTurnstileOAuthService>["app"],
    fields: Record<string, string>,
  ) {
    return app.request(`${PROD_AUTH}/api/auth/oauth2/token`, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams(fields).toString(),
    });
  }

  function prodAuthorizeUrl(
    clientId: string,
    extra: Record<string, string | undefined> = {},
  ): string {
    const url = new URL(`${PROD_AUTH}/api/auth/oauth2/authorize`);
    const params: Record<string, string | undefined> = {
      response_type: "code",
      client_id: clientId,
      redirect_uri: REDIRECT_URI,
      scope: "openid offline_access",
      state: "state-1",
      resource: PROD_RESOURCE,
      ...extra,
    };
    for (const [key, value] of Object.entries(params)) {
      if (value !== undefined) url.searchParams.set(key, value);
    }
    return url.toString();
  }

  async function registerClient(app: ReturnType<typeof createTurnstileOAuthService>["app"]) {
    const res = await postProd(app, "/api/auth/oauth2/register", {
      client_name: "Sign-up flow client",
      redirect_uris: [REDIRECT_URI],
      token_endpoint_auth_method: "none",
      grant_types: ["authorization_code", "refresh_token"],
      response_types: ["code"],
    });
    expect([200, 201]).toContain(res.status);
    return (await json(res)).client_id as string;
  }

  async function runOAuthSignUpPage(
    app: ReturnType<typeof createTurnstileOAuthService>["app"],
    jar: Jar,
    signInUrl: string,
    form: { name: string; email: string; password: string },
    turnstileWidget: { mode: "pass" | "error"; token?: string },
  ) {
    interface PageEl {
      textContent: string;
      hidden: boolean;
      disabled: boolean;
      classList: { toggle: () => void };
      addEventListener: (type: string, fn: (event: { preventDefault: () => void }) => void) => void;
      querySelector: () => { focus: () => void };
      trigger: (type: string) => void;
    }
    const elements = new Map<string, PageEl>();
    const byId = (id: string): PageEl => {
      let el = elements.get(id);
      if (!el) {
        const listeners = new Map<string, Array<(event: { preventDefault: () => void }) => void>>();
        el = {
          textContent: "",
          hidden: false,
          disabled: false,
          classList: { toggle: () => {} },
          addEventListener: (type, fn) => {
            const list = listeners.get(type) ?? [];
            list.push(fn);
            listeners.set(type, list);
          },
          querySelector: () => ({ focus: () => {} }),
          trigger: (type) => {
            for (const fn of listeners.get(type) ?? []) fn({ preventDefault: () => {} });
          },
        };
        elements.set(id, el);
      }
      return el;
    };
    byId("password-form").hidden = false;
    byId("sign-up-form").hidden = true;
    byId("two-factor-form").hidden = true;
    byId("turnstile").hidden = true;

    const assigned: string[] = [];
    const pending: Promise<unknown>[] = [];
    const parsedUrl = new URL(signInUrl);

    class StubFormData {
      get(key: string) {
        return (form as Record<string, string>)[key] ?? null;
      }
    }

    runInNewContext((ASSETS["oauth.js"] as { body: string }).body, {
      window: {
        location: {
          search: parsedUrl.search,
          href: parsedUrl.href,
          pathname: parsedUrl.pathname,
          assign: (url: string) => assigned.push(url),
        },
        turnstile: {
          render: (_container: unknown, opts: Record<string, unknown>) => {
            if (turnstileWidget.mode === "error") {
              (opts["error-callback"] as (() => void) | undefined)?.();
            } else {
              (opts.callback as ((token: string) => void) | undefined)?.(
                turnstileWidget.token ?? "stub-turnstile-proof",
              );
            }
            return "widget-1";
          },
          remove: () => {},
        },
      },
      document: {
        body: {
          dataset: {
            page: "sign-in",
            turnstileSiteKey: "1x00000000000000000000AA",
          },
          append: () => {},
        },
        head: { append: () => {} },
        getElementById: byId,
        querySelectorAll: (selector: string) =>
          selector === "button" ? [byId("sign-up-submit"), byId("create-account")] : [],
        createElement: () => ({ textContent: "" }),
      },
      FormData: StubFormData,
      fetch: (input: string, init: RequestInit = {}) => {
        const task = (async () => {
          const res = await app.request(`${PROD_AUTH}${input}`, {
            method: init.method ?? "GET",
            headers: {
              ...(init.headers as Record<string, string>),
              origin: PROD_AUTH,
              ...jar.header(),
            },
            body: init.body as string | undefined,
          });
          jar.store(res);
          return res;
        })();
        pending.push(task);
        return task;
      },
      URL,
      URLSearchParams,
    });

    const flush = async () => {
      for (let i = 0; i < 6; i++) {
        await Promise.allSettled(pending.splice(0));
        await new Promise((resolve) => setTimeout(resolve, 0));
      }
    };
    await flush();
    const openedSignUpAutomatically = !byId("sign-up-form").hidden;
    if (byId("sign-up-form").hidden) {
      byId("create-account").trigger("click");
      await flush();
    }
    byId("sign-up-form").trigger("submit");
    await flush();
    return { byId, assigned, openedSignUpAutomatically };
  }

  it("creates the user on a split-host deployment, verifies Turnstile on both auth and web hostnames, and returns to consent", async () => {
    const { app, db } = createTurnstileOAuthService();
    const clientId = await registerClient(app);
    const jar = new Jar();
    const { verifier, challenge } = pkce();
    const requestedScopes = "openid profile email offline_access";
    const requestedState = "state-signup-42";

    const siteverifyCalls: Array<Record<string, string>> = [];
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      expect(String(input)).toBe("https://challenges.cloudflare.com/turnstile/v0/siteverify");
      siteverifyCalls.push(JSON.parse(String(init?.body)) as Record<string, string>);
      return Response.json({
        success: true,
        action: "signup",
        hostname: "auth.example.com",
      });
    });

    const authorize = await app.request(
      prodAuthorizeUrl(clientId, {
        scope: requestedScopes,
        state: requestedState,
        code_challenge: challenge,
        code_challenge_method: "S256",
        resource: PROD_RESOURCE,
      }),
    );
    expect(authorize.status).toBe(302);
    const signInUrl = authorize.headers.get("location") ?? "";
    expect(signInUrl.startsWith(`${PROD_AUTH}/oauth/sign-in?`)).toBe(true);

    const signInPageRes = await app.request(signInUrl);
    expect(signInPageRes.status).toBe(200);
    const signInHtml = await signInPageRes.text();
    expect(signInHtml).toContain('id="create-account"');
    expect(signInHtml).toContain('id="sign-up-form"');
    expect(signInPageRes.headers.get("content-security-policy")).toContain(
      "https://challenges.cloudflare.com",
    );

    const { assigned, byId } = await runOAuthSignUpPage(
      app,
      jar,
      signInUrl,
      {
        name: "New OAuth User",
        email: "new-oauth@example.com",
        password: "correct horse battery staple",
      },
      { mode: "pass", token: "turnstile-proof-signup" },
    );

    expect(siteverifyCalls).toEqual([
      {
        secret: "turnstile-secret",
        response: "turnstile-proof-signup",
        remoteip: "203.0.113.10",
      },
    ]);
    expect(db.user).toHaveLength(1);
    const createdUser = db.user?.[0] as { id: string; email: string };
    expect(createdUser.email).toBe("new-oauth@example.com");
    expect(byId("status").textContent).toBe("Account created. Continuing…");

    expect(assigned).toHaveLength(1);
    const consentUrl = assigned[0] as string;
    const consentParsed = new URL(consentUrl);
    expect(consentParsed.origin).toBe(PROD_AUTH);
    expect(consentParsed.pathname).toBe("/oauth/consent");
    expect(consentParsed.searchParams.get("client_id")).toBe(clientId);
    expect(consentParsed.searchParams.get("redirect_uri")).toBe(REDIRECT_URI);
    expect(consentParsed.searchParams.get("scope")).toBe(requestedScopes);
    expect(consentParsed.searchParams.get("state")).toBe(requestedState);
    expect(consentParsed.searchParams.get("code_challenge")).toBe(challenge);
    expect(consentParsed.searchParams.get("code_challenge_method")).toBe("S256");
    expect(consentParsed.searchParams.get("resource")).toBe(PROD_RESOURCE);
    expect(consentParsed.searchParams.get("sig")).toBeTruthy();

    const consent = await postProd(
      app,
      "/api/auth/oauth2/consent",
      { accept: true, oauth_query: signedQuery(consentUrl) },
      jar.header(),
    );
    expect(consent.status).toBe(200);
    const callback = new URL((await json(consent)).url as string);
    expect(`${callback.origin}${callback.pathname}`).toBe(REDIRECT_URI);
    expect(callback.searchParams.get("state")).toBe(requestedState);
    const code = callback.searchParams.get("code") ?? "";
    expect(code).not.toBe("");

    const issued = await tokenProd(app, {
      grant_type: "authorization_code",
      code,
      redirect_uri: REDIRECT_URI,
      client_id: clientId,
      code_verifier: verifier,
      resource: PROD_RESOURCE,
    });
    expect(issued.status).toBe(200);
    const tokens = await json(issued);
    const access = decode(tokens.access_token as string);
    expect(access.payload.sub).toBe(createdUser.id);
    expect(access.payload.client_id).toBe(clientId);
    expect(audiences(access.payload.aud)).toContain(PROD_RESOURCE);

    // Web origin hostname (app.example.com) remains accepted on the same split-host service,
    // while an untrusted hostname is still rejected.
    vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      Response.json({
        success: true,
        action: "signup",
        hostname: "app.example.com",
      }),
    );
    const webSignup = await app.request(`${PROD_AUTH}/api/auth/sign-up/email`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        accept: "application/json",
        origin: PROD_WEB,
        "x-captcha-response": "turnstile-proof-web",
      },
      body: JSON.stringify({
        name: "Web User",
        email: "web-user@example.com",
        password: "correct horse battery staple",
      }),
    });
    expect(webSignup.status).toBe(200);
    expect(db.user).toHaveLength(2);

    vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      Response.json({
        success: true,
        action: "signup",
        hostname: "evil.example.com",
      }),
    );
    const evilHostSignup = await postProd(
      app,
      "/api/auth/sign-up/email",
      {
        name: "Evil Host",
        email: "evil-host@example.com",
        password: "correct horse battery staple",
      },
      { "x-captcha-response": "turnstile-proof-evil" },
    );
    expect(evilHostSignup.status).toBe(403);
    expect((await json(evilHostSignup)).code).toBe("CAPTCHA_VERIFICATION_FAILED");
    expect(db.user).toHaveLength(2);
  });

  it("supports prompt=create on /api/auth/oauth2/authorize through /oauth/sign-in and /oauth2/continue", async () => {
    const { app, db } = createTurnstileOAuthService();
    const clientId = await registerClient(app);
    const jar = new Jar();
    const { verifier, challenge } = pkce();

    vi.spyOn(globalThis, "fetch").mockImplementation(async () =>
      Response.json({
        success: true,
        action: "signup",
        hostname: "auth.example.com",
      }),
    );

    const authorize = await app.request(
      prodAuthorizeUrl(clientId, {
        prompt: "create",
        state: "state-prompt-create",
        code_challenge: challenge,
        code_challenge_method: "S256",
        resource: PROD_RESOURCE,
      }),
    );
    expect(authorize.status).toBe(302);
    const signInUrl = authorize.headers.get("location") ?? "";
    expect(signInUrl.startsWith(`${PROD_AUTH}/oauth/sign-in?`)).toBe(true);
    expect(new URL(signInUrl).searchParams.get("prompt")).toBe("create");

    const { assigned, openedSignUpAutomatically } = await runOAuthSignUpPage(
      app,
      jar,
      signInUrl,
      {
        name: "Prompt Create User",
        email: "prompt-create@example.com",
        password: "correct horse battery staple",
      },
      { mode: "pass", token: "turnstile-proof-prompt-create" },
    );
    expect(openedSignUpAutomatically).toBe(true);
    expect(db.user).toHaveLength(1);
    expect(assigned).toHaveLength(1);

    const consentUrl = assigned[0] as string;
    const consentParsed = new URL(consentUrl);
    expect(consentParsed.origin).toBe(PROD_AUTH);
    expect(consentParsed.pathname).toBe("/oauth/consent");
    expect(consentParsed.searchParams.get("prompt")).toBeNull();
    expect(consentParsed.searchParams.get("state")).toBe("state-prompt-create");

    const consent = await postProd(
      app,
      "/api/auth/oauth2/consent",
      { accept: true, oauth_query: signedQuery(consentUrl) },
      jar.header(),
    );
    expect(consent.status).toBe(200);
    const callback = new URL((await json(consent)).url as string);
    expect(`${callback.origin}${callback.pathname}`).toBe(REDIRECT_URI);
    expect(callback.searchParams.get("state")).toBe("state-prompt-create");
    const code = callback.searchParams.get("code") ?? "";
    expect(code).not.toBe("");

    const issued = await tokenProd(app, {
      grant_type: "authorization_code",
      code,
      redirect_uri: REDIRECT_URI,
      client_id: clientId,
      code_verifier: verifier,
      resource: PROD_RESOURCE,
    });
    expect(issued.status).toBe(200);
  });

  it("omits sign-up controls, Turnstile site key, and Turnstile CSP when AUTH_ALLOW_SIGN_UP=false, while plain password sign-in still works", async () => {
    const { app } = createTurnstileOAuthService();
    const clientId = await registerClient(app);
    const { challenge } = pkce();

    // First create a user on the sign-up-enabled service, then test plain password sign-in.
    vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      Response.json({
        success: true,
        action: "signup",
        hostname: "auth.example.com",
      }),
    );
    const created = await postProd(
      app,
      "/api/auth/sign-up/email",
      {
        name: "Existing User",
        email: "existing@example.com",
        password: "correct horse battery staple",
      },
      { "x-captcha-response": "turnstile-proof-existing" },
    );
    expect(created.status).toBe(200);

    const authorize = await app.request(
      prodAuthorizeUrl(clientId, {
        code_challenge: challenge,
        code_challenge_method: "S256",
      }),
    );
    expect(authorize.status).toBe(302);
    const signInUrl = authorize.headers.get("location") ?? "";
    const signIn = await postProd(app, "/api/auth/sign-in/email", {
      email: "existing@example.com",
      password: "correct horse battery staple",
      oauth_query: signedQuery(signInUrl),
    });
    expect(signIn.status).toBe(200);
    const afterSignIn = await json(signIn);
    expect(afterSignIn.redirect).toBe(true);
    expect((afterSignIn.url as string).startsWith(`${PROD_AUTH}/oauth/consent?`)).toBe(true);

    // Now verify a deployment with AUTH_ALLOW_SIGN_UP=false.
    const disabled = createTurnstileOAuthService({ AUTH_ALLOW_SIGN_UP: "false" });
    const disabledPage = await disabled.app.request(`${PROD_AUTH}/oauth/sign-in`);
    expect(disabledPage.status).toBe(200);
    const disabledHtml = await disabledPage.text();
    expect(disabledHtml).not.toContain('id="create-account"');
    expect(disabledHtml).not.toContain('id="sign-up-form"');
    expect(disabledHtml).not.toContain("data-turnstile-site-key");
    expect(disabledPage.headers.get("content-security-policy")).not.toContain(
      "challenges.cloudflare.com",
    );
  });

  it("refuses tampered, expired, or form-urlencoded return parameters on OAuth sign-up without creating a user or open redirect", async () => {
    const { app, db } = createTurnstileOAuthService();
    const clientId = await registerClient(app);
    const { challenge } = pkce();

    vi.spyOn(globalThis, "fetch").mockImplementation(async () =>
      Response.json({
        success: true,
        action: "signup",
        hostname: "auth.example.com",
      }),
    );

    const authorize = await app.request(
      prodAuthorizeUrl(clientId, {
        code_challenge: challenge,
        code_challenge_method: "S256",
      }),
    );
    expect(authorize.status).toBe(302);
    const signInUrl = authorize.headers.get("location") ?? "";
    const validOAuthQuery = signedQuery(signInUrl);

    // 1. Tampered redirect_uri inside signed oauth_query (signature mismatch).
    const tamperedRedirect = new URLSearchParams(validOAuthQuery);
    tamperedRedirect.set("redirect_uri", "https://attacker.example/callback");
    const resTamperedRedirect = await postProd(
      app,
      "/api/auth/sign-up/email",
      {
        name: "Tampered Redirect",
        email: "tampered-redirect@example.com",
        password: "correct horse battery staple",
        oauth_query: tamperedRedirect.toString(),
      },
      { "x-captcha-response": "turnstile-proof" },
    );
    expect(resTamperedRedirect.status).toBe(400);

    // 2. Repeated redirect_uri appended to oauth_query (parameter pollution).
    const pollutedQuery = `${validOAuthQuery}&redirect_uri=${encodeURIComponent("https://attacker.example/callback")}`;
    const resPolluted = await postProd(
      app,
      "/api/auth/sign-up/email",
      {
        name: "Polluted",
        email: "polluted@example.com",
        password: "correct horse battery staple",
        oauth_query: pollutedQuery,
      },
      { "x-captcha-response": "turnstile-proof" },
    );
    expect(resPolluted.status).toBe(400);

    // 3. Free-form return URL fields in the JSON sign-up request body.
    for (const field of [
      "return_to",
      "returnTo",
      "return_url",
      "redirect_to",
      "redirect_uri",
      "callbackURL",
      "next",
      "continue",
      "url",
    ]) {
      const resField = await postProd(
        app,
        "/api/auth/sign-up/email",
        {
          name: "Return Field",
          email: `${field}@example.com`,
          password: "correct horse battery staple",
          oauth_query: validOAuthQuery,
          [field]: "https://attacker.example/steal",
        },
        { "x-captcha-response": "turnstile-proof" },
      );
      expect(resField.status, field).toBe(400);
    }

    // 4. Hostile application/x-www-form-urlencoded bodies (M1): forbidden return fields,
    // tampered oauth_query, and repeated oauth_query.
    const formWithCallbackUrl = new URLSearchParams({
      name: "Form Callback",
      email: "form-callback@example.com",
      password: "correct horse battery staple",
      callbackURL: "https://attacker.example/steal",
    }).toString();
    const resFormCallback = await postProdForm(
      app,
      "/api/auth/sign-up/email",
      formWithCallbackUrl,
      {
        "x-captcha-response": "turnstile-proof",
      },
    );
    expect(resFormCallback.status).toBe(400);

    const formWithTamperedOAuth = new URLSearchParams({
      name: "Form Tampered",
      email: "form-tampered@example.com",
      password: "correct horse battery staple",
      oauth_query: tamperedRedirect.toString(),
    }).toString();
    const resFormTampered = await postProdForm(
      app,
      "/api/auth/sign-up/email",
      formWithTamperedOAuth,
      { "x-captcha-response": "turnstile-proof" },
    );
    expect(resFormTampered.status).toBe(400);

    const formRepeatedOAuth = new URLSearchParams([
      ["name", "Form Repeated"],
      ["email", "form-repeated@example.com"],
      ["password", "correct horse battery staple"],
      ["oauth_query", validOAuthQuery],
      ["oauth_query", tamperedRedirect.toString()],
    ]).toString();
    const resFormRepeated = await postProdForm(app, "/api/auth/sign-up/email", formRepeatedOAuth, {
      "x-captcha-response": "turnstile-proof",
    });
    expect(resFormRepeated.status).toBe(400);

    const resUnsupportedMedia = await app.request(`${PROD_AUTH}/api/auth/sign-up/email`, {
      method: "POST",
      headers: {
        "content-type": "text/plain",
        origin: PROD_AUTH,
        "x-captcha-response": "turnstile-proof",
      },
      body: "name=Plain&email=plain@example.com",
    });
    expect(resUnsupportedMedia.status).toBe(415);

    // 5. Query string return parameter on the sign-up endpoint itself.
    const resQueryReturn = await postProd(
      app,
      "/api/auth/sign-up/email?return_to=https://attacker.example/steal",
      {
        name: "Query Return",
        email: "query-return@example.com",
        password: "correct horse battery staple",
        oauth_query: validOAuthQuery,
      },
      { "x-captcha-response": "turnstile-proof" },
    );
    expect(resQueryReturn.status).toBe(400);

    // 6. Expired signed oauth_query (L5): advance clock past the signed `exp` timestamp.
    const expSeconds = Number(new URLSearchParams(validOAuthQuery).get("exp"));
    expect(Number.isSafeInteger(expSeconds)).toBe(true);
    vi.useFakeTimers();
    vi.setSystemTime((expSeconds + 60) * 1000);
    const resExpiredQuery = await postProd(
      app,
      "/api/auth/sign-up/email",
      {
        name: "Expired Query",
        email: "expired-query@example.com",
        password: "correct horse battery staple",
        oauth_query: validOAuthQuery,
      },
      { "x-captcha-response": "turnstile-proof" },
    );
    expect(resExpiredQuery.status).toBe(400);
    vi.useRealTimers();

    // 7. Browser script with a tampered return_to query parameter in /oauth/sign-in.
    const jar = new Jar();
    const tamperedSignInUrl = `${signInUrl}&return_to=${encodeURIComponent("https://attacker.example/steal")}`;
    const { assigned, byId } = await runOAuthSignUpPage(
      app,
      jar,
      tamperedSignInUrl,
      {
        name: "Browser Tampered",
        email: "browser-tampered@example.com",
        password: "correct horse battery staple",
      },
      { mode: "pass", token: "turnstile-proof" },
    );
    expect(assigned).toHaveLength(0);
    expect(byId("status").textContent).toMatch(/not valid/);
    expect(db.user).toHaveLength(0);
    expect(db.session).toHaveLength(0);
  });

  it("blocks OAuth sign-up when the bot check fails", async () => {
    const { app, db } = createTurnstileOAuthService();
    const clientId = await registerClient(app);
    const { challenge } = pkce();

    const authorize = await app.request(
      prodAuthorizeUrl(clientId, {
        code_challenge: challenge,
        code_challenge_method: "S256",
      }),
    );
    expect(authorize.status).toBe(302);
    const signInUrl = authorize.headers.get("location") ?? "";
    const validOAuthQuery = signedQuery(signInUrl);

    // 1. Client-side widget failure blocks submission before any request is made.
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const jar = new Jar();
    const { assigned, byId } = await runOAuthSignUpPage(
      app,
      jar,
      signInUrl,
      {
        name: "Bot Widget Fail",
        email: "widget-fail@example.com",
        password: "correct horse battery staple",
      },
      { mode: "error" },
    );
    expect(assigned).toHaveLength(0);
    expect(byId("status").textContent).toMatch(/Bot check failed/);
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(db.user).toHaveLength(0);

    // 2. Server-side missing proof or failed Siteverify response blocks sign-up.
    const missingProof = await postProd(app, "/api/auth/sign-up/email", {
      name: "No Proof",
      email: "no-proof@example.com",
      password: "correct horse battery staple",
      oauth_query: validOAuthQuery,
    });
    expect(missingProof.status).toBe(400);
    expect((await json(missingProof)).code).toBe("MISSING_CAPTCHA_RESPONSE");

    fetchSpy.mockResolvedValueOnce(
      Response.json({
        success: false,
        "error-codes": ["invalid-input-response"],
      }),
    );
    const rejectedProof = await postProd(
      app,
      "/api/auth/sign-up/email",
      {
        name: "Rejected Proof",
        email: "rejected-proof@example.com",
        password: "correct horse battery staple",
        oauth_query: validOAuthQuery,
      },
      { "x-captcha-response": "bad-turnstile-token" },
    );
    expect(rejectedProof.status).toBe(403);
    expect((await json(rejectedProof)).code).toBe("CAPTCHA_VERIFICATION_FAILED");
    expect(db.user).toHaveLength(0);
    expect(db.session).toHaveLength(0);
  });
});
