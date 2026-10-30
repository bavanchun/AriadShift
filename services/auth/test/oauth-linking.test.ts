// Social sign-in against an existing password account, with GitHub's OAuth
// endpoints stubbed. The address was never verified, so a GitHub login for
// it must not be attached to that account.

import { memoryAdapter } from "better-auth/adapters/memory";
import { getAuthTables } from "better-auth/db";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createApp } from "../src/app.ts";
import { buildAuthOptions, createAuth, discoveryDocuments } from "../src/auth.ts";
import { parseConfig } from "../src/config.ts";
import { pageOptions } from "../src/pages.ts";
import { AUTH, Jar, json, post } from "./oauth-helpers.ts";

afterEach(() => vi.restoreAllMocks());

function serviceWithGitHub(allowSignUp = true) {
  const config = parseConfig({
    DATABASE_URL: "postgres://unused@127.0.0.1:1/none",
    BETTER_AUTH_SECRET: "0123456789abcdef0123456789abcdef",
    BETTER_AUTH_URL: AUTH,
    BETTER_AUTH_TRUSTED_ORIGINS: "http://localhost:3000",
    ARIAD_PUBLIC_URL: "http://localhost:8080",
    ...(allowSignUp ? { AUTH_ALLOW_SIGN_UP: "true" } : {}),
    GITHUB_CLIENT_ID: "github-client",
    GITHUB_CLIENT_SECRET: "github-secret",
  });
  const probe = buildAuthOptions(config, memoryAdapter({}));
  const db: Record<string, unknown[]> = Object.fromEntries(
    Object.values(getAuthTables(probe)).map((table) => [table.modelName, []]),
  );
  const options = buildAuthOptions(config, memoryAdapter(db));
  const auth = createAuth({ ...options, rateLimit: { ...options.rateLimit, enabled: false } });
  const app = createApp({
    config,
    handler: (request) => auth.handler(request),
    metadata: discoveryDocuments(auth),
    pages: pageOptions(config),
    peerAddress: () => "127.0.0.1",
  });
  return { app, db };
}

/** GitHub answers as if the user owns `email` and GitHub verified it. */
function stubGitHub(email: string) {
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (url.startsWith("https://github.com/login/oauth/access_token")) {
      return Response.json({
        access_token: "gho_test",
        token_type: "bearer",
        scope: "read:user,user:email",
      });
    }
    if (url.startsWith("https://api.github.com/user/emails")) {
      return Response.json([{ email, primary: true, verified: true, visibility: "private" }]);
    }
    if (url.startsWith("https://api.github.com/user")) {
      return Response.json({ id: 4242, login: "owner", name: "Owner", email, avatar_url: "" });
    }
    throw new Error(`Unexpected provider request: ${url}`);
  });
}

async function gitHubCallback(app: ReturnType<typeof serviceWithGitHub>["app"]) {
  const browser = new Jar();
  const start = await post(app, "/api/auth/sign-in/social", {
    provider: "github",
    callbackURL: "/",
  });
  expect(start.status).toBe(200);
  browser.store(start);
  const state = new URL((await json(start)).url as string).searchParams.get("state") ?? "";
  return app.request(
    `${AUTH}/api/auth/callback/github?code=test-code&state=${encodeURIComponent(state)}`,
    { headers: browser.header() },
  );
}

const gitHubAccounts = (db: Record<string, unknown[]>) =>
  (db.account ?? []).filter((row) => (row as { providerId?: string }).providerId === "github");

describe("social sign-in and existing accounts", () => {
  it("signs in a new user with GitHub", async () => {
    const { app, db } = serviceWithGitHub();
    stubGitHub("new-user@example.com");
    const callback = await gitHubCallback(app);
    expect(callback.status).toBe(302);
    expect(callback.headers.get("location")).not.toContain("error=");
    expect(gitHubAccounts(db)).toHaveLength(1);
  });

  it("refuses new social accounts without signup opt-in", async () => {
    const { app, db } = serviceWithGitHub(false);
    stubGitHub("new-user@example.com");
    const callback = await gitHubCallback(app);
    expect(callback.status).toBe(302);
    expect(callback.headers.get("location")).toContain("error=");
    expect(gitHubAccounts(db)).toHaveLength(0);
    expect(db.user).toHaveLength(0);
  });

  it("never attaches a GitHub login to an unverified password account with the same email", async () => {
    const { app, db } = serviceWithGitHub();
    const email = "owner@example.com";
    // Someone else registered the address first, with a password nobody verified.
    const squatter = await post(app, "/api/auth/sign-up/email", {
      name: "Squatter",
      email,
      password: "correct horse battery staple",
    });
    expect(squatter.status).toBe(200);
    expect((await json(squatter)).user).toMatchObject({ emailVerified: false });
    stubGitHub(email);
    const callback = await gitHubCallback(app);
    expect(callback.status).toBe(302);
    expect(callback.headers.get("location")).toContain("error=");
    expect(gitHubAccounts(db)).toHaveLength(0);
  });
});
