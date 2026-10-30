import { randomBytes } from "node:crypto";
import { describe, expect, it } from "vitest";
import { ConfigError, parseConfig } from "../src/config.ts";

const env = {
  DATABASE_URL: "postgres://unused@localhost/test",
  BETTER_AUTH_SECRET: randomBytes(32).toString("hex"),
  BETTER_AUTH_URL: "http://localhost:3001",
  BETTER_AUTH_TRUSTED_ORIGINS: "http://localhost:3000",
  ARIAD_PUBLIC_URL: "http://localhost:8080",
};

describe("auth secret placeholders", () => {
  it.each([
    "replace-with-openssl-rand-hex-32-auth-secret",
    "  replace-with-a-long-enough-public-template-secret  ",
  ])("refuses a template secret after trimming without disclosing it", (secret) => {
    expect(secret.trim().length).toBeGreaterThanOrEqual(32);
    let error: unknown;
    try {
      parseConfig({ ...env, BETTER_AUTH_SECRET: secret });
    } catch (caught) {
      error = caught;
    }
    expect(error).toBeInstanceOf(ConfigError);
    expect((error as Error).message).toContain("BETTER_AUTH_SECRET");
    expect((error as Error).message).not.toContain(secret.trim());
  });

  it("accepts a random signing secret and public client identifiers", () => {
    expect(
      parseConfig({
        ...env,
        GITHUB_CLIENT_ID: "replace-with-public-id",
        GITHUB_CLIENT_SECRET: "random-provider-secret",
      }).secret,
    ).toBe(env.BETTER_AUTH_SECRET);
  });

  it.each(["AUTH_TURNSTILE_SECRET", "GITHUB_CLIENT_SECRET", "GOOGLE_CLIENT_SECRET"])(
    "refuses an optional secret placeholder in %s without disclosing it",
    (name) => {
      const secret = "  replace-with-private-provider-value  ";
      expect(() => parseConfig({ ...env, [name]: secret })).toThrow(
        `${name} must be a generated secret, not a template placeholder`,
      );
    },
  );
});
