// Set ARIAD_TEST_DATABASE_URL to a disposable PostgreSQL administrator URL,
// as `just test-auth-db` uses. Required mode rejects absent or blank URLs.
import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import pg from "pg";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { buildAuthOptions, createAuth, createPool } from "../src/auth.ts";
import { migrate } from "../src/migrate.ts";
import {
  OAUTH_TOKEN_LIMITS,
  OAUTH_TOKEN_LOCK_KEY,
  serializedOAuthHandler,
} from "../src/oauth-lock.ts";
import {
  authorizeUrl,
  browserFlow,
  config,
  createService,
  json,
  pkce,
  post,
  REDIRECT_URI,
  token,
} from "./oauth-helpers.ts";

const databaseUrl = process.env.ARIAD_TEST_DATABASE_URL?.trim();
if (process.env.ARIAD_TEST_REQUIRE_DATABASE === "1" && !databaseUrl) {
  throw new Error("ARIAD_TEST_REQUIRE_DATABASE=1 needs ARIAD_TEST_DATABASE_URL");
}
const GATE = "7270514213820104003";

describe.skipIf(!databaseUrl)("PostgreSQL OAuth serialization across replicas", () => {
  const name = `auth_test_${randomBytes(8).toString("hex")}`;
  let backups = "";
  let admin: pg.Pool;
  let first: pg.Pool;
  let second: pg.Pool;
  let observer: pg.Pool;
  let url: URL;

  function backup(label: string) {
    execFileSync("pg_dump", ["--format=custom", "--file", join(backups, `${label}.dump`)], {
      env: {
        ...process.env,
        PGHOST: url.hostname,
        PGPORT: url.port || "5432",
        PGUSER: decodeURIComponent(url.username),
        PGPASSWORD: decodeURIComponent(url.password),
        PGDATABASE: name,
      },
      stdio: "pipe",
    });
  }

  beforeAll(async () => {
    backups = mkdtempSync(join(tmpdir(), "ariad-auth-backup-"));
    admin = new pg.Pool({ connectionString: databaseUrl });
    await admin.query(`CREATE DATABASE "${name}"`);
    url = new URL(databaseUrl as string);
    url.pathname = `/${name}`;
    observer = new pg.Pool({ connectionString: url.toString() });
    backup("before-schema");
    await observer.query("CREATE SCHEMA auth");
    first = createPool({ ...config, databaseUrl: url.toString() });
    second = createPool({ ...config, databaseUrl: url.toString() });
    await migrate(first, buildAuthOptions(config, first));
    // A second migration must be safe on an existing auth schema.
    backup("before-rerun");
    expect((await migrate(second, buildAuthOptions(config, second))).createdTables).toEqual([]);
    await observer.query(`
      CREATE TABLE auth.test_gate (operation text);
      CREATE FUNCTION auth.pause_refresh() RETURNS trigger LANGUAGE plpgsql AS $$
      BEGIN
        IF EXISTS (SELECT 1 FROM auth.test_gate WHERE operation = TG_OP) THEN
          PERFORM pg_advisory_xact_lock(${GATE});
        END IF;
        IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
        RETURN NEW;
      END $$;
      CREATE TRIGGER pause_refresh BEFORE INSERT OR DELETE ON auth."oauthRefreshToken"
      FOR EACH ROW EXECUTE FUNCTION auth.pause_refresh();
    `);
  }, 30_000);

  // Backup and database removal need the same bounded I/O allowance as setup.
  afterAll(async () => {
    if (observer) {
      backup("before-disposal");
      await observer.end();
    }
    await first?.end();
    await second?.end();
    if (admin) {
      await admin.query(`DROP DATABASE IF EXISTS "${name}" WITH (FORCE)`);
      await admin.end();
    }
    if (backups) rmSync(backups, { recursive: true, force: true });
  }, 30_000);

  function advisoryLockParts(key: string) {
    const value = BigInt(key);
    return [(value >> 32n).toString(), (value & 0xffffffffn).toString()];
  }

  async function lockPid(key: string, granted: boolean) {
    const deadline = Date.now() + 5_000;
    while (Date.now() < deadline) {
      const result = await observer.query(
        "SELECT pid FROM pg_locks WHERE locktype='advisory' AND granted=$3 AND classid=$1::oid AND objid=$2::oid AND database=(SELECT oid FROM pg_database WHERE datname=current_database())",
        [...advisoryLockParts(key), granted],
      );
      if (result.rows.length) return result.rows[0].pid as number;
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    throw new Error("expected database lock waiter did not arrive");
  }

  const waiting = (key: string) => lockPid(key, false);

  it("does not hold the token lock while a metadata lookup is slow", async () => {
    let release!: () => void;
    let started!: () => void;
    const lookupStarted = new Promise<void>((resolve) => {
      started = resolve;
    });
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const service = createService(first, false, {
      lookup: async () => [{ address: "93.184.216.34", family: 4 }],
      transport: async () => {
        started();
        await gate;
        return { status: 404, headers: {}, body: "" };
      },
    });
    const id = "https://slow.example/client.json";
    const { challenge } = pkce();
    const authorization = service.app.request(
      authorizeUrl(id, { code_challenge: challenge, code_challenge_method: "S256" }),
    );
    try {
      await lookupStarted;
      const response = await Promise.race([
        token(service.app, {
          grant_type: "authorization_code",
          client_id: id,
          code: "invalid",
          code_verifier: "a".repeat(43),
        }),
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error("token waited for metadata")), 1_000),
        ),
      ]);
      expect(response.status).not.toBe(503);
    } finally {
      release();
      await authorization;
    }
  });

  it("rejects stale CIMD refresh grants without fetching metadata", async () => {
    const id = "https://postgres-cimd.example/client.json";
    let fetches = 0;
    const cimd = {
      lookup: async () => [{ address: "93.184.216.34", family: 4 }],
      transport: async () => {
        fetches++;
        return {
          status: 200,
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            client_id: id,
            redirect_uris: [REDIRECT_URI],
            token_endpoint_auth_method: "none",
            grant_types: ["authorization_code", "refresh_token"],
          }),
        };
      },
    } satisfies import("../src/cimd.ts").CimdFetcherOptions;
    const service = createService(first, false, cimd);
    const flow = await browserFlow(service.app, id);
    const issued = await flow.issue();
    backup("before-cimd-stale");
    await observer.query(
      `UPDATE auth."oauthClient" SET "updatedAt" = now() - interval '25 hours' WHERE "clientId" = $1`,
      [id],
    );
    const priorFetches = fetches;
    const handler = serializedOAuthHandler(
      first,
      service.auth.handler,
      (database) => createAuth(buildAuthOptions(config, database, cimd)).handler,
    );
    const response = await handler(
      new Request(`${config.baseUrl}/api/auth/oauth2/token`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: new URLSearchParams({
          grant_type: "refresh_token",
          client_id: id,
          refresh_token: issued.refresh_token as string,
          resource: config.mcpResource,
        }),
      }),
    );
    expect(response.status).toBe(400);
    expect(await json(response)).toMatchObject({ error: "invalid_grant" });
    expect(fetches).toBe(priorFetches);
  });

  it("returns the identical refresh response to simultaneous retries on two replicas", async () => {
    const one = createService(first);
    const two = createService(second);
    const flow = await browserFlow(one.app);
    const issued = await flow.issue();
    const request = {
      grant_type: "refresh_token",
      client_id: flow.clientId,
      refresh_token: issued.refresh_token as string,
      resource: config.mcpResource,
    };
    const [a, b] = await Promise.all([token(one.app, request), token(two.app, request)]);
    expect(a.status).toBe(200);
    expect(b.status).toBe(200);
    const original = await json(a);
    expect(await json(b)).toMatchObject({ ...original, expires_in: expect.any(Number) });
  });

  it.each(["INSERT", "DELETE"])(
    "prevents a surviving descendant when %s is paused",
    async (operation) => {
      const one = createService(first);
      const two = createService(second);
      const flow = await browserFlow(one.app);
      const issued = await flow.issue();
      const rotated = await json(await flow.refresh(issued.refresh_token as string));
      // Expire the original token's replay window without sleeping or changing
      // crypto/database timers. Other rows remain live and redeemable.
      await observer.query(
        'UPDATE auth."oauthRefreshToken" SET "rotationReplayExpiresAt"=now()-interval \'1 second\' WHERE "clientId"=$1 AND revoked IS NOT NULL',
        [flow.clientId],
      );
      const gate = await observer.connect();
      await gate.query("SELECT pg_advisory_lock($1)", [GATE]);
      await observer.query("INSERT INTO auth.test_gate VALUES ($1)", [operation]);
      const reuse = () =>
        token(two.app, {
          grant_type: "refresh_token",
          client_id: flow.clientId,
          refresh_token: issued.refresh_token as string,
          resource: config.mcpResource,
        });
      let a: Promise<Response> | undefined;
      let b: Promise<Response> | undefined;
      try {
        a = operation === "INSERT" ? flow.refresh(rotated.refresh_token as string) : reuse();
        await waiting(GATE);
        b = operation === "INSERT" ? reuse() : flow.refresh(rotated.refresh_token as string);
        await waiting(OAUTH_TOKEN_LOCK_KEY);
      } finally {
        await observer.query("DELETE FROM auth.test_gate");
        await gate.query("SELECT pg_advisory_unlock($1)", [GATE]);
        gate.release();
      }
      const results = await Promise.all([a as Promise<Response>, b as Promise<Response>]);
      expect((results[operation === "INSERT" ? 1 : 0] as Response).status).toBe(400);
      if (operation === "INSERT") {
        expect(results[0].status).toBe(200);
        const descendant = (await json(results[0])).refresh_token as string;
        expect((await flow.refresh(descendant)).status).toBe(400);
      } else expect(results[1].status).toBe(400);
      const remaining = await observer.query(
        'SELECT count(*)::int AS n FROM auth."oauthRefreshToken" WHERE "clientId"=$1',
        [flow.clientId],
      );
      expect(remaining.rows[0].n).toBe(0);
    },
  );

  it.each(["delete-consent", "delete-client"] as const)(
    "serializes %s with refresh so no working refresh token survives without its consent",
    async (endpoint) => {
      const one = createService(first);
      const two = createService(second);
      const flow = await browserFlow(one.app);
      const issued = await flow.issue();
      const consentRow = await observer.query(
        'SELECT id, "userId" FROM auth."oauthConsent" WHERE "clientId"=$1',
        [flow.clientId],
      );
      const consentId = consentRow.rows[0].id as string;
      const userId = consentRow.rows[0].userId as string;
      await observer.query('UPDATE auth."oauthClient" SET "userId"=$1 WHERE "clientId"=$2', [
        userId,
        flow.clientId,
      ]);

      const gate = await observer.connect();
      await gate.query("SELECT pg_advisory_lock($1)", [GATE]);
      await observer.query("INSERT INTO auth.test_gate VALUES ('INSERT')");
      let refreshing: Promise<Response> | undefined;
      let revoking: Promise<Response> | undefined;
      try {
        refreshing = flow.refresh(issued.refresh_token as string);
        await waiting(GATE);
        revoking = Promise.resolve(
          post(
            two.app,
            `/api/auth/oauth2/${endpoint}`,
            endpoint === "delete-consent" ? { id: consentId } : { client_id: flow.clientId },
            flow.jar.header(),
          ),
        );
        await waiting(OAUTH_TOKEN_LOCK_KEY);
      } finally {
        await observer.query("DELETE FROM auth.test_gate");
        await gate.query("SELECT pg_advisory_unlock($1)", [GATE]);
        gate.release();
      }
      const [refreshRes, revokeRes] = await Promise.all([
        refreshing as Promise<Response>,
        revoking as Promise<Response>,
      ]);
      expect(refreshRes.status).toBe(200);
      expect(revokeRes.status).toBe(200);
      const rotated = (await json(refreshRes)).refresh_token as string;
      expect((await flow.refresh(rotated)).status).toBe(400);
      const consents = await observer.query(
        'SELECT count(*)::int AS n FROM auth."oauthConsent" WHERE "clientId"=$1',
        [flow.clientId],
      );
      expect(consents.rows[0].n).toBe(0);
      const refreshTokens = await observer.query(
        'SELECT count(*)::int AS n FROM auth."oauthRefreshToken" WHERE "clientId"=$1',
        [flow.clientId],
      );
      expect(refreshTokens.rows[0].n).toBe(0);
    },
  );

  it("cannot insert a descendant after the lock backend disconnects", async () => {
    const { app } = createService(first);
    const flow = await browserFlow(app);
    const issued = await flow.issue();
    const gate = await observer.connect();
    await gate.query("SELECT pg_advisory_lock($1)", [GATE]);
    await observer.query("INSERT INTO auth.test_gate VALUES ('INSERT')");
    let pending: Promise<Response> | undefined;
    try {
      pending = flow.refresh(issued.refresh_token as string);
      const inserter = await waiting(GATE);
      const holder = await lockPid(OAUTH_TOKEN_LOCK_KEY, true);
      expect(inserter).toBe(holder);
      await observer.query("SELECT pg_terminate_backend($1)", [holder]);
    } finally {
      await observer.query("DELETE FROM auth.test_gate");
      await gate.query("SELECT pg_advisory_unlock($1)", [GATE]);
      gate.release();
    }
    expect((await (pending as Promise<Response>)).status).toBe(503);
    const live = await observer.query(
      'SELECT count(*)::int AS n FROM auth."oauthRefreshToken" WHERE "clientId"=$1 AND revoked IS NULL',
      [flow.clientId],
    );
    expect(live.rows[0].n).toBe(0);
  });

  async function gone(pid: number) {
    await expect
      .poll(async () => {
        const result = await observer.query("SELECT pid FROM pg_stat_activity WHERE pid=$1", [pid]);
        return result.rowCount;
      })
      .toBe(0);
    const locks = await observer.query("SELECT pid FROM pg_locks WHERE pid=$1", [pid]);
    expect(locks.rowCount).toBe(0);
  }

  async function freeTokenLock() {
    const check = await observer.connect();
    try {
      expect(
        (await check.query("SELECT pg_try_advisory_lock($1) AS free", [OAUTH_TOKEN_LOCK_KEY]))
          .rows[0].free,
      ).toBe(true);
    } finally {
      await check.query("SELECT pg_advisory_unlock($1)", [OAUTH_TOKEN_LOCK_KEY]);
      check.release();
    }
  }

  it("cancels a stalled provider, destroys its lock backend and restores pool capacity", async () => {
    const { app } = createService(first);
    const flow = await browserFlow(app);
    const issued = await flow.issue();
    const gate = await observer.connect();
    const gatePid = (await gate.query("SELECT pg_backend_pid() AS pid")).rows[0].pid;
    await gate.query("BEGIN");
    // Stall the revocation UPDATE before it consumes the original refresh token.
    await gate.query('SELECT id FROM auth."oauthRefreshToken" WHERE "clientId"=$1 FOR UPDATE', [
      flow.clientId,
    ]);
    const started = Date.now();
    try {
      const pending = flow.refresh(issued.refresh_token as string);
      const pid = await lockPid(OAUTH_TOKEN_LOCK_KEY, true);
      await expect
        .poll(async () => {
          const result = await observer.query("SELECT $1=ANY(pg_blocking_pids($2)) AS blocked", [
            gatePid,
            pid,
          ]);
          return result.rows[0].blocked;
        })
        .toBe(true);
      const response = await pending;
      expect(response.status).toBe(503);
      expect(response.headers.get("retry-after")).toBe("1");
      expect(await json(response)).toEqual({
        error: "temporarily_unavailable",
        error_description: "Please retry later.",
      });
      expect(Date.now() - started).toBeLessThan(OAUTH_TOKEN_LIMITS.lockMs + 2_000);
      await gone(pid);
      await freeTokenLock();
    } finally {
      await gate.query("ROLLBACK");
      gate.release();
    }
    expect((await flow.refresh(issued.refresh_token as string)).status).toBe(200);
    const clients = await Promise.all(Array.from({ length: 10 }, () => first.connect()));
    for (const client of clients) client.release();
    expect(first.waitingCount).toBe(0);
  }, 15_000);

  it("caps lock waiters before checkout while ordinary database work stays available", async () => {
    const { app } = createService(first);
    const flow = await browserFlow(app);
    const issued = await flow.issue();
    const gate = await observer.connect();
    await gate.query("SELECT pg_advisory_lock($1)", [OAUTH_TOKEN_LOCK_KEY]);
    const pending: Promise<Response>[] = [];
    try {
      for (let i = 0; i < OAUTH_TOKEN_LIMITS.capacity; i++)
        pending.push(flow.refresh(issued.refresh_token as string));
      await expect
        .poll(async () => {
          const locks = await observer.query(
            "SELECT count(*)::int AS n FROM pg_locks WHERE locktype='advisory' AND NOT granted AND classid=$1::oid AND objid=$2::oid",
            advisoryLockParts(OAUTH_TOKEN_LOCK_KEY),
          );
          return locks.rows[0].n;
        })
        .toBe(OAUTH_TOKEN_LIMITS.capacity);
      const total = first.totalCount;
      const started = Date.now();
      const excess = await flow.refresh(issued.refresh_token as string);
      expect(excess.status).toBe(503);
      expect(excess.headers.get("retry-after")).toBe("1");
      expect(Date.now() - started).toBeLessThan(500);
      expect(first.totalCount).toBe(total);
      expect(first.waitingCount).toBe(0);
      expect((await first.query("SELECT 1 AS available")).rows[0].available).toBe(1);
      expect(
        (
          await app.request(`${config.baseUrl}/api/auth/get-session`, {
            headers: flow.jar.header(),
          })
        ).status,
      ).toBe(200);
    } finally {
      await gate.query("SELECT pg_advisory_unlock($1)", [OAUTH_TOKEN_LOCK_KEY]);
      gate.release();
      expect((await Promise.all(pending)).every((response) => response.status === 200)).toBe(true);
    }
    expect((await flow.refresh(issued.refresh_token as string)).status).toBe(200);
  }, 15_000);

  it("bounds advisory lock acquisition and later refreshes recover", async () => {
    const { app } = createService(first);
    const flow = await browserFlow(app);
    const issued = await flow.issue();
    const gate = await observer.connect();
    await gate.query("SELECT pg_advisory_lock($1)", [OAUTH_TOKEN_LOCK_KEY]);
    try {
      const started = Date.now();
      const pending = flow.refresh(issued.refresh_token as string);
      const pid = await waiting(OAUTH_TOKEN_LOCK_KEY);
      expect((await pending).status).toBe(503);
      expect(Date.now() - started).toBeLessThan(OAUTH_TOKEN_LIMITS.acquisitionMs + 2_000);
      await gone(pid);
    } finally {
      await gate.query("SELECT pg_advisory_unlock($1)", [OAUTH_TOKEN_LOCK_KEY]);
      gate.release();
    }
    await freeTokenLock();
    expect((await flow.refresh(issued.refresh_token as string)).status).toBe(200);
  }, 15_000);

  it("bounds checkout without leaking a late pool acquisition", async () => {
    const { app, auth } = createService(first);
    // Finish provider startup before deliberately saturating its shared pool.
    await auth.$context;
    const held = await Promise.all(Array.from({ length: 10 }, () => first.connect()));
    try {
      const started = Date.now();
      expect((await token(app, { grant_type: "refresh_token" })).status).toBe(503);
      expect(Date.now() - started).toBeLessThan(OAUTH_TOKEN_LIMITS.checkoutMs + 1_000);
      expect(first.waitingCount).toBe(0);
    } finally {
      for (const client of held) client.release();
    }
    expect((await first.query("SELECT 1 AS available")).rows[0].available).toBe(1);
  }, 10_000);

  it("enforces an overall deadline and prevents abandoned provider writes", async () => {
    let pid = 0;
    let continueProvider: () => void = () => {};
    const ready = new Promise<void>((resolve) => {
      continueProvider = resolve;
    });
    let settled: Promise<unknown> | undefined;
    const handler = serializedOAuthHandler(
      first,
      async () => new Response(),
      (database) => async () => {
        const connection = await database.connect();
        pid = (await connection.query("SELECT pg_backend_pid() AS pid")).rows[0].pid;
        await ready;
        settled = connection.query("SELECT 1");
        await settled;
        return new Response();
      },
      { ...OAUTH_TOKEN_LIMITS, requestMs: 300 },
    );
    try {
      const response = await handler(
        new Request(`${config.baseUrl}/api/auth/oauth2/token`, {
          method: "POST",
          body: "grant_type=refresh_token",
        }),
      );
      expect(response.status).toBe(503);
      expect(pid).toBeGreaterThan(0);
      await gone(pid);
      await freeTokenLock();
    } finally {
      continueProvider();
    }
    await expect.poll(() => settled !== undefined).toBe(true);
    await expect(settled).rejects.toThrow();
    expect((await first.query("SELECT 1 AS available")).rows[0].available).toBe(1);
  });

  it("cancels an executing statement at the overall request deadline", async () => {
    let pid = 0;
    const handler = serializedOAuthHandler(
      first,
      async () => new Response(),
      (database) => async () => {
        const connection = await database.connect();
        pid = (await connection.query("SELECT pg_backend_pid() AS pid")).rows[0].pid;
        await connection.query("SELECT pg_sleep(5)");
        return new Response();
      },
      { ...OAUTH_TOKEN_LIMITS, requestMs: 300 },
    );
    const started = Date.now();
    expect(
      (
        await handler(
          new Request(`${config.baseUrl}/api/auth/oauth2/revoke`, {
            method: "POST",
            body: "token=synthetic",
          }),
        )
      ).status,
    ).toBe(503);
    expect(Date.now() - started).toBeLessThan(1_500);
    expect(pid).toBeGreaterThan(0);
    await gone(pid);
    await freeTokenLock();
  });

  it("accepts a legitimate token request close to the 64 KiB body limit", async () => {
    const { app } = createService(first);
    const flow = await browserFlow(app);
    const { fields } = await flow.authorization();
    const body = new URLSearchParams({ ...fields, padding: "x".repeat(63 * 1024) }).toString();
    expect(Buffer.byteLength(body)).toBeLessThan(64 * 1024);
    expect(Buffer.byteLength(body)).toBeGreaterThan(63 * 1024);
    const response = await app.request(`${config.baseUrl}/api/auth/oauth2/token`, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      body,
    });
    expect(response.status).toBe(200);
    expect((await flow.refresh((await json(response)).refresh_token as string)).status).toBe(200);
  });

  it("does not hold the token lock while a request body is incomplete", async () => {
    const { app } = createService(first);
    const flow = await browserFlow(app);
    const issued = await flow.issue();
    const payload = new TextEncoder().encode(
      new URLSearchParams({
        grant_type: "refresh_token",
        client_id: flow.clientId,
        refresh_token: issued.refresh_token as string,
        resource: config.mcpResource,
      }).toString(),
    );
    let readStarted: () => void = () => {};
    let finishBody: () => void = () => {};
    const reading = new Promise<void>((resolve) => {
      readStarted = resolve;
    });
    const ready = new Promise<void>((resolve) => {
      finishBody = resolve;
    });
    // No prefetch: signal only when the handler actually starts reading.
    const stream = new ReadableStream<Uint8Array>(
      {
        async pull(controller) {
          readStarted();
          await ready;
          controller.enqueue(payload);
          controller.close();
        },
      },
      { highWaterMark: 0 },
    );
    const slow = Promise.resolve(
      app.request(
        new Request(`${config.baseUrl}/api/auth/oauth2/token`, {
          method: "POST",
          headers: {
            "content-type": "application/x-www-form-urlencoded",
            "content-length": String(payload.byteLength),
          },
          body: stream,
          duplex: "half",
        } as RequestInit),
      ),
    );
    await reading;
    const quick = flow.refresh(issued.refresh_token as string);
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      const deadline = new Promise<never>((_resolve, reject) => {
        timer = setTimeout(
          () => reject(new Error("incomplete body blocked another token request")),
          3_000,
        );
      });
      expect((await Promise.race([quick, deadline])).status).toBe(200);
    } finally {
      if (timer) clearTimeout(timer);
      finishBody();
      await Promise.all([slow, quick]);
    }
    expect((await slow).status).toBe(200);
  }, 15_000);
});
