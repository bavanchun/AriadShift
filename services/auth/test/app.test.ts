import { describe, expect, it } from "vitest";
import { clientAddress, createApp, MAX_BODY_BYTES } from "../src/app.ts";
import { CLIENT_IP_HEADER } from "../src/auth.ts";

const WEB = "http://localhost:3000";

/** An app whose Better Auth handler records what it receives. */
function setup(trustedClientIpHeader?: string, peer = "203.0.113.7") {
  const seen: Request[] = [];
  const app = createApp({
    config: { trustedOrigins: [WEB], trustedClientIpHeader },
    handler: async (request) => {
      seen.push(request);
      return Response.json({ ok: true });
    },
    peerAddress: () => peer,
  });
  return { app, seen };
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

  it("serves nothing outside /api/auth", async () => {
    const { app, seen } = setup();
    expect((await app.request("/")).status).toBe(404);
    expect((await app.request("/api/other")).status).toBe(404);
    expect(seen).toHaveLength(0);
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
