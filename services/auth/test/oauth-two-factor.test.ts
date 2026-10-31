// Two-factor accounts and privileged client metadata during OAuth, in process
// over Better Auth's memory adapter. A password alone must never yield an
// authorization code for an account with two-factor on, and an anonymous
// registration must not be able to skip the consent screen.

import { createHmac, randomBytes } from "node:crypto";
import { describe, expect, it } from "vitest";
import {
  type App,
  AUTH,
  authorizeUrl,
  createService,
  Jar,
  json,
  pkce,
  post,
  REDIRECT_URI,
  signedQuery,
} from "./oauth-helpers.ts";

const PASSWORD = "correct horse battery staple";

function base32Decode(value: string): Buffer {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = 0;
  let accumulator = 0;
  const out: number[] = [];
  for (const char of value.replace(/=+$/, "").toUpperCase()) {
    const index = alphabet.indexOf(char);
    if (index < 0) throw new Error(`not base32: ${char}`);
    accumulator = (accumulator << 5) | index;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.push((accumulator >>> bits) & 0xff);
    }
  }
  return Buffer.from(out);
}

/** RFC 6238 TOTP: SHA-1, 6 digits, 30-second steps, as authenticator apps compute it. */
function totp(secret: string, now = Date.now()): string {
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(Math.floor(now / 30_000)));
  const mac = createHmac("sha1", base32Decode(secret)).update(counter).digest();
  const offset = (mac.at(-1) as number) & 0x0f;
  return ((mac.readUInt32BE(offset) & 0x7fffffff) % 1_000_000).toString().padStart(6, "0");
}

async function signUp(app: App) {
  const email = `2fa-${randomBytes(6).toString("hex")}@example.com`;
  const jar = new Jar();
  const res = await post(app, "/api/auth/sign-up/email", {
    name: "Two factor",
    email,
    password: PASSWORD,
  });
  expect(res.status).toBe(200);
  jar.store(res);
  return { email, jar };
}

/** An account with TOTP two-factor turned on, and its TOTP secret. */
async function twoFactorAccount(app: App) {
  const { email, jar } = await signUp(app);
  const enable = await post(
    app,
    "/api/auth/two-factor/enable",
    { password: PASSWORD },
    jar.header(),
  );
  expect(enable.status).toBe(200);
  jar.store(enable);
  const secret = new URL((await json(enable)).totpURI as string).searchParams.get("secret") ?? "";
  const verify = await post(
    app,
    "/api/auth/two-factor/verify-totp",
    { code: totp(secret) },
    jar.header(),
  );
  expect(verify.status).toBe(200);
  return { email, secret };
}

async function registerClient(app: App, extra: Record<string, unknown> = {}) {
  return post(app, "/api/auth/oauth2/register", {
    client_name: "Two-factor test",
    redirect_uris: [REDIRECT_URI],
    token_endpoint_auth_method: "none",
    grant_types: ["authorization_code", "refresh_token"],
    response_types: ["code"],
    ...extra,
  });
}

/** The signed query of the sign-in page an unauthenticated authorization lands on. */
async function signInQuery(app: App, clientId: string) {
  const res = await app.request(
    authorizeUrl(clientId, { code_challenge: pkce().challenge, code_challenge_method: "S256" }),
  );
  expect(res.status).toBe(302);
  const location = res.headers.get("location") ?? "";
  expect(location).toContain(`${AUTH}/oauth/sign-in?`);
  return signedQuery(location);
}

describe("OAuth and two-factor accounts", () => {
  it("issues no code and keeps no session after a password-only sign-in", async () => {
    const { app } = createService();
    const { email } = await twoFactorAccount(app);
    const clientId = (await json(await registerClient(app))).client_id as string;
    const oauthQuery = await signInQuery(app, clientId);

    const browser = new Jar();
    const signIn = await post(app, "/api/auth/sign-in/email", {
      email,
      password: PASSWORD,
      oauth_query: oauthQuery,
    });
    expect(signIn.status).toBe(200);
    browser.store(signIn);
    const body = await json(signIn);
    expect(body.twoFactorRedirect).toBe(true);
    expect(body.redirect).not.toBe(true);
    expect(JSON.stringify(body)).not.toContain("code=");
    expect(signIn.headers.get("location")).toBeNull();

    const session = await app.request(`${AUTH}/api/auth/get-session`, {
      headers: browser.header(),
    });
    expect(await session.json()).toBeNull();
    const again = await app.request(
      authorizeUrl(clientId, { code_challenge: pkce().challenge, code_challenge_method: "S256" }),
      { headers: browser.header() },
    );
    expect(again.status).toBe(302);
    const location = new URL(again.headers.get("location") ?? "");
    expect(`${location.origin}${location.pathname}`).toBe(`${AUTH}/oauth/sign-in`);
    expect(location.searchParams.has("code")).toBe(false);
  });

  it("continues the authorization once the second factor is verified", async () => {
    const { app } = createService();
    const { email, secret } = await twoFactorAccount(app);
    const clientId = (await json(await registerClient(app))).client_id as string;
    const oauthQuery = await signInQuery(app, clientId);
    const browser = new Jar();
    const signIn = await post(app, "/api/auth/sign-in/email", {
      email,
      password: PASSWORD,
      oauth_query: oauthQuery,
    });
    browser.store(signIn);
    const second = await post(
      app,
      "/api/auth/two-factor/verify-totp",
      { code: totp(secret, Date.now() + 30_000), oauth_query: oauthQuery },
      browser.header(),
    );
    expect(second.status).toBe(200);
    browser.store(second);
    const continued = await json(second);
    expect(continued.redirect).toBe(true);
    const consent = await post(
      app,
      "/api/auth/oauth2/consent",
      { accept: true, oauth_query: signedQuery(continued.url as string) },
      browser.header(),
    );
    expect(consent.status).toBe(200);
    const callback = new URL((await json(consent)).url as string);
    expect(`${callback.origin}${callback.pathname}`).toBe(REDIRECT_URI);
    expect(callback.searchParams.get("code")).toBeTruthy();
  });
});

describe("OAuth registration without an account", () => {
  it.each([
    ["skip_consent", true],
    ["skipConsent", true],
    ["require_pkce", false],
    ["requirePKCE", false],
    ["enable_end_session", true],
    ["enableEndSession", true],
    ["user_id", "attacker"],
    ["userId", "attacker"],
    ["reference_id", "attacker"],
    ["referenceId", "attacker"],
  ])("cannot set privileged client metadata %s without an account", async (field, value) => {
    const { app, db } = createService();
    const { email } = await signUp(app);
    const victim = new Jar();
    const signIn = await post(app, "/api/auth/sign-in/email", { email, password: PASSWORD });
    expect(signIn.status).toBe(200);
    victim.store(signIn);

    const register = await registerClient(app, { [field as string]: value });
    // Refusing the privileged field outright is also safe.
    if (register.status >= 400) {
      expect([400, 401, 403]).toContain(register.status);
      expect(db.oauthClient).toHaveLength(0);
      return;
    }
    const client = await json(register);
    expect(client.skip_consent).not.toBe(true);
    const stored = db.oauthClient?.[0] as Record<string, unknown>;
    expect(stored.skipConsent).not.toBe(true);
    expect(stored.requirePKCE).not.toBe(false);
    expect(stored.enableEndSession).not.toBe(true);
    expect(stored.userId).not.toBe("attacker");
    expect(stored.referenceId).not.toBe("attacker");
    const res = await app.request(
      authorizeUrl(client.client_id as string, {
        code_challenge: pkce().challenge,
        code_challenge_method: "S256",
      }),
      { headers: victim.header() },
    );
    expect(res.status).toBe(302);
    const location = new URL(res.headers.get("location") ?? "");
    expect(`${location.origin}${location.pathname}`).toBe(`${AUTH}/oauth/consent`);
    expect(location.searchParams.has("code")).toBe(false);
  });
});
