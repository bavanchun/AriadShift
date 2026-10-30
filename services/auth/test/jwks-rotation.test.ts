import { createPublicKey, randomBytes, verify } from "node:crypto";
import pg from "pg";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { buildAuthOptions, createPool, scheduleLegacySigningKeys } from "../src/auth.ts";
import { migrate } from "../src/migrate.ts";
import { audiences, browserFlow, config, createService, decode, json } from "./oauth-helpers.ts";

const databaseUrl = process.env.ARIAD_TEST_DATABASE_URL?.trim();
if (process.env.ARIAD_TEST_REQUIRE_DATABASE === "1" && !databaseUrl) {
  throw new Error("ARIAD_TEST_REQUIRE_DATABASE=1 needs ARIAD_TEST_DATABASE_URL");
}

describe.skipIf(!databaseUrl)("PostgreSQL signing-key rollover", () => {
  const name = `jwks_test_${randomBytes(8).toString("hex")}`;
  let admin: pg.Pool;
  let pool: pg.Pool;

  beforeAll(async () => {
    admin = new pg.Pool({ connectionString: databaseUrl });
    await admin.query(`CREATE DATABASE "${name}"`);
    const url = new URL(databaseUrl as string);
    url.pathname = `/${name}`;
    pool = createPool({ ...config, databaseUrl: url.toString() });
    await pool.query("CREATE SCHEMA auth");
    await migrate(pool, buildAuthOptions(config, pool));
  }, 30_000);
  afterAll(async () => {
    await pool?.end();
    if (admin) {
      await admin.query(`DROP DATABASE IF EXISTS "${name}"`);
      await admin.end();
    }
  }, 30_000);

  it("schedules previously undated keys once without truncating their publication grace", async () => {
    const { app } = createService(pool);
    const response = await app.request(`${config.baseUrl}/api/auth/jwks`);
    const key = (await response.json()).keys[0];
    await pool.query(
      'UPDATE auth.jwks SET "createdAt"=now()-interval \'40 days\', "expiresAt"=NULL WHERE id=$1',
      [key.kid],
    );
    const before = Date.now();
    await scheduleLegacySigningKeys(pool);
    const read = async () =>
      (
        await pool.query(
          'SELECT "expiresAt", "publicKey", "privateKey" FROM auth.jwks WHERE id=$1',
          [key.kid],
        )
      ).rows[0];
    const scheduled = await read();
    expect(scheduled.expiresAt.getTime()).toBeGreaterThanOrEqual(before - 1);
    expect(scheduled.expiresAt.getTime()).toBeLessThanOrEqual(Date.now());
    await scheduleLegacySigningKeys(pool);
    expect(await read()).toEqual(scheduled);
    const published = await (await app.request(`${config.baseUrl}/api/auth/jwks`)).json();
    expect(published.keys).toContainEqual(key);
  });

  it("rotates real REST and OAuth signatures, retains grace keys and retires their publication", async () => {
    const { app } = createService(pool);
    const flow = await browserFlow(app);
    const rest = async () => {
      const res = await app.request(`${config.baseUrl}/api/auth/token`, {
        headers: flow.jar.header(),
      });
      expect(res.status).toBe(200);
      return (await json(res)).token as string;
    };
    const jwks = async () => {
      const res = await app.request(`${config.baseUrl}/api/auth/jwks`);
      expect(res.status).toBe(200);
      return (await res.json()).keys as Record<string, string>[];
    };
    function verified(token: string, keys: Record<string, string>[], audience: string) {
      const value = decode(token);
      expect(value.header.alg).toBe("EdDSA");
      expect(value.payload.iss).toBe(config.baseUrl);
      expect(audiences(value.payload.aud)).toContain(audience);
      expect(value.payload.exp - value.payload.iat).toBe(900);
      const key = keys.find((entry) => entry.kid === value.header.kid);
      if (!key) return false;
      expect(key.crv).toBe("Ed25519");
      return verify(
        null,
        Buffer.from(value.signed),
        createPublicKey({ key, format: "jwk" }),
        value.signature,
      );
    }
    const oldRest = await rest();
    const oldOAuth = (await flow.issue()).access_token as string;
    const oldKid = decode(oldRest).header.kid;
    expect(decode(oldOAuth).header.kid).toBe(oldKid);
    const stored = (
      await pool.query('SELECT "createdAt", "expiresAt", "privateKey" FROM auth.jwks WHERE id=$1', [
        oldKid,
      ])
    ).rows[0];
    expect(stored.expiresAt).toBeInstanceOf(Date);
    expect((stored.expiresAt.getTime() - stored.createdAt.getTime()) / 1000).toBeCloseTo(
      30 * 24 * 60 * 60,
      0,
    );
    expect(stored.privateKey).not.toContain('"d":');
    const cached = await jwks();
    expect(verified(oldRest, cached, config.baseUrl)).toBe(true);
    expect(verified(oldOAuth, cached, config.mcpResource)).toBe(true);

    // Accelerate only the persisted key's clock. Creation and selection still
    // run through the unchanged production JWT/OAuth plugins and real PG rows.
    await pool.query("UPDATE auth.jwks SET \"expiresAt\"=now()-interval '1 second' WHERE id=$1", [
      oldKid,
    ]);
    const newRest = await rest();
    const newOAuth = (await flow.issue()).access_token as string;
    const newKid = decode(newRest).header.kid;
    expect(newKid).not.toBe(oldKid);
    expect(decode(newOAuth).header.kid).toBe(newKid);
    expect(verified(newRest, cached, config.baseUrl)).toBe(false);
    const fresh = await jwks();
    for (const [value, aud] of [
      [oldRest, config.baseUrl],
      [oldOAuth, config.mcpResource],
      [newRest, config.baseUrl],
      [newOAuth, config.mcpResource],
    ]) {
      expect(verified(value as string, fresh, aud as string)).toBe(true);
    }
    for (const key of fresh) {
      expect(Object.keys(key).sort()).toEqual(["alg", "crv", "kid", "kty", "x"]);
    }
    const replica = createService(pool);
    const replicaKeys = (
      await (await replica.app.request(`${config.baseUrl}/api/auth/jwks`)).json()
    ).keys;
    expect(verified(newOAuth, replicaKeys, config.mcpResource)).toBe(true);
    for (const document of ["oauth-authorization-server", "openid-configuration"]) {
      const discovery = await (
        await app.request(`${config.baseUrl}/.well-known/${document}`)
      ).json();
      expect(discovery.issuer).toBe(config.baseUrl);
      expect(discovery.jwks_uri).toBe(`${config.baseUrl}/api/auth/jwks`);
      expect(JSON.stringify(discovery)).not.toContain("privateKey");
    }
    await pool.query(
      "UPDATE auth.jwks SET \"expiresAt\"=now()-interval '1790 seconds' WHERE id=$1",
      [oldKid],
    );
    expect((await jwks()).some((key) => key.kid === oldKid)).toBe(true);
    await pool.query(
      "UPDATE auth.jwks SET \"expiresAt\"=now()-interval '1801 seconds' WHERE id=$1",
      [oldKid],
    );
    const retired = await jwks();
    expect(retired.some((key) => key.kid === oldKid)).toBe(false);
    expect(verified(newRest, retired, config.baseUrl)).toBe(true);
    expect(verified(newOAuth, retired, config.mcpResource)).toBe(true);
    // Cached public keys still verify until token expiry: retirement is not
    // immediate revocation of already cached signatures.
    expect(verified(oldRest, cached, config.baseUrl)).toBe(true);
    expect(verified(oldOAuth, cached, config.mcpResource)).toBe(true);
  }, 30_000);
});
