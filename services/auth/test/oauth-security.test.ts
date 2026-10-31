import { afterEach, describe, expect, it, vi } from "vitest";
import { OAUTH_REGISTRATION_RATE_LIMIT } from "../src/auth.ts";
import {
  AUTH,
  authorizeUrl,
  browserFlow,
  createService,
  json,
  pkce,
  post,
  REDIRECT_URI,
  RESOURCE,
  token,
} from "./oauth-helpers.ts";

// Only Date is faked: provider expiry checks use wall time, while crypto and
// database operations must keep their real event-loop timers.
afterEach(() => vi.useRealTimers());

describe("OAuth security boundaries", () => {
  it.each([
    "http://example.com/callback",
    "https://user:password@example.com/callback",
    "https://example.com/callback#fragment",
    "javascript:alert(1)",
    "data:text/plain,x",
  ])("rejects forbidden registration redirect %s", async (redirect) => {
    const { app } = createService();
    const res = await post(app, "/api/auth/oauth2/register", {
      redirect_uris: [redirect],
      token_endpoint_auth_method: "none",
    });
    expect(res.status).toBeGreaterThanOrEqual(400);
  });

  it("keeps PKCE mandatory when a registering client tries to disable it", async () => {
    const { app } = createService();
    const res = await post(app, "/api/auth/oauth2/register", {
      redirect_uris: [REDIRECT_URI],
      token_endpoint_auth_method: "none",
      grant_types: ["authorization_code", "refresh_token"],
      response_types: ["code"],
      require_pkce: false,
    });
    expect(res.status).toBe(201);
    const client = await json(res);
    expect(client.require_pkce).not.toBe(false);
    const denied = await app.request(authorizeUrl(client.client_id as string, {}));
    expect(denied.status).toBe(302);
    const callback = new URL(denied.headers.get("location") ?? "");
    expect(`${callback.origin}${callback.pathname}`).toBe(REDIRECT_URI);
    expect(callback.searchParams.get("error")).toBe("invalid_request");
    expect(callback.searchParams.has("code")).toBe(false);
  });

  it("matches redirect host/path/query exactly while allowing a loopback port change", async () => {
    const { app } = createService();
    const flow = await browserFlow(app);
    for (const redirect of [
      "http://localhost:33418/callback",
      "http://127.0.0.1:33418/other",
      `${REDIRECT_URI}?extra=1`,
    ]) {
      const res = await app.request(
        authorizeUrl(flow.clientId, {
          redirect_uri: redirect,
          code_challenge: pkce().challenge,
          code_challenge_method: "S256",
        }),
      );
      expect(res.status).toBe(302);
      const errorPage = new URL(res.headers.get("location") ?? "");
      expect(errorPage.origin).toBe(AUTH);
      expect(errorPage.pathname).toBe("/api/auth/error");
      expect(errorPage.searchParams.get("error")).toBe("invalid_redirect");
    }
    const valid = await app.request(
      authorizeUrl(flow.clientId, {
        redirect_uri: "http://127.0.0.1:49152/callback",
        code_challenge: pkce().challenge,
        code_challenge_method: "S256",
      }),
    );
    expect(valid.status).toBe(302);
    expect(valid.headers.get("location")).toContain(`${AUTH}/oauth/sign-in?`);
  });

  it("consumes authorization codes once and rejects expired codes", async () => {
    const { app } = createService();
    const flow = await browserFlow(app);
    const first = await flow.authorization();
    expect((await token(app, first.fields)).status).toBe(200);
    const replay = await token(app, first.fields);
    expect(replay.status).toBe(400);
    expect((await json(replay)).error).toBe("invalid_grant");
    const expired = await flow.authorization();
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(Date.now() + 601_000);
    const res = await token(app, expired.fields);
    expect(res.status).toBe(400);
    expect((await json(res)).error).toBe("invalid_grant");
  });

  it.each(["client_id", "redirect_uri", "resource"])(
    "rejects token-endpoint %s substitution",
    async (field) => {
      const { app } = createService();
      const flow = await browserFlow(app);
      const other = await browserFlow(app);
      const { fields } = await flow.authorization();
      const replacements = {
        client_id: other.clientId,
        redirect_uri: "http://127.0.0.1:49152/callback",
        resource: `${RESOURCE}/other`,
      };
      const res = await token(app, {
        ...fields,
        [field]: replacements[field as keyof typeof replacements],
      });
      expect(res.status).toBe(400);
      expect((await json(res)).error).toBe(
        field === "resource" ? "invalid_target" : "invalid_grant",
      );
    },
  );

  it("replays refresh responses only inside the grace window and reuse kills descendants", async () => {
    const { app } = createService();
    const flow = await browserFlow(app);
    const issued = await flow.issue();
    const rotated = await json(await flow.refresh(issued.refresh_token as string));
    expect(rotated.refresh_token).toBeTypeOf("string");
    expect(rotated.refresh_token).not.toBe(issued.refresh_token);
    const retry = await flow.refresh(issued.refresh_token as string);
    expect(retry.status).toBe(200);
    const replayed = await json(retry);
    expect(replayed).toMatchObject({ ...rotated, expires_in: expect.any(Number) });
    expect(replayed.expires_in as number).toBeLessThanOrEqual(rotated.expires_in as number);
    // A request with changed scopes cannot replay the cached response.
    const changed = await token(app, {
      grant_type: "refresh_token",
      client_id: flow.clientId,
      refresh_token: issued.refresh_token as string,
      scope: "openid",
      resource: RESOURCE,
    });
    expect(changed.status).toBe(400);
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(Date.now() + 31_000);
    const reuse = await flow.refresh(issued.refresh_token as string);
    expect(reuse.status).toBe(400);
    expect((await json(reuse)).error).toBe("invalid_grant");
    const descendant = await flow.refresh(rotated.refresh_token as string);
    expect(descendant.status).toBe(400);
    expect((await json(descendant)).error).toBe("invalid_grant");
  });

  it("rejects signed-query tampering and returns denial without a code", async () => {
    const { app } = createService();
    const flow = await browserFlow(app);
    const tampered = await flow.authorization(true, true);
    expect(tampered.consent.status).toBeGreaterThanOrEqual(400);
    const denied = await flow.authorization(false);
    expect(denied.callback?.searchParams.get("error")).toBe("access_denied");
    expect(denied.callback?.searchParams.has("code")).toBe(false);
    expect(denied.callback?.searchParams.get("state")).toBe("state-1");
  });

  it.each([
    ["register", OAUTH_REGISTRATION_RATE_LIMIT.max],
    ["token", 20],
  ] as const)(
    "rate-limits real /%s requests despite forged address headers",
    async (endpoint, limit) => {
      const { app } = createService(undefined, true);
      for (let i = 0; i <= limit; i++) {
        const forged = {
          "x-ariadshift-client-ip": `198.51.100.${i}`,
          "x-forwarded-for": `198.51.100.${i}`,
          "x-real-ip": `198.51.100.${i}`,
        };
        const res =
          endpoint === "register"
            ? await post(
                app,
                "/api/auth/oauth2/register",
                { redirect_uris: [REDIRECT_URI], token_endpoint_auth_method: "none" },
                forged,
              )
            : await token(
                app,
                { grant_type: "refresh_token", client_id: "missing", refresh_token: "missing" },
                forged,
              );
        if (i < limit) {
          if (endpoint === "register") expect(res.status).toBe(201);
          else expect(res.status).not.toBe(429);
        } else {
          expect(res.status).toBe(429);
          expect(Number(res.headers.get("x-retry-after"))).toBeGreaterThan(0);
        }
      }
      if (endpoint === "register") {
        vi.useFakeTimers({ toFake: ["Date"] });
        const start = Date.now();
        const register = () =>
          post(app, "/api/auth/oauth2/register", {
            redirect_uris: [REDIRECT_URI],
            token_endpoint_auth_method: "none",
          });
        vi.setSystemTime(start + 61_000);
        expect((await register()).status).toBe(429);
        vi.setSystemTime(start + (OAUTH_REGISTRATION_RATE_LIMIT.window + 1) * 1000);
        expect((await register()).status).toBe(201);
      }
    },
  );

  it.each(["/", "//"])(
    "refuses registration aliases ending in %s without storing clients",
    async (suffix) => {
      const { app, db } = createService(undefined, true);
      const response = await post(app, `/api/auth/oauth2/register${suffix}`, {
        redirect_uris: [REDIRECT_URI],
        token_endpoint_auth_method: "none",
      });
      expect(response.status).toBe(404);
      expect(db.oauthClient).toHaveLength(0);
    },
  );
});
