import { describe, expect, it } from "vitest";
import { ConfigError, parseConfig } from "../src/config.ts";

const SECRET = "0123456789abcdef0123456789abcdef";

function env(overrides: Record<string, string | undefined> = {}) {
  return {
    DATABASE_URL: "postgres://ariad_auth:pw@postgres:5432/ariadshift",
    BETTER_AUTH_SECRET: SECRET,
    BETTER_AUTH_URL: "http://localhost:3001",
    BETTER_AUTH_TRUSTED_ORIGINS: "http://localhost:3000",
    ...overrides,
  };
}

describe("parseConfig", () => {
  it("accepts the compose environment with defaults", () => {
    const config = parseConfig(env());
    expect(config).toEqual({
      databaseUrl: "postgres://ariad_auth:pw@postgres:5432/ariadshift",
      secret: SECRET,
      baseUrl: "http://localhost:3001",
      trustedOrigins: ["http://localhost:3000"],
      port: 3000,
      host: "0.0.0.0",
      trustedClientIpHeader: undefined,
      passkeyRpId: "localhost",
      github: undefined,
      google: undefined,
    });
  });

  it("requires the database URL, secret, base URL and web origin", () => {
    for (const name of [
      "DATABASE_URL",
      "BETTER_AUTH_SECRET",
      "BETTER_AUTH_URL",
      "BETTER_AUTH_TRUSTED_ORIGINS",
    ]) {
      expect(() => parseConfig(env({ [name]: undefined })), name).toThrow(ConfigError);
      expect(() => parseConfig(env({ [name]: "   " })), name).toThrow(ConfigError);
    }
  });

  it("rejects a short secret", () => {
    expect(() => parseConfig(env({ BETTER_AUTH_SECRET: "short" }))).toThrow(/at least 32/);
  });

  it("rejects a non-Postgres database URL", () => {
    expect(() => parseConfig(env({ DATABASE_URL: "mysql://db/app" }))).toThrow(/postgres/);
    expect(() => parseConfig(env({ DATABASE_URL: "not a url" }))).toThrow(ConfigError);
  });

  it("keeps the base URL an origin so it matches the API's iss and aud", () => {
    expect(parseConfig(env({ BETTER_AUTH_URL: "https://auth.example.com/" })).baseUrl).toBe(
      "https://auth.example.com",
    );
    for (const bad of [
      "https://auth.example.com/api/auth",
      "https://auth.example.com/?x=1",
      "ftp://auth.example.com",
      "https://user:pw@auth.example.com",
      "auth.example.com",
    ]) {
      expect(() => parseConfig(env({ BETTER_AUTH_URL: bad })), bad).toThrow(ConfigError);
    }
  });

  it("parses, normalises and deduplicates trusted origins", () => {
    const config = parseConfig(
      env({
        BETTER_AUTH_TRUSTED_ORIGINS:
          " https://app.example.com , https://APP.example.com/,https://admin.example.com ",
      }),
    );
    expect(config.trustedOrigins).toEqual(["https://app.example.com", "https://admin.example.com"]);
    expect(() =>
      parseConfig(env({ BETTER_AUTH_TRUSTED_ORIGINS: "https://app.example.com/path" })),
    ).toThrow(ConfigError);
  });

  it("accepts AUTH_TRUSTED_ORIGINS as an alias", () => {
    const config = parseConfig(
      env({
        BETTER_AUTH_TRUSTED_ORIGINS: undefined,
        AUTH_TRUSTED_ORIGINS: "https://app.example.com",
      }),
    );
    expect(config.trustedOrigins).toEqual(["https://app.example.com"]);
  });

  it("validates the port", () => {
    expect(parseConfig(env({ PORT: "8081" })).port).toBe(8081);
    for (const bad of ["0", "65536", "80a", "-1", "3.5"]) {
      expect(() => parseConfig(env({ PORT: bad })), bad).toThrow(/PORT/);
    }
  });

  it("enables an OAuth provider only when both of its values are set", () => {
    const both = parseConfig(env({ GITHUB_CLIENT_ID: "id", GITHUB_CLIENT_SECRET: "secret" }));
    expect(both.github).toEqual({ clientId: "id", clientSecret: "secret" });
    expect(both.google).toBeUndefined();

    // Compose passes empty strings for unset providers.
    const empty = parseConfig(env({ GOOGLE_CLIENT_ID: "", GOOGLE_CLIENT_SECRET: "" }));
    expect(empty.google).toBeUndefined();

    expect(() => parseConfig(env({ GOOGLE_CLIENT_ID: "id" }))).toThrow(/set together/);
    expect(() => parseConfig(env({ GITHUB_CLIENT_SECRET: "secret" }))).toThrow(/set together/);
  });

  it("derives the passkey relying party from the web origin unless overridden", () => {
    expect(
      parseConfig(env({ BETTER_AUTH_TRUSTED_ORIGINS: "https://app.example.com:8443" })).passkeyRpId,
    ).toBe("app.example.com");
    expect(parseConfig(env({ AUTH_PASSKEY_RP_ID: "example.com" })).passkeyRpId).toBe(
      "example.com",
    );
  });

  it("lowercases a trusted client IP header and rejects invalid names", () => {
    expect(
      parseConfig(env({ AUTH_TRUSTED_CLIENT_IP_HEADER: "CF-Connecting-IP" })).trustedClientIpHeader,
    ).toBe("cf-connecting-ip");
    expect(() => parseConfig(env({ AUTH_TRUSTED_CLIENT_IP_HEADER: "bad header" }))).toThrow(
      ConfigError,
    );
  });
});
