import { describe, expect, it, vi } from "vitest";
import {
  clientAddress,
  createApp,
  type DiscoveryDocument,
  MAX_BODY_BYTES,
  MAX_REGISTRATION_BODY_BYTES,
  PUBLIC_OAUTH_PATHS,
  REGISTRATION_PATH,
  withoutDpop,
} from "../src/app.ts";
import { CLIENT_IP_HEADER } from "../src/auth.ts";
import { type PageOptions, pageOptions } from "../src/pages.ts";
import { AUTH, createService, REDIRECT_URI } from "./oauth-helpers.ts";

const WEB = "http://localhost:3000";
const INSPECTOR = "http://localhost:6274";

/** An app whose Better Auth handler records what it receives. */
function setup(
  trustedClientIpHeader?: string,
  peer = "203.0.113.7",
  pages: PageOptions = { socialProviders: [], passkeys: false },
) {
  const seen: Request[] = [];
  const documents: DiscoveryDocument[] = [];
  const app = createApp({
    config: { trustedOrigins: [WEB], trustedClientIpHeader },
    handler: async (request) => {
      seen.push(request);
      return Response.json({ ok: true });
    },
    metadata: async (_request, document) => {
      documents.push(document);
      return {
        issuer: "http://localhost:3001",
        code_challenge_methods_supported: ["S256"],
        dpop_signing_alg_values_supported: ["EdDSA"],
      };
    },
    pages,
    peerAddress: () => peer,
  });
  return { app, seen, documents };
}

describe("createApp", () => {
  it("answers a CORS preflight from the web app with credentials", async () => {
    const { app } = setup();
    const res = await app.request("/api/auth/sign-in/email", {
      method: "OPTIONS",
      headers: { Origin: WEB, "Access-Control-Request-Method": "POST" },
    });
    expect(res.status).toBe(204);
    expect(res.headers.get("access-control-allow-origin")).toBe(WEB);
    expect(res.headers.get("access-control-allow-credentials")).toBe("true");
    expect(res.headers.get("access-control-allow-methods")).toContain("POST");
  });

  it("does not grant CORS to other origins", async () => {
    const { app } = setup();
    const res = await app.request("/api/auth/get-session", {
      headers: { Origin: "https://evil.example" },
    });
    expect(res.headers.get("access-control-allow-origin")).toBeNull();
  });

  it("exposes the JWT header to the web app", async () => {
    const { app } = setup();
    const res = await app.request("/api/auth/get-session", { headers: { Origin: WEB } });
    expect(res.status).toBe(200);
    expect(res.headers.get("access-control-allow-origin")).toBe(WEB);
    expect(res.headers.get("access-control-expose-headers")).toContain("set-auth-jwt");
  });

  it("passes requests to Better Auth with the socket peer as the client address", async () => {
    const { app, seen } = setup();
    await app.request("/api/auth/sign-in/email", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        [CLIENT_IP_HEADER]: "198.51.100.1",
        "X-Forwarded-For": "198.51.100.2",
      },
      body: JSON.stringify({ email: "a@example.com", password: "pw" }),
    });
    expect(seen).toHaveLength(1);
    const request = seen[0] as Request;
    expect(request.headers.get(CLIENT_IP_HEADER)).toBe("203.0.113.7");
    expect(await request.json()).toEqual({ email: "a@example.com", password: "pw" });
  });

  it("uses a trusted proxy header when configured", async () => {
    const { app, seen } = setup("cf-connecting-ip", "10.0.0.2");
    await app.request("/api/auth/get-session", {
      headers: { "CF-Connecting-IP": "198.51.100.9" },
    });
    expect((seen[0] as Request).headers.get(CLIENT_IP_HEADER)).toBe("198.51.100.9");
  });

  it("rejects bodies over the limit before Better Auth sees them", async () => {
    const { app, seen } = setup();
    const res = await app.request("/api/auth/sign-up/email", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "x".repeat(MAX_BODY_BYTES + 1),
    });
    expect(res.status).toBe(413);
    expect(seen).toHaveLength(0);
  });

  it("caps client registrations far below the general body limit", async () => {
    const { app, seen } = setup();
    expect(REGISTRATION_PATH).toBe("/api/auth/oauth2/register");
    expect(MAX_REGISTRATION_BODY_BYTES).toBeLessThan(MAX_BODY_BYTES);
    const metadata = (nameLength: number) =>
      JSON.stringify({
        redirect_uris: ["http://127.0.0.1:33418/callback"],
        client_name: "x".repeat(nameLength),
      });

    const oversized = metadata(MAX_REGISTRATION_BODY_BYTES);
    expect(oversized.length).toBeGreaterThan(MAX_REGISTRATION_BODY_BYTES);
    const rejected = await app.request(REGISTRATION_PATH, {
      method: "POST",
      headers: { "Content-Type": "application/json", Origin: INSPECTOR },
      body: oversized,
    });
    expect(rejected.status).toBe(413);
    expect(await rejected.json()).toMatchObject({ error: "invalid_client_metadata" });
    expect(seen).toHaveLength(0);

    const accepted = await app.request(REGISTRATION_PATH, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: metadata(256),
    });
    expect(accepted.status).toBe(200);
    expect(seen).toHaveLength(1);

    // Other endpoints keep the general limit.
    const signUp = await app.request("/api/auth/sign-up/email", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: "x".repeat(MAX_REGISTRATION_BODY_BYTES * 2) }),
    });
    expect(signUp.status).toBe(200);
    expect(seen).toHaveLength(2);
  });

  it.each(["/", "//"])(
    "rejects registration with trailing slashes %s before forwarding",
    async (suffix) => {
      const { app, seen } = setup();
      for (const size of [256, MAX_REGISTRATION_BODY_BYTES + 1, MAX_BODY_BYTES + 1]) {
        const response = await app.request(`${REGISTRATION_PATH}${suffix}`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ client_name: "x".repeat(size) }),
        });
        expect(response.status).toBe(404);
      }
      expect(seen).toHaveLength(0);
    },
  );

  it("serves nothing else outside /api/auth", async () => {
    const { app, seen } = setup();
    expect((await app.request("/")).status).toBe(404);
    expect((await app.request("/api/other")).status).toBe(404);
    expect((await app.request("/.well-known/oauth-protected-resource")).status).toBe(404);
    expect((await app.request("/oauth/assets/missing.js")).status).toBe(404);
    expect(seen).toHaveLength(0);
  });
});

describe("registration body bytes", () => {
  function metadata(size: number): string {
    const fields = {
      redirect_uris: [REDIRECT_URI],
      token_endpoint_auth_method: "none",
      client_name: "",
    };
    fields.client_name = "x".repeat(size - JSON.stringify(fields).length);
    const body = JSON.stringify(fields);
    expect(new TextEncoder().encode(body)).toHaveLength(size);
    return body;
  }

  it.each([
    { size: 9106, declared: 10, status: 413 },
    { size: 200, declared: 200, status: 201 },
    { size: 8192, declared: undefined, status: 201 },
    { size: 8193, declared: undefined, status: 413 },
    { size: 8192, declared: 10, status: 201 },
    { size: 8193, declared: 10, status: 413 },
  ])(
    "checks $size actual bytes with declared length $declared",
    async ({ size, declared, status }) => {
      const { app, db } = createService();
      const headers = new Headers({ "Content-Type": "application/json" });
      if (declared !== undefined) headers.set("Content-Length", String(declared));
      const response = await app.request(
        new Request(`${AUTH}${REGISTRATION_PATH}`, {
          method: "POST",
          headers,
          body: metadata(size),
        }),
      );
      expect(response.status).toBe(status);
      expect(db.oauthClient).toHaveLength(status === 201 ? 1 : 0);
      if (status === 413) {
        expect(await response.json()).toMatchObject({ error: "invalid_client_metadata" });
      }
    },
  );

  it.each([false, true])(
    "rejects an oversized streamed body with chunked header %s",
    async (chunked) => {
      const { app, db } = createService();
      const bytes = new TextEncoder().encode(metadata(8193));
      const headers = new Headers({ "Content-Type": "application/json" });
      if (chunked) headers.set("Transfer-Encoding", "chunked");
      const response = await app.request(
        new Request(`${AUTH}${REGISTRATION_PATH}`, {
          method: "POST",
          headers,
          body: new ReadableStream({
            start(controller) {
              controller.enqueue(bytes.subarray(0, 4096));
              controller.enqueue(bytes.subarray(4096));
              controller.close();
            },
          }),
          duplex: "half",
        } as RequestInit),
      );
      expect(response.status).toBe(413);
      expect(await response.json()).toMatchObject({ error: "invalid_client_metadata" });
      expect(db.oauthClient).toHaveLength(0);
    },
  );

  it("counts UTF-8 bytes rather than characters despite an understated length", async () => {
    const { app, db } = createService();
    const body = JSON.stringify({ redirect_uris: [REDIRECT_URI], client_name: "é".repeat(4096) });
    expect(body.length).toBeLessThan(MAX_REGISTRATION_BODY_BYTES);
    expect(new TextEncoder().encode(body).length).toBeGreaterThan(MAX_REGISTRATION_BODY_BYTES);
    const response = await app.request(
      new Request(`${AUTH}${REGISTRATION_PATH}`, {
        method: "POST",
        headers: { "Content-Type": "application/json", "Content-Length": "10" },
        body,
      }),
    );
    expect(response.status).toBe(413);
    expect(db.oauthClient).toHaveLength(0);
  });

  it("drops an oversized single chunk and cancels before calling the provider", async () => {
    const { app, db, auth } = createService();
    const handler = vi.spyOn(auth, "handler");
    let reads = 0;
    let cancelled = false;
    const response = await app.request(
      new Request(`${AUTH}${REGISTRATION_PATH}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: new ReadableStream(
          {
            pull(controller) {
              reads++;
              if (reads > 1) throw new Error("read beyond the oversized chunk");
              controller.enqueue(new Uint8Array(1024 * 1024));
            },
            cancel() {
              cancelled = true;
            },
          },
          { highWaterMark: 0 },
        ),
        duplex: "half",
      } as RequestInit),
    );
    expect(response.status).toBe(413);
    expect(await response.json()).toMatchObject({ error: "invalid_client_metadata" });
    expect(reads).toBe(1);
    expect(cancelled).toBe(true);
    expect(handler).not.toHaveBeenCalled();
    expect(db.oauthClient).toHaveLength(0);
    handler.mockRestore();
  });

  it("handles an errored upload without calling the provider or creating a client", async () => {
    const { app, db, auth } = createService();
    const handler = vi.spyOn(auth, "handler");
    const response = await app.request(
      new Request(`${AUTH}${REGISTRATION_PATH}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: new ReadableStream({
          start(controller) {
            controller.error(new Error("registration upload interrupted"));
          },
        }),
        duplex: "half",
      } as RequestInit),
    );
    expect(response.status).toBe(500);
    expect(handler).not.toHaveBeenCalled();
    expect(db.oauthClient).toHaveLength(0);
    handler.mockRestore();
  });

  it("forwards only the read bytes with corrected framing headers", async () => {
    const { app, seen } = setup();
    const body = metadata(200);
    const response = await app.request(REGISTRATION_PATH, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Content-Length": "10",
        "Transfer-Encoding": "chunked",
      },
      body,
    });
    expect(response.status).toBe(200);
    expect(seen).toHaveLength(1);
    expect(seen[0]?.headers.get("content-length")).toBe("200");
    expect(seen[0]?.headers.has("transfer-encoding")).toBe(false);
    expect(await seen[0]?.text()).toBe(body);
  });

  it.each([true, false])(
    "cancels rejected bodies, with early declared rejection %s",
    async (declared) => {
      const { app, seen } = setup();
      let reads = 0;
      let cancelled = false;
      const headers = new Headers({ "Content-Type": "application/json" });
      if (declared) headers.set("Content-Length", "8193");
      const response = await app.request(
        new Request(`${AUTH}${REGISTRATION_PATH}`, {
          method: "POST",
          headers,
          body: new ReadableStream(
            {
              pull(controller) {
                reads++;
                if (reads > 3) throw new Error("read past the registration limit");
                controller.enqueue(new Uint8Array(reads === 3 ? 1 : 4096));
              },
              cancel() {
                cancelled = true;
              },
            },
            { highWaterMark: 0 },
          ),
          duplex: "half",
        } as RequestInit),
      );
      expect(response.status).toBe(413);
      expect(reads).toBe(declared ? 0 : 3);
      expect(cancelled).toBe(true);
      expect(seen).toHaveLength(0);
    },
  );
});

describe("OAuth endpoints", () => {
  it("serves RFC 8414 and OpenID discovery at the issuer root, without DPoP", async () => {
    const { app, seen, documents } = setup();
    for (const document of ["oauth-authorization-server", "openid-configuration"] as const) {
      const res = await app.request(`/.well-known/${document}`, {
        headers: { Origin: INSPECTOR },
      });
      expect(res.status).toBe(200);
      expect(res.headers.get("access-control-allow-origin")).toBe("*");
      expect(res.headers.get("cache-control")).toContain("max-age=15");
      const body = (await res.json()) as Record<string, unknown>;
      expect(body.issuer).toBe("http://localhost:3001");
      expect(body.code_challenge_methods_supported).toEqual(["S256"]);
      expect(body).not.toHaveProperty("dpop_signing_alg_values_supported");
    }
    expect(documents).toEqual(["oauth-authorization-server", "openid-configuration"]);
    expect(seen).toHaveLength(0);
  });

  it("lets browser OAuth clients call the public endpoints from any origin, without cookies", async () => {
    const { app } = setup();
    for (const path of PUBLIC_OAUTH_PATHS) {
      const preflight = await app.request(path, {
        method: "OPTIONS",
        headers: { Origin: INSPECTOR, "Access-Control-Request-Method": "POST" },
      });
      expect(preflight.status, path).toBe(204);
      expect(preflight.headers.get("access-control-allow-origin"), path).toBe("*");
      expect(preflight.headers.get("access-control-allow-credentials"), path).toBeNull();
    }
  });

  it("keeps session endpoints, authorize and consent limited to trusted origins", async () => {
    const { app } = setup();
    for (const path of [
      "/api/auth/get-session",
      "/api/auth/oauth2/authorize",
      "/api/auth/oauth2/consent",
      "/api/auth/api-key/create",
    ]) {
      const res = await app.request(path, { headers: { Origin: INSPECTOR } });
      expect(res.headers.get("access-control-allow-origin"), path).toBeNull();
    }
  });

  it("drops DPoP proofs at the token endpoint so tokens stay Bearer tokens", async () => {
    const { app, seen } = setup();
    await app.request("/api/auth/oauth2/token", {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded", DPoP: "proof" },
      body: "grant_type=authorization_code&code=c",
    });
    await app.request("/api/auth/sign-in/email", {
      method: "POST",
      headers: { "Content-Type": "application/json", DPoP: "proof" },
      body: "{}",
    });
    expect(seen.map((request) => request.headers.get("dpop"))).toEqual([null, "proof"]);
    expect(await (seen[0] as Request).text()).toBe("grant_type=authorization_code&code=c");
  });

  it("removes only the DPoP field from discovery metadata", () => {
    expect(withoutDpop({ issuer: "x", dpop_signing_alg_values_supported: ["EdDSA"] })).toEqual({
      issuer: "x",
    });
  });
});

describe("sign-in and consent pages", () => {
  it("posts credentials to a query-free fallback when page scripts fail", async () => {
    const { app, seen } = setup();
    const page = await app.request("/oauth/sign-in?sig=s&client_id=c");
    const html = await page.text();
    for (const id of ["password-form", "two-factor-form"]) {
      const form = html.match(new RegExp(`<form id="${id}"[^>]*>`))?.[0] ?? "";
      expect(form).toContain('method="post"');
      expect(form).toContain('action="/oauth/sign-in"');
      const fallback = new URL("/oauth/sign-in", "http://localhost");
      const response = await app.request(fallback.toString(), {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: new URLSearchParams({
          email: "flow@example.com",
          password: "secret",
          code: "123456",
        }),
      });
      expect(response.status).toBe(404);
      expect(response.headers.get("location")).toBeNull();
      expect(fallback.search).toBe("");
      expect(await response.text()).not.toContain("secret");
    }
    expect(seen).toHaveLength(0);
  });

  it("serves the sign-in page with a strict policy and the configured methods", async () => {
    const { app } = setup(undefined, undefined, { socialProviders: ["github"], passkeys: true });
    const res = await app.request("/oauth/sign-in?client_id=c&sig=s");
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toContain("text/html");
    const csp = res.headers.get("content-security-policy") ?? "";
    expect(csp).toContain("script-src 'self'");
    expect(csp).toContain("frame-ancestors 'none'");
    expect(csp).not.toContain("unsafe-inline");
    expect(res.headers.get("x-frame-options")).toBe("DENY");
    expect(res.headers.get("referrer-policy")).toBe("no-referrer");
    expect(res.headers.get("cache-control")).toBe("no-store");
    const html = await res.text();
    expect(html).toContain('id="password-form"');
    expect(html).toContain('data-provider="github"');
    expect(html).not.toContain('data-provider="google"');
    expect(html).toContain('id="passkey"');
    expect(html).toContain('src="/oauth/assets/oauth.js"');
    expect(html).not.toContain("<script>");
  });

  it("omits passkeys and social buttons that cannot work", async () => {
    const html = await (await setup().app.request("/oauth/sign-in")).text();
    expect(html).not.toContain("data-provider=");
    expect(html).not.toContain('id="passkey"');
  });

  it("serves the consent page with the same frame protection", async () => {
    const res = await setup().app.request("/oauth/consent");
    expect(res.status).toBe(200);
    expect(res.headers.get("x-frame-options")).toBe("DENY");
    const html = await res.text();
    expect(html).toContain('id="allow"');
    expect(html).toContain('id="deny"');
  });

  it("serves the page script and stylesheet with their types", async () => {
    const { app } = setup();
    const script = await app.request("/oauth/assets/oauth.js");
    expect(script.headers.get("content-type")).toContain("text/javascript");
    expect(await script.text()).toContain("oauth_query");
    const style = await app.request("/oauth/assets/oauth.css");
    expect(style.headers.get("content-type")).toContain("text/css");
  });

  it("offers passkeys only where the relying party covers this service's host", () => {
    const base = { github: undefined, google: undefined };
    expect(
      pageOptions({ ...base, baseUrl: "http://localhost:3001", passkeyRpId: "localhost" }).passkeys,
    ).toBe(true);
    expect(
      pageOptions({ ...base, baseUrl: "https://auth.example.com", passkeyRpId: "example.com" })
        .passkeys,
    ).toBe(true);
    expect(
      pageOptions({
        ...base,
        baseUrl: "https://auth.example.com",
        passkeyRpId: "app.example.com",
      }).passkeys,
    ).toBe(false);
    expect(
      pageOptions({
        github: { clientId: "i", clientSecret: "s" },
        google: undefined,
        baseUrl: "https://auth.example.com",
        passkeyRpId: "example.com",
      }).socialProviders,
    ).toEqual(["github"]);
  });
});

describe("clientAddress", () => {
  it("falls back to the peer when the trusted header is missing or a list", () => {
    const peer = "10.0.0.2";
    expect(clientAddress(new Headers(), peer, "x-real-ip")).toBe(peer);
    expect(clientAddress(new Headers({ "x-real-ip": "1.1.1.1, 2.2.2.2" }), peer, "x-real-ip")).toBe(
      peer,
    );
    expect(clientAddress(new Headers({ "x-real-ip": "1.1.1.1" }), peer, undefined)).toBe(peer);
    expect(clientAddress(new Headers({ "x-real-ip": " 1.1.1.1 " }), peer, "x-real-ip")).toBe(
      "1.1.1.1",
    );
  });
});
