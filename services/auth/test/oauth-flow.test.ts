// The OAuth authorization server end to end, in process: the real Better
// Auth plugins and HTTP layer over Better Auth's memory adapter. It follows
// what an MCP client does: discovery, dynamic registration, authorization
// with PKCE through the sign-in and consent steps, the token exchange and a
// refresh, and checks the access token the way ariad-server verifies it.

import { createPublicKey, type JsonWebKey, verify } from "node:crypto";
import { runInNewContext } from "node:vm";
import { memoryAdapter } from "better-auth/adapters/memory";
import { getAuthTables } from "better-auth/db";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { createApp } from "../src/app.ts";
import { buildAuthOptions, createAuth, discoveryDocuments } from "../src/auth.ts";
import { ASSETS, pageOptions } from "../src/pages.ts";
import {
  AUTH,
  audiences,
  authorizeUrl,
  config as baseConfig,
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
    expect(metadata.jwks_uri).toBe(`${AUTH}/api/auth/jwks`);
    expect(metadata.code_challenge_methods_supported).toEqual(["S256"]);
    expect(metadata.grant_types_supported).toEqual(["authorization_code", "refresh_token"]);
    expect(metadata.response_types_supported).toEqual(["code"]);
    expect(metadata.token_endpoint_auth_methods_supported).toContain("none");
    expect(metadata).not.toHaveProperty("dpop_signing_alg_values_supported");

    const oidc = await json(await app.request(`${AUTH}/.well-known/openid-configuration`));
    expect(oidc.issuer).toBe(AUTH);
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
    const key = createPublicKey({ key: jwk as JsonWebKey, format: "jwk" });
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
  afterEach(() => {
    vi.restoreAllMocks();
  });

  function createTurnstileOAuthService() {
    const turnstileConfig = {
      ...baseConfig,
      trustedOrigins: [AUTH],
      turnstileSecret: "turnstile-secret",
    };
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
      pages: pageOptions(turnstileConfig, {
        AUTH_TURNSTILE_SITE_KEY: "1x00000000000000000000AA",
      }),
      peerAddress: () => "203.0.113.10",
    });
    return { app, db, auth };
  }

  async function registerClient(app: ReturnType<typeof createTurnstileOAuthService>["app"]) {
    const res = await post(app, "/api/auth/oauth2/register", {
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
          const res = await app.request(`${AUTH}${input}`, {
            method: init.method ?? "GET",
            headers: {
              ...(init.headers as Record<string, string>),
              origin: AUTH,
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
    byId("create-account").trigger("click");
    await flush();
    byId("sign-up-form").trigger("submit");
    await flush();
    return { byId, assigned };
  }

  it("creates the user from the OAuth sign-in page, runs the bot check, and returns to consent with the same parameters", async () => {
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
        hostname: "localhost",
      });
    });

    const authorize = await app.request(
      authorizeUrl(clientId, {
        scope: requestedScopes,
        state: requestedState,
        code_challenge: challenge,
        code_challenge_method: "S256",
        resource: RESOURCE,
      }),
    );
    expect(authorize.status).toBe(302);
    const signInUrl = authorize.headers.get("location") ?? "";
    expect(signInUrl.startsWith(`${AUTH}/oauth/sign-in?`)).toBe(true);

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
    expect(consentParsed.origin).toBe(AUTH);
    expect(consentParsed.pathname).toBe("/oauth/consent");
    expect(consentParsed.searchParams.get("client_id")).toBe(clientId);
    expect(consentParsed.searchParams.get("redirect_uri")).toBe(REDIRECT_URI);
    expect(consentParsed.searchParams.get("scope")).toBe(requestedScopes);
    expect(consentParsed.searchParams.get("state")).toBe(requestedState);
    expect(consentParsed.searchParams.get("code_challenge")).toBe(challenge);
    expect(consentParsed.searchParams.get("code_challenge_method")).toBe("S256");
    expect(consentParsed.searchParams.get("resource")).toBe(RESOURCE);
    expect(consentParsed.searchParams.get("sig")).toBeTruthy();

    const consent = await post(
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
    const access = decode(tokens.access_token as string);
    expect(access.payload.sub).toBe(createdUser.id);
    expect(access.payload.client_id).toBe(clientId);
    expect(audiences(access.payload.aud)).toContain(RESOURCE);
  });

  it("refuses a tampered return parameter on OAuth sign-up without creating a user or open redirect", async () => {
    const { app, db } = createTurnstileOAuthService();
    const clientId = await registerClient(app);
    const { challenge } = pkce();

    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      Response.json({
        success: true,
        action: "signup",
        hostname: "localhost",
      }),
    );

    const authorize = await app.request(
      authorizeUrl(clientId, {
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
    const resTamperedRedirect = await post(
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
    const resPolluted = await post(
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

    // 3. Free-form return URL fields in the sign-up request body.
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
      const resField = await post(
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

    // 4. Query string return parameter on the sign-up endpoint itself.
    const resQueryReturn = await post(
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

    // 5. Browser script with a tampered return_to query parameter in /oauth/sign-in.
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
      authorizeUrl(clientId, {
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
    const missingProof = await post(app, "/api/auth/sign-up/email", {
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
    const rejectedProof = await post(
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
