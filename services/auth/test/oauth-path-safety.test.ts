import { describe, expect, it } from "vitest";
import {
  createApp,
  MAX_REGISTRATION_BODY_BYTES,
  PUBLIC_OAUTH_PATHS,
  REGISTRATION_PATH,
  TOKEN_PATH,
} from "../src/app.ts";
import { BASE_PATH } from "../src/auth.ts";
import { SERIALIZED_OAUTH_PATHS } from "../src/oauth-lock.ts";
import { AUTH, createService } from "./oauth-helpers.ts";

function service() {
  const seen: Request[] = [];
  const app = createApp({
    config: { trustedOrigins: ["http://localhost:3000"], trustedClientIpHeader: undefined },
    handler: async (request) => {
      seen.push(request);
      return Response.json({ ok: true });
    },
    metadata: async () => ({}),
    pages: { socialProviders: [], passkeys: false },
    peerAddress: () => "127.0.0.1",
  });
  return { app, seen };
}

describe("OAuth path safety", () => {
  it("keeps serialized paths aligned with the OAuth token and revocation contract", () => {
    expect([...SERIALIZED_OAUTH_PATHS].sort()).toEqual(
      [
        `${BASE_PATH}/oauth2/delete-client`,
        `${BASE_PATH}/oauth2/delete-consent`,
        `${BASE_PATH}/oauth2/revoke`,
        TOKEN_PATH,
      ].sort(),
    );
    expect(PUBLIC_OAUTH_PATHS.has(TOKEN_PATH)).toBe(true);
    expect(PUBLIC_OAUTH_PATHS.has(`${BASE_PATH}/oauth2/revoke`)).toBe(true);
    expect(PUBLIC_OAUTH_PATHS.has(`${BASE_PATH}/oauth2/delete-consent`)).toBe(false);
    expect(PUBLIC_OAUTH_PATHS.has(`${BASE_PATH}/oauth2/delete-client`)).toBe(false);
  });
  it.each(["constructor", "toString", "__proto__", "hasOwnProperty"])(
    "refuses the inherited asset key %s",
    async (name) => {
      expect((await service().app.request(`/oauth/assets/${name}`)).status).toBe(404);
    },
  );

  it.each([...PUBLIC_OAUTH_PATHS])(
    "keeps public CORS for %s with trailing slashes",
    async (path) => {
      for (const suffix of ["", "/", "///"]) {
        const { app, seen } = service();
        const response = await app.request(`${path}${suffix}`, {
          method: "POST",
          headers: {
            origin: "http://localhost:6274",
            "content-type": "application/x-www-form-urlencoded",
            dpop: "unsupported-proof",
          },
          body: "grant_type=refresh_token",
        });
        expect(response.headers.get("access-control-allow-origin")).toBe("*");
        expect(response.headers.get("access-control-allow-credentials")).toBeNull();
        if (path === REGISTRATION_PATH && suffix) {
          expect(response.status).toBe(404);
          expect(seen).toHaveLength(0);
        } else {
          expect(seen).toHaveLength(1);
          expect(seen[0]?.headers.get("dpop")).toBe(
            path === TOKEN_PATH ? null : "unsupported-proof",
          );
        }
      }
    },
  );

  it("refuses registration aliases before the provider and its body-size cap", async () => {
    const { app, db } = createService();
    for (const suffix of ["/", "///"]) {
      const response = await app.request(`${AUTH}${REGISTRATION_PATH}${suffix}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ client_name: "x".repeat(MAX_REGISTRATION_BODY_BYTES + 1) }),
      });
      expect(response.status).toBe(404);
    }
    expect(db.oauthClient).toHaveLength(0);
  });

  it("preserves the real provider's refusal of trailing token aliases", async () => {
    const { app } = createService();
    for (const suffix of ["", "/", "///"]) {
      const response = await app.request(`${AUTH}${TOKEN_PATH}${suffix}`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: "grant_type=unsupported",
      });
      expect(response.status).toBe(suffix ? 404 : 400);
      if (!suffix) {
        expect(await response.json()).toMatchObject({ error: "unsupported_grant_type" });
      }
    }
  });
});
