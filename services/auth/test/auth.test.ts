import { memoryAdapter } from "better-auth/adapters/memory";
import { hashPassword, makeSignature } from "better-auth/crypto";
import { getAuthTables } from "better-auth/db";
import pg from "pg";
import { afterAll, describe, expect, it, vi } from "vitest";
import { createApp } from "../src/app.ts";
import {
  API_KEY_PREFIX,
  BASE_PATH,
  buildAuthOptions,
  CLIENT_IP_HEADER,
  createAuth,
  discoveryDocuments,
  JWT_LIFETIME,
  OAUTH_ACCESS_TOKEN_LIFETIME_S,
  OAUTH_CONSENT_PATH,
  OAUTH_REGISTRATION_RATE_LIMIT,
  OAUTH_SIGN_IN_PATH,
} from "../src/auth.ts";
import { type AuthServiceConfig, parseConfig } from "../src/config.ts";
import { pageOptions } from "../src/pages.ts";

// A pool connects lazily, so building options never touches a database.
const pool = new pg.Pool({ connectionString: "postgres://nobody@127.0.0.1:1/none" });
afterAll(() => pool.end());

function config(overrides: Record<string, string> = {}): AuthServiceConfig {
  return parseConfig({
    DATABASE_URL: "postgres://ariad_auth:pw@postgres:5432/ariadshift",
    BETTER_AUTH_SECRET: "0123456789abcdef0123456789abcdef",
    BETTER_AUTH_URL: "http://localhost:3001",
    BETTER_AUTH_TRUSTED_ORIGINS: "http://localhost:3000",
    ARIAD_PUBLIC_URL: "http://localhost:8080",
    ...overrides,
  });
}

function plugin(options: ReturnType<typeof buildAuthOptions>, id: string) {
  const found = options.plugins.find((p) => p.id === id);
  if (!found) throw new Error(`plugin ${id} is missing`);
  // Plugins carry differently typed options; the tests read them loosely.
  return found as unknown as { id: string; options?: Record<string, unknown>; schema?: unknown };
}

describe("buildAuthOptions", () => {
  it("installs exactly the JWT, API key, passkey, two-factor and OAuth provider plugins", () => {
    const ids = buildAuthOptions(config(), pool).plugins.map((p) => p.id);
    expect(ids.sort()).toEqual(["api-key", "jwt", "oauth-provider", "passkey", "two-factor"]);
  });

  it("mounts at /api/auth so the JWKS is at /api/auth/jwks", () => {
    const options = buildAuthOptions(config(), pool);
    expect(BASE_PATH).toBe("/api/auth");
    expect(options.basePath).toBe(BASE_PATH);
    expect(options.baseURL).toBe("http://localhost:3001");
    expect(plugin(options, "jwt").options?.jwks).toEqual({
      keyPairConfig: { alg: "EdDSA", crv: "Ed25519" },
      rotationInterval: 2592000,
      gracePeriod: 1800,
    });
  });

  it("issues 15-minute tokens whose iss and aud are the service origin", async () => {
    const options = buildAuthOptions(config({ BETTER_AUTH_URL: "https://auth.example.com" }), pool);
    const jwt = plugin(options, "jwt").options?.jwt as {
      issuer: string;
      audience: string;
      expirationTime: string;
      definePayload: () => unknown;
    };
    expect(JWT_LIFETIME).toBe("15m");
    expect(jwt.expirationTime).toBe(JWT_LIFETIME);
    expect(jwt.issuer).toBe("https://auth.example.com");
    expect(jwt.audience).toBe("https://auth.example.com");
    // Only registered claims; `sub` stays Better Auth's default, the user id.
    expect(await jwt.definePayload()).toEqual({});
  });

  it("creates API keys in the apikey table ariad-server reads", () => {
    const apiKey = plugin(buildAuthOptions(config(), pool), "api-key");
    const schema = apiKey.schema as { apikey: { fields: Record<string, unknown> } };
    expect(Object.keys(schema.apikey.fields)).toEqual(
      expect.arrayContaining([
        "key",
        "referenceId",
        "configId",
        "enabled",
        "expiresAt",
        "remaining",
        "rateLimitEnabled",
      ]),
    );
    expect(API_KEY_PREFIX).toBe("ashift_");
  });

  it("binds passkeys to the web app and to this service's sign-in page", () => {
    const passkey = plugin(
      buildAuthOptions(
        config({
          BETTER_AUTH_URL: "https://auth.example.com",
          BETTER_AUTH_TRUSTED_ORIGINS: "https://app.example.com",
          AUTH_PASSKEY_RP_ID: "example.com",
        }),
        pool,
      ),
      "passkey",
    );
    expect(passkey.options).toMatchObject({
      rpID: "example.com",
      rpName: "AriadShift",
      origin: ["https://app.example.com", "https://auth.example.com"],
    });
  });

  it("names the app in authenticator entries", () => {
    expect(plugin(buildAuthOptions(config(), pool), "two-factor").options).toMatchObject({
      issuer: "AriadShift",
    });
  });

  it("enables social providers only when configured", () => {
    expect(buildAuthOptions(config(), pool).socialProviders).toEqual({});
    const both = buildAuthOptions(
      config({
        GITHUB_CLIENT_ID: "gh-id",
        GITHUB_CLIENT_SECRET: "gh-secret",
        GOOGLE_CLIENT_ID: "g-id",
        GOOGLE_CLIENT_SECRET: "g-secret",
      }),
      pool,
    );
    expect(both.socialProviders).toEqual({
      github: { clientId: "gh-id", clientSecret: "gh-secret", disableSignUp: true },
      google: { clientId: "g-id", clientSecret: "g-secret", disableSignUp: true },
    });
  });

  it("keeps rate limiting on, shared in the database, keyed by the pinned client header", () => {
    const options = buildAuthOptions(config(), pool);
    expect(options.rateLimit).toMatchObject({ enabled: true, storage: "database" });
    expect(options.advanced.ipAddress.ipAddressHeaders).toEqual([CLIENT_IP_HEADER]);
  });

  it("trusts the web origins and sets secure cookies only over https", () => {
    const local = buildAuthOptions(config(), pool);
    expect(local.trustedOrigins).toEqual(["http://localhost:3000"]);
    expect(local.advanced.useSecureCookies).toBe(false);
    expect(local.advanced.defaultCookieAttributes).toEqual({ sameSite: "lax", httpOnly: true });

    const tls = buildAuthOptions(config({ BETTER_AUTH_URL: "https://auth.example.com" }), pool);
    expect(tls.advanced.useSecureCookies).toBe(true);
  });

  it("lets users delete their own account without an emailed confirmation", () => {
    const options = buildAuthOptions(config(), pool);
    expect(options.user.deleteUser.enabled).toBe(true);
  });

  it("revokes every API key of the deleted user without touching other owners", async () => {
    const query = vi.fn().mockResolvedValue({ rowCount: 2 });
    const database = { query } as unknown as pg.Pool;
    const options = buildAuthOptions(config(), database);
    await options.user.deleteUser.beforeDelete({
      id: "user-'1",
      name: "Ada",
      email: "ada@example.com",
      emailVerified: false,
      createdAt: new Date(),
      updatedAt: new Date(),
    });
    expect(query).toHaveBeenCalledExactlyOnceWith(
      'DELETE FROM "auth"."apikey" WHERE "referenceId" = $1',
      ["user-'1"],
    );
  });

  it("refuses account deletion when API key revocation fails", async () => {
    const failure = new Error("database unavailable");
    const database = { query: vi.fn().mockRejectedValue(failure) } as unknown as pg.Pool;
    const options = buildAuthOptions(config(), database);
    await expect(
      options.user.deleteUser.beforeDelete({
        id: "user-1",
        name: "Ada",
        email: "ada@example.com",
        emailVerified: false,
        createdAt: new Date(),
        updatedAt: new Date(),
      }),
    ).rejects.toBe(failure);
  });

  it("refuses account deletion when its database cannot revoke API keys", async () => {
    const options = buildAuthOptions(config(), memoryAdapter({}));
    await expect(
      options.user.deleteUser.beforeDelete({
        id: "user-1",
        name: "Ada",
        email: "ada@example.com",
        emailVerified: false,
        createdAt: new Date(),
        updatedAt: new Date(),
      }),
    ).rejects.toThrow("API key revocation requires a PostgreSQL connection");
  });

  it("does not require email verification and sends no telemetry", () => {
    const options = buildAuthOptions(config(), pool);
    expect(options.emailAndPassword).toEqual({
      enabled: true,
      disableSignUp: true,
      requireEmailVerification: false,
    });
    expect(options.telemetry).toEqual({ enabled: false });
    expect(options.database).toBe(pool);
  });

  describe("OAuth authorization server for remote MCP", () => {
    function oauth(overrides: Record<string, string> = {}) {
      return plugin(buildAuthOptions(config(overrides), pool), "oauth-provider").options as Record<
        string,
        unknown
      >;
    }

    it("issues tokens only for the MCP resource the API derives from its public URL", () => {
      const options = oauth({ ARIAD_PUBLIC_URL: "https://api.example.com" });
      expect(options.resources).toEqual(["https://api.example.com/mcp"]);
      expect(options.clientRegistrationDefaultResources).toEqual(["https://api.example.com/mcp"]);
      // Better Auth's default: a client is limited to the resources it is linked to.
      expect(options.enforcePerClientResources).toBeUndefined();
    });

    it("offers authorization code and refresh only, never client credentials", () => {
      expect(oauth().grantTypes).toEqual(["authorization_code", "refresh_token"]);
    });

    it("opens dynamic client registration without an account and keeps PKCE required", () => {
      const options = oauth();
      expect(options).toMatchObject({
        allowDynamicClientRegistration: true,
        allowUnauthenticatedClientRegistration: true,
        clientRegistrationRequirePKCE: true,
        clientRegistrationDefaultApplicationType: "infer",
      });
    });

    it("limits anonymous client registration per address far below Better Auth's default", () => {
      expect(OAUTH_REGISTRATION_RATE_LIMIT).toEqual({ window: 3600, max: 20 });
      expect(oauth().rateLimit).toEqual({ register: { window: 3600, max: 20 } });
      // The plugin turns the option into a rule for the registration path.
      const rules = (
        plugin(buildAuthOptions(config(), pool), "oauth-provider") as unknown as {
          rateLimit: { pathMatcher: (path: string) => boolean; window: number; max: number }[];
        }
      ).rateLimit;
      const register = rules.filter((rule) => rule.pathMatcher("/oauth2/register"));
      expect(register).toHaveLength(1);
      expect(register[0]).toMatchObject({ window: 3600, max: 20 });
    });

    it("runs the two-factor check before the provider resumes an authorization", () => {
      // Both plugins have after-hooks on /sign-in/email. Two-factor must clear
      // the half-signed-in session first, so the provider never resumes an
      // authorization (and issues a code) before the second factor.
      const ids = buildAuthOptions(config(), pool).plugins.map((p) => p.id);
      expect(ids.indexOf("two-factor")).toBeGreaterThanOrEqual(0);
      expect(ids.indexOf("two-factor")).toBeLessThan(ids.indexOf("oauth-provider"));
    });

    it("sends sign-in and consent to this service's pages", () => {
      const options = oauth({ BETTER_AUTH_URL: "https://auth.example.com" });
      expect(OAUTH_SIGN_IN_PATH).toBe("/oauth/sign-in");
      expect(OAUTH_CONSENT_PATH).toBe("/oauth/consent");
      expect(options.loginPage).toBe("https://auth.example.com/oauth/sign-in");
      expect(options.consentPage).toBe("https://auth.example.com/oauth/consent");
    });

    it("keeps access tokens as short-lived as the API JWT, with a refresh retry window", () => {
      const options = oauth();
      expect(OAUTH_ACCESS_TOKEN_LIFETIME_S).toBe(15 * 60);
      expect(options.accessTokenExpiresIn).toBe(OAUTH_ACCESS_TOKEN_LIFETIME_S);
      expect(options.refreshTokenReuseInterval).toBe(30);
      expect(options.scopes).toEqual(["openid", "profile", "email", "offline_access"]);
    });

    it("signs access tokens with the JWT plugin, so they share its issuer and JWKS", () => {
      const options = oauth();
      expect(options.disableJwtPlugin).toBe(false);
      expect(plugin(buildAuthOptions(config(), pool), "jwt").options?.jwt).toMatchObject({
        issuer: "http://localhost:3001",
      });
    });

    it("adds the OAuth tables to the schema the migration applies", () => {
      const schema = plugin(buildAuthOptions(config(), pool), "oauth-provider").schema as Record<
        string,
        unknown
      >;
      expect(Object.keys(schema)).toEqual(
        expect.arrayContaining([
          "oauthClient",
          "oauthAccessToken",
          "oauthRefreshToken",
          "oauthConsent",
          "oauthResource",
          "oauthClientResource",
        ]),
      );
    });
  });

  describe("account deletion", () => {
    async function seededService(freshSession = true) {
      const serviceConfig = config();
      const probe = buildAuthOptions(serviceConfig, memoryAdapter({}));
      const db: Record<string, unknown[]> = Object.fromEntries(
        Object.values(getAuthTables(probe)).map((table) => [table.modelName, []]),
      );
      const now = new Date();
      const stale = new Date(now.getTime() - 2 * 24 * 60 * 60 * 1000);
      const future = new Date(now.getTime() + 60 * 60 * 1000);
      const passwordHash = await hashPassword("correct horse battery staple");

      db.user = [
        {
          id: "user-social",
          name: "Social User",
          email: "social@example.com",
          emailVerified: true,
          createdAt: stale,
          updatedAt: now,
        },
        {
          id: "user-other",
          name: "Other User",
          email: "other@example.com",
          emailVerified: true,
          createdAt: stale,
          updatedAt: now,
        },
        {
          id: "user-password",
          name: "Password User",
          email: "password@example.com",
          emailVerified: false,
          createdAt: stale,
          updatedAt: now,
        },
      ];
      db.account = [
        {
          id: "acc-social",
          userId: "user-social",
          providerId: "github",
          accountId: "gh-101",
          createdAt: stale,
          updatedAt: now,
        },
        {
          id: "acc-other",
          userId: "user-other",
          providerId: "google",
          accountId: "g-202",
          createdAt: stale,
          updatedAt: now,
        },
        {
          id: "acc-password",
          userId: "user-password",
          providerId: "credential",
          accountId: "user-password",
          password: passwordHash,
          createdAt: stale,
          updatedAt: now,
        },
      ];
      db.session = [
        {
          id: "sess-social",
          userId: "user-social",
          token: "tok-social",
          createdAt: freshSession ? now : stale,
          updatedAt: now,
          expiresAt: future,
        },
        {
          id: "sess-other",
          userId: "user-other",
          token: "tok-other",
          createdAt: now,
          updatedAt: now,
          expiresAt: future,
        },
        {
          id: "sess-password",
          userId: "user-password",
          token: "tok-password",
          createdAt: now,
          updatedAt: now,
          expiresAt: future,
        },
      ];
      db.apikey = [
        { id: "key-social", referenceId: "user-social", key: "h1", enabled: true },
        { id: "key-other", referenceId: "user-other", key: "h2", enabled: true },
      ];

      const database = Object.assign(memoryAdapter(db), {
        query: vi.fn(async (_sql: string, [userId]: [string]) => {
          db.apikey = (db.apikey ?? []).filter(
            (row) => (row as { referenceId?: string }).referenceId !== userId,
          );
          return { rowCount: 1 };
        }),
      });
      const options = buildAuthOptions(serviceConfig, database);
      const auth = createAuth({ ...options, rateLimit: { ...options.rateLimit, enabled: false } });
      const app = createApp({
        config: serviceConfig,
        handler: (request) => auth.handler(request),
        metadata: discoveryDocuments(auth),
        pages: pageOptions(serviceConfig),
        peerAddress: () => "127.0.0.1",
      });

      async function sessionCookie(tokenValue: string): Promise<string> {
        const sig = await makeSignature(tokenValue, serviceConfig.secret);
        return `ariadshift.session_token=${encodeURIComponent(`${tokenValue}.${sig}`)}`;
      }

      async function deleteRequest(tokenValue: string, body: Record<string, unknown> = {}) {
        return app.request("http://localhost:3001/api/auth/delete-user", {
          method: "POST",
          headers: {
            "content-type": "application/json",
            accept: "application/json",
            origin: "http://localhost:3001",
            cookie: await sessionCookie(tokenValue),
          },
          body: JSON.stringify(body),
        });
      }

      return { db, deleteRequest };
    }

    it("deletes a social-only user and its sessions with a fresh session while leaving another user untouched", async () => {
      const { db, deleteRequest } = await seededService(true);
      const response = await deleteRequest("tok-social");
      expect(response.status).toBe(200);
      expect(await response.json()).toEqual({ success: true, message: "User deleted" });

      expect((db.user as { id: string }[]).map((u) => u.id)).toEqual([
        "user-other",
        "user-password",
      ]);
      expect((db.session as { userId: string }[]).map((s) => s.userId)).toEqual([
        "user-other",
        "user-password",
      ]);
      expect((db.apikey as { referenceId: string }[]).map((k) => k.referenceId)).toEqual([
        "user-other",
      ]);
    });

    it("refuses social-only account deletion when the session is stale", async () => {
      const { db, deleteRequest } = await seededService(false);
      const response = await deleteRequest("tok-social");
      expect(response.status).toBe(400);
      expect((await response.json()) as { code?: string }).toMatchObject({
        code: "SESSION_EXPIRED",
      });
      expect((db.user as { id: string }[]).map((u) => u.id)).toContain("user-social");
      expect((db.session as { id: string }[]).map((s) => s.id)).toContain("sess-social");
    });

    it("requires the password before deleting a password-backed account even with a fresh session", async () => {
      const { db, deleteRequest } = await seededService(true);

      const withoutPassword = await deleteRequest("tok-password", {});
      expect(withoutPassword.status).toBe(400);
      expect((await withoutPassword.json()) as { code?: string }).toMatchObject({
        code: "PASSWORD_REQUIRED",
      });
      expect((db.user as { id: string }[]).map((u) => u.id)).toContain("user-password");

      const wrongPassword = await deleteRequest("tok-password", { password: "wrong password" });
      expect(wrongPassword.status).toBe(400);
      expect((db.user as { id: string }[]).map((u) => u.id)).toContain("user-password");

      const withPassword = await deleteRequest("tok-password", {
        password: "correct horse battery staple",
      });
      expect(withPassword.status).toBe(200);
      expect((db.user as { id: string }[]).map((u) => u.id)).not.toContain("user-password");
    });
  });

  describe("OAuth consents and registered clients", () => {
    async function seededOAuthService() {
      const serviceConfig = config();
      const probe = buildAuthOptions(serviceConfig, memoryAdapter({}));
      const db: Record<string, unknown[]> = Object.fromEntries(
        Object.values(getAuthTables(probe)).map((table) => [table.modelName, []]),
      );
      const now = new Date("2026-10-30T10:00:00.000Z");
      const future = new Date(Date.now() + 60 * 60 * 1000);

      db.user = [
        {
          id: "user-1",
          name: "Ada",
          email: "ada@example.com",
          emailVerified: true,
          createdAt: now,
          updatedAt: now,
        },
        {
          id: "user-2",
          name: "Grace",
          email: "grace@example.com",
          emailVerified: true,
          createdAt: now,
          updatedAt: now,
        },
      ];
      db.session = [
        {
          id: "sess-1",
          userId: "user-1",
          token: "tok-1",
          createdAt: now,
          updatedAt: now,
          expiresAt: future,
        },
        {
          id: "sess-2",
          userId: "user-2",
          token: "tok-2",
          createdAt: now,
          updatedAt: now,
          expiresAt: future,
        },
      ];
      db.oauthClient = [
        {
          id: "row-client-1",
          clientId: "client-owned-1",
          name: "Ada MCP Client",
          uri: "https://ada.example.com",
          userId: "user-1",
          redirectUris: ["http://127.0.0.1:33418/callback"],
          scopes: ["openid", "offline_access"],
          grantTypes: ["authorization_code", "refresh_token"],
          responseTypes: ["code"],
          tokenEndpointAuthMethod: "none",
          createdAt: now,
          updatedAt: now,
        },
        {
          id: "row-client-2",
          clientId: "client-owned-2",
          name: "Grace MCP Client",
          uri: "https://grace.example.com",
          userId: "user-2",
          redirectUris: ["http://127.0.0.1:33419/callback"],
          scopes: ["openid", "offline_access"],
          grantTypes: ["authorization_code", "refresh_token"],
          responseTypes: ["code"],
          tokenEndpointAuthMethod: "none",
          createdAt: now,
          updatedAt: now,
        },
      ];
      db.oauthConsent = [
        {
          id: "consent-1",
          clientId: "client-owned-2",
          userId: "user-1",
          scopes: ["openid", "offline_access"],
          createdAt: now,
          updatedAt: now,
        },
        {
          id: "consent-2",
          clientId: "client-owned-1",
          userId: "user-2",
          scopes: ["openid"],
          createdAt: now,
          updatedAt: now,
        },
      ];
      db.oauthRefreshToken = [
        {
          id: "rt-1",
          token: "hashed-rt-1",
          clientId: "client-owned-2",
          userId: "user-1",
          sessionId: "sess-1",
          scopes: ["openid", "offline_access"],
          createdAt: now,
          expiresAt: future,
        },
        {
          id: "rt-2",
          token: "hashed-rt-2",
          clientId: "client-owned-1",
          userId: "user-2",
          sessionId: "sess-2",
          scopes: ["openid", "offline_access"],
          createdAt: now,
          expiresAt: future,
        },
      ];
      db.oauthAccessToken = [
        {
          id: "at-1",
          token: "hashed-at-1",
          clientId: "client-owned-2",
          userId: "user-1",
          refreshId: "rt-1",
          scopes: ["openid", "offline_access"],
          createdAt: now,
          expiresAt: future,
        },
      ];

      const options = buildAuthOptions(serviceConfig, memoryAdapter(db));
      const auth = createAuth({ ...options, rateLimit: { ...options.rateLimit, enabled: false } });
      const app = createApp({
        config: serviceConfig,
        handler: (request) => auth.handler(request),
        metadata: discoveryDocuments(auth),
        pages: pageOptions(serviceConfig),
        peerAddress: () => "127.0.0.1",
      });

      async function requestAs(
        tokenValue: string,
        path: string,
        body?: Record<string, unknown>,
      ): Promise<Response> {
        const sig = await makeSignature(tokenValue, serviceConfig.secret);
        return app.request(`http://localhost:3001/api/auth${path}`, {
          method: body ? "POST" : "GET",
          headers: {
            accept: "application/json",
            origin: "http://localhost:3001",
            cookie: `ariadshift.session_token=${encodeURIComponent(`${tokenValue}.${sig}`)}`,
            ...(body ? { "content-type": "application/json" } : {}),
          },
          ...(body ? { body: JSON.stringify(body) } : {}),
        });
      }

      return { db, requestAs };
    }

    it("lists only the signed-in user's consents (with client name) and registered clients", async () => {
      const { requestAs } = await seededOAuthService();

      const consentsRes = await requestAs("tok-1", "/oauth2/get-consents");
      expect(consentsRes.status).toBe(200);
      const consents = (await consentsRes.json()) as Record<string, unknown>[];
      expect(consents).toHaveLength(1);
      expect(consents[0]).toMatchObject({
        id: "consent-1",
        clientId: "client-owned-2",
        clientName: "Grace MCP Client",
        scopes: ["openid", "offline_access"],
      });

      const clientsRes = await requestAs("tok-1", "/oauth2/get-clients");
      expect(clientsRes.status).toBe(200);
      const clients = (await clientsRes.json()) as Record<string, unknown>[];
      expect(clients).toHaveLength(1);
      expect(clients[0]).toMatchObject({
        client_id: "client-owned-1",
        client_name: "Ada MCP Client",
      });
    });

    it("refuses revoking another user's consent or registered client", async () => {
      const { db, requestAs } = await seededOAuthService();

      const revokeOtherConsent = await requestAs("tok-1", "/oauth2/delete-consent", {
        id: "consent-2",
      });
      expect(revokeOtherConsent.status).toBe(401);
      expect((db.oauthConsent as { id: string }[]).map((c) => c.id)).toContain("consent-2");
      expect((db.oauthRefreshToken as { id: string }[]).map((t) => t.id)).toContain("rt-2");

      const revokeOtherClient = await requestAs("tok-1", "/oauth2/delete-client", {
        client_id: "client-owned-2",
      });
      expect(revokeOtherClient.status).toBe(401);
      expect((db.oauthClient as { clientId: string }[]).map((c) => c.clientId)).toContain(
        "client-owned-2",
      );
    });

    it("removes the consent and revokes active tokens when the owner revokes a consent or client", async () => {
      const { db, requestAs } = await seededOAuthService();

      const revokeConsent = await requestAs("tok-1", "/oauth2/delete-consent", {
        id: "consent-1",
      });
      expect(revokeConsent.status).toBe(200);
      expect((db.oauthConsent as { id: string }[]).map((c) => c.id)).toEqual(["consent-2"]);
      expect((db.oauthRefreshToken as { id: string }[]).map((t) => t.id)).toEqual(["rt-2"]);
      expect(db.oauthAccessToken).toEqual([]);

      const revokeClient = await requestAs("tok-1", "/oauth2/delete-client", {
        client_id: "client-owned-1",
      });
      expect(revokeClient.status).toBe(200);
      expect((db.oauthClient as { clientId: string }[]).map((c) => c.clientId)).toEqual([
        "client-owned-2",
      ]);
      expect(db.oauthConsent).toEqual([]);
      expect(db.oauthRefreshToken).toEqual([]);
    });
  });
});
