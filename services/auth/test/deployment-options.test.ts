import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { memoryAdapter } from "better-auth/adapters/memory";
import { getAuthTables } from "better-auth/db";
import { describe, expect, it } from "vitest";
import { createApp } from "../src/app.ts";
import { buildAuthOptions, createAuth, discoveryDocuments } from "../src/auth.ts";
import { parseConfig } from "../src/config.ts";
import { pageOptions } from "../src/pages.ts";
import { Jar } from "./oauth-helpers.ts";

const environment = {
  DATABASE_URL: "postgres://unused@127.0.0.1:1/none",
  BETTER_AUTH_SECRET: "0123456789abcdef0123456789abcdef",
  BETTER_AUTH_URL: "http://localhost:3001",
  BETTER_AUTH_TRUSTED_ORIGINS: "http://localhost:3000",
  ARIAD_PUBLIC_URL: "http://localhost:8080",
};

function service(overrides: Record<string, string> = {}) {
  const config = parseConfig({ ...environment, ...overrides });
  const probe = buildAuthOptions(config, memoryAdapter({}));
  const db = Object.fromEntries(
    Object.values(getAuthTables(probe)).map((table) => [table.modelName, []]),
  );
  const options = buildAuthOptions(config, memoryAdapter(db));
  const auth = createAuth({ ...options, rateLimit: { ...options.rateLimit, enabled: false } });
  const app = createApp({
    config,
    handler: auth.handler,
    metadata: discoveryDocuments(auth),
    pages: pageOptions(config),
    peerAddress: () => "127.0.0.1",
  });
  const signup = () =>
    app.request(`${config.baseUrl}/api/auth/sign-up/email`, {
      method: "POST",
      headers: { "content-type": "application/json", origin: config.baseUrl },
      body: JSON.stringify({
        name: "Deployment",
        email: "deployment@example.com",
        password: "correct horse battery staple",
      }),
    });
  return { app, config, options, signup };
}

describe("explicit deployment options", () => {
  it("refuses signup by default at the real endpoint", async () => {
    const { app, config, signup } = service();
    const refused = await signup();
    expect(refused.status).toBe(400);
    expect(await refused.json()).toMatchObject({ code: "EMAIL_PASSWORD_SIGN_UP_DISABLED" });
    expect(
      await (
        await app.request(`${config.baseUrl}/api/auth/sign-in/email`, {
          method: "POST",
          headers: { "content-type": "application/json", origin: config.baseUrl },
          body: JSON.stringify({
            email: "deployment@example.com",
            password: "correct horse battery staple",
          }),
        })
      ).json(),
    ).toMatchObject({ code: "INVALID_EMAIL_OR_PASSWORD" });
  });

  it("creates an unverified account only after explicit signup opt-in", async () => {
    const { signup } = service({ AUTH_ALLOW_SIGN_UP: "true" });
    const response = await signup();
    expect(response.status).toBe(200);
    expect(await response.json()).toMatchObject({ user: { emailVerified: false } });
  });

  it.each(["AUTH_ALLOW_SIGN_UP", "AUTH_ALLOW_INSECURE_HTTP"])(
    "rejects invalid values for %s",
    (name) => {
      for (const value of ["1", "0", "yes", "TRUE", "False"]) {
        expect(() => parseConfig({ ...environment, [name]: value })).toThrow(name);
      }
    },
  );

  it.each(["BETTER_AUTH_URL", "ARIAD_PUBLIC_URL"])(
    "requires explicit opt-in for LAN HTTP in %s",
    (name) => {
      expect(() => parseConfig({ ...environment, [name]: "http://nas.local:3001" })).toThrow(
        /https/,
      );
      expect(() =>
        parseConfig({
          ...environment,
          [name]: "http://nas.local:3001",
          AUTH_ALLOW_INSECURE_HTTP: "false",
        }),
      ).toThrow(/https/);
    },
  );

  it.each(["BETTER_AUTH_URL", "ARIAD_PUBLIC_URL"])(
    "keeps accounts, sessions, JWT and API keys on opted-in LAN HTTP in %s",
    async (name) => {
      const { app, config, options, signup } = service({
        [name]: "http://nas.local:3001",
        AUTH_ALLOW_SIGN_UP: "true",
        AUTH_ALLOW_INSECURE_HTTP: "true",
      });
      expect(options.plugins.map((plugin) => plugin.id)).not.toContain("oauth-provider");
      const browser = new Jar();
      const created = await signup();
      expect(created.status).toBe(200);
      browser.store(created);
      const session = await app.request(`${config.baseUrl}/api/auth/get-session`, {
        headers: browser.header(),
      });
      expect(await session.json()).toMatchObject({ user: { email: "deployment@example.com" } });
      const jwt = await app.request(`${config.baseUrl}/api/auth/token`, {
        headers: browser.header(),
      });
      expect(jwt.status).toBe(200);
      expect(await jwt.json()).toMatchObject({ token: expect.any(String) });
      const key = await app.request(`${config.baseUrl}/api/auth/api-key/create`, {
        method: "POST",
        headers: {
          ...browser.header(),
          "content-type": "application/json",
          origin: config.baseUrl,
        },
        body: JSON.stringify({ name: "LAN key" }),
      });
      expect(key.status).toBe(200);
      expect(await key.json()).toMatchObject({ key: expect.stringMatching(/^ashift_/) });
      for (const path of [
        "/.well-known/oauth-authorization-server",
        "/.well-known/openid-configuration",
        "/oauth/sign-in",
        "/oauth/consent",
        "/oauth/assets/oauth.js",
        "/api/auth/oauth2/authorize",
        "/api/auth/oauth2/register",
        "/api/auth/oauth2/token",
      ]) {
        const response = await app.request(`${config.baseUrl}${path}`, {
          method: path.includes("/oauth2/") ? "POST" : "GET",
        });
        expect(response.status, path).toBe(404);
      }
    },
  );

  it.each([
    "https://example.com",
    "http://localhost:4000",
    "http://auth.localhost",
    "http://127.0.0.2:4000",
    "http://[::1]:4000",
  ])("retains OAuth on %s", (origin) => {
    for (const name of ["BETTER_AUTH_URL", "ARIAD_PUBLIC_URL"]) {
      const config = parseConfig({
        ...environment,
        [name]: origin,
        AUTH_ALLOW_INSECURE_HTTP: "true",
      });
      expect(
        buildAuthOptions(config, memoryAdapter({})).plugins.map((plugin) => plugin.id),
      ).toContain("oauth-provider");
    }
  });

  it.each(["BETTER_AUTH_URL", "ARIAD_PUBLIC_URL"])("rejects path-prefixed %s", (name) => {
    expect(() => parseConfig({ ...environment, [name]: "https://example.com/ariadshift" })).toThrow(
      /origin without a path/,
    );
  });

  it.each(["false", "true"])("forwards explicit operator settings through compose: %s", (value) => {
    const root = fileURLToPath(new URL("../../../", import.meta.url));
    const result = JSON.parse(
      execFileSync(
        "docker",
        [
          "compose",
          "--env-file",
          `${root}.env.example`,
          "-f",
          `${root}infra/compose/docker-compose.yml`,
          "--profile",
          "core",
          "config",
          "--format",
          "json",
        ],
        {
          env: {
            PATH: process.env.PATH,
            HOME: process.env.HOME,
            SystemRoot: process.env.SystemRoot,
            AUTH_ALLOW_SIGN_UP: value,
            AUTH_ALLOW_INSECURE_HTTP: value,
          },
          encoding: "utf8",
          timeout: 15_000,
        },
      ),
    );
    expect(result.services.auth.environment.AUTH_ALLOW_SIGN_UP).toBe(value);
    expect(result.services.auth.environment.AUTH_ALLOW_INSECURE_HTTP).toBe(value);
  });
});
