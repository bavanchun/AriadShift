import pg from "pg";
import { afterAll, describe, expect, it } from "vitest";
import {
  API_KEY_PREFIX,
  BASE_PATH,
  buildAuthOptions,
  CLIENT_IP_HEADER,
  JWT_LIFETIME,
} from "../src/auth.ts";
import { type AuthServiceConfig, parseConfig } from "../src/config.ts";

// A pool connects lazily, so building options never touches a database.
const pool = new pg.Pool({ connectionString: "postgres://nobody@127.0.0.1:1/none" });
afterAll(() => pool.end());

function config(overrides: Record<string, string> = {}): AuthServiceConfig {
  return parseConfig({
    DATABASE_URL: "postgres://ariad_auth:pw@postgres:5432/ariadshift",
    BETTER_AUTH_SECRET: "0123456789abcdef0123456789abcdef",
    BETTER_AUTH_URL: "http://localhost:3001",
    BETTER_AUTH_TRUSTED_ORIGINS: "http://localhost:3000",
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
  it("installs exactly the JWT, API key, passkey and two-factor plugins", () => {
    const ids = buildAuthOptions(config(), pool).plugins.map((p) => p.id);
    expect(ids.sort()).toEqual(["api-key", "jwt", "passkey", "two-factor"]);
  });

  it("mounts at /api/auth so the JWKS is at /api/auth/jwks", () => {
    const options = buildAuthOptions(config(), pool);
    expect(BASE_PATH).toBe("/api/auth");
    expect(options.basePath).toBe(BASE_PATH);
    expect(options.baseURL).toBe("http://localhost:3001");
    expect(plugin(options, "jwt").options?.jwks).toEqual({
      keyPairConfig: { alg: "EdDSA", crv: "Ed25519" },
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

  it("binds passkeys to the web app origin", () => {
    const passkey = plugin(
      buildAuthOptions(config({ BETTER_AUTH_TRUSTED_ORIGINS: "https://app.example.com" }), pool),
      "passkey",
    );
    expect(passkey.options).toMatchObject({
      rpID: "app.example.com",
      rpName: "AriadShift",
      origin: ["https://app.example.com"],
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
      github: { clientId: "gh-id", clientSecret: "gh-secret" },
      google: { clientId: "g-id", clientSecret: "g-secret" },
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

  it("does not require email verification and sends no telemetry", () => {
    const options = buildAuthOptions(config(), pool);
    expect(options.emailAndPassword).toEqual({ enabled: true, requireEmailVerification: false });
    expect(options.telemetry).toEqual({ enabled: false });
    expect(options.database).toBe(pool);
  });
});
