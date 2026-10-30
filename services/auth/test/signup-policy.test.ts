import { randomBytes } from "node:crypto";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import pg from "pg";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { createApp } from "../src/app.ts";
import { buildAuthOptions, createAuth, createPool } from "../src/auth.ts";
import { parseConfig } from "../src/config.ts";
import { migrate } from "../src/migrate.ts";
import { pageOptions } from "../src/pages.ts";
import { config as baseConfig, Jar } from "./oauth-helpers.ts";

const databaseUrl = process.env.ARIAD_TEST_DATABASE_URL?.trim();
if (process.env.ARIAD_TEST_REQUIRE_DATABASE === "1" && !databaseUrl) {
  throw new Error("ARIAD_TEST_REQUIRE_DATABASE=1 needs ARIAD_TEST_DATABASE_URL");
}

describe.skipIf(!databaseUrl)("PostgreSQL signup policy", () => {
  const name = `signup_test_${randomBytes(8).toString("hex")}`;
  let admin: pg.Pool;
  let pool: pg.Pool;
  let verifier: Server;
  let endpoint: string;
  let answer: unknown = { success: true, action: "signup", hostname: "localhost" };
  let status = 200;
  let stall = false;
  let verification: Record<string, unknown>[] = [];
  const used = new Set<string>();

  beforeAll(async () => {
    admin = new pg.Pool({ connectionString: databaseUrl });
    await admin.query(`CREATE DATABASE "${name}"`);
    const url = new URL(databaseUrl as string);
    url.pathname = `/${name}`;
    pool = createPool({ ...baseConfig, databaseUrl: url.toString() });
    await pool.query("CREATE SCHEMA auth");
    await migrate(pool, buildAuthOptions(baseConfig, pool));
    verifier = createServer(async (req, res) => {
      let body = "";
      for await (const chunk of req) body += chunk;
      const fields = JSON.parse(body) as Record<string, unknown>;
      verification.push(fields);
      if (stall) return;
      const replay = used.has(String(fields.response));
      used.add(String(fields.response));
      res.writeHead(status, { "content-type": "application/json" });
      res.end(
        JSON.stringify(
          replay ? { success: false, "error-codes": ["timeout-or-duplicate"] } : answer,
        ),
      );
    });
    await new Promise<void>((resolve) => verifier.listen(0, "127.0.0.1", resolve));
    endpoint = `http://127.0.0.1:${(verifier.address() as AddressInfo).port}`;
  }, 30_000);

  afterEach(() => {
    vi.restoreAllMocks();
    answer = { success: true, action: "signup", hostname: "localhost" };
    status = 200;
    stall = false;
    verification = [];
  });

  afterAll(async () => {
    verifier?.closeAllConnections();
    if (verifier) await new Promise<void>((resolve) => verifier.close(() => resolve()));
    await pool?.end();
    if (admin) {
      await admin.query(`DROP DATABASE IF EXISTS "${name}"`);
      await admin.end();
    }
  }, 30_000);

  function service(allowSignUp: boolean, challenged = false) {
    const config = parseConfig({
      DATABASE_URL: databaseUrl,
      BETTER_AUTH_SECRET: baseConfig.secret,
      BETTER_AUTH_URL: baseConfig.baseUrl,
      BETTER_AUTH_TRUSTED_ORIGINS: baseConfig.trustedOrigins.join(","),
      ARIAD_PUBLIC_URL: "http://localhost:8080",
      AUTH_ALLOW_SIGN_UP: String(allowSignUp),
      ...(challenged ? { AUTH_TURNSTILE_SECRET: "test-verifier-secret" } : {}),
    });
    const options = buildAuthOptions(config, pool);
    const auth = createAuth({ ...options, rateLimit: { ...options.rateLimit, enabled: false } });
    const app = createApp({
      config,
      handler: auth.handler,
      pages: pageOptions(config),
      peerAddress: () => "203.0.113.17",
    });
    const originalFetch = globalThis.fetch;
    vi.spyOn(globalThis, "fetch").mockImplementation((input, init) => {
      expect(String(input)).toBe("https://challenges.cloudflare.com/turnstile/v0/siteverify");
      return originalFetch(endpoint, init);
    });
    return { app, auth };
  }

  function signup(
    app: ReturnType<typeof service>["app"],
    challenge?: string,
    path = "/api/auth/sign-up/email",
  ) {
    const email = `signup-${randomBytes(8).toString("hex")}@example.com`;
    const request = new Request(`${baseConfig.baseUrl}${path}`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        origin: baseConfig.baseUrl,
        "x-ariadshift-client-ip": "198.51.100.1",
        "x-forwarded-for": "198.51.100.2",
        ...(challenge === undefined ? {} : { "x-captcha-response": challenge }),
      },
      body: JSON.stringify({ name: "Signup", email, password: "correct horse battery staple" }),
    });
    return { email, request, response: app.request(request) };
  }

  async function counts() {
    const result = await pool.query(
      'SELECT (SELECT count(*)::int FROM auth."user") AS users, (SELECT count(*)::int FROM auth.session) AS sessions, (SELECT count(*)::int FROM auth.apikey) AS keys',
    );
    return result.rows[0];
  }

  async function refused(challenge?: string) {
    const { app } = service(true, true);
    const before = await counts();
    const res = await signup(app, challenge).response;
    expect(res.status).toBeGreaterThanOrEqual(400);
    expect(res.headers.getSetCookie()).toEqual([]);
    expect(await counts()).toEqual(before);
  }

  it("keeps signup closed by default while existing accounts can sign in", async () => {
    const opened = service(true);
    const created = signup(opened.app);
    expect((await created.response).status).toBe(200);
    vi.restoreAllMocks();
    const closed = service(false);
    expect((await signup(closed.app).response).status).toBe(400);
    const login = await closed.app.request(`${baseConfig.baseUrl}/api/auth/sign-in/email`, {
      method: "POST",
      headers: { "content-type": "application/json", origin: baseConfig.baseUrl },
      body: JSON.stringify({ email: created.email, password: "correct horse battery staple" }),
    });
    expect(login.status).toBe(200);
    const jar = new Jar();
    jar.store(login);
    expect(jar.cookies.size).toBeGreaterThan(0);
  });

  it("allows explicit signup without Cloudflare when no secret is configured", async () => {
    const { app } = service(true);
    expect((await signup(app).response).status).toBe(200);
    expect(verification).toEqual([]);
  });

  it.each([undefined, "", "   ", "x".repeat(2049)])(
    "refuses missing, blank or oversized challenge (%s)",
    async (challenge) => {
      await refused(challenge);
      expect(verification).toEqual([]);
    },
  );

  it.each(["invalid-input-response", "timeout-or-duplicate"])(
    "refuses invalid or expired challenge: %s",
    async (code) => {
      answer = { success: false, "error-codes": [code] };
      await refused(randomBytes(12).toString("hex"));
    },
  );

  it.each([
    { success: true, action: "upload", hostname: "localhost" },
    { success: true, action: "signup", hostname: "attacker.example" },
    { success: "true", action: "signup", hostname: "localhost" },
  ])("refuses an unexpected verifier result (%j)", async (result) => {
    answer = result;
    await refused(randomBytes(12).toString("hex"));
  });

  it("accepts a fresh signup challenge using the socket peer and rejects its replay", async () => {
    const { app } = service(true, true);
    const challenge = randomBytes(12).toString("hex");
    expect((await signup(app, challenge).response).status).toBe(200);
    expect(verification).toEqual([
      { secret: "test-verifier-secret", response: challenge, remoteip: "203.0.113.17" },
    ]);
    const before = await counts();
    expect((await signup(app, challenge).response).status).toBe(403);
    expect(await counts()).toEqual(before);
  });

  it("guards direct requests to the auth handler before any account write", async () => {
    const { app, auth } = service(true, true);
    const before = await counts();
    // Build a request without dispatching the application request as well.
    const request = new Request(`${baseConfig.baseUrl}/api/auth/sign-up/email`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        name: "Bypass",
        email: "bypass@example.com",
        password: "correct horse battery staple",
      }),
    });
    expect((await auth.handler(request)).status).toBe(400);
    expect(await counts()).toEqual(before);
    expect((await app.request(`${baseConfig.baseUrl}/api/auth/ok`)).status).toBe(200);
  });

  it("fails closed when verification is unavailable", async () => {
    status = 503;
    await refused(randomBytes(12).toString("hex"));
  });

  it("bounds a stalled verification request", async () => {
    stall = true;
    const started = Date.now();
    await refused(randomBytes(12).toString("hex"));
    expect(Date.now() - started).toBeLessThan(7_000);
  }, 10_000);
});
