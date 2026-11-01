// The OAuth authorization server end to end, in process: the real Better
// Auth plugins and HTTP layer over Better Auth's memory adapter. It follows
// what an MCP client does: discovery, dynamic registration, authorization
// with PKCE through the sign-in and consent steps, the token exchange and a
// refresh, and checks the access token the way ariad-server verifies it.

import { createPublicKey, type JsonWebKeyInput, verify } from "node:crypto";
import { beforeAll, describe, expect, it } from "vitest";
import {
  AUTH,
  audiences,
  authorizeUrl,
  createService,
  decode,
  Jar,
  json,
  pkce,
  post,
  REDIRECT_URI,
  RESOURCE,
  signedQuery,
  token,
} from "./oauth-helpers.ts";

describe("OAuth authorization for remote MCP", () => {
  const { app } = createService();
  const email = "flow@example.com";
  const password = "correct horse battery staple";
  let userId = "";
  let metadata: Record<string, unknown> = {};
  let clientId = "";

  beforeAll(async () => {
    const signUp = await post(app, "/api/auth/sign-up/email", { name: "Flow", email, password });
    expect(signUp.status).toBe(200);
    userId = ((await json(signUp)).user as { id: string }).id;
  });

  it("publishes discovery for the auth origin with S256 PKCE and no client credentials", async () => {
    const res = await app.request(`${AUTH}/.well-known/oauth-authorization-server`);
    expect(res.status).toBe(200);
    metadata = await json(res);
    expect(metadata.issuer).toBe(AUTH);
    expect(metadata.authorization_endpoint).toBe(`${AUTH}/api/auth/oauth2/authorize`);
    expect(metadata.token_endpoint).toBe(`${AUTH}/api/auth/oauth2/token`);
    expect(metadata.registration_endpoint).toBe(`${AUTH}/api/auth/oauth2/register`);
    expect(metadata.client_id_metadata_document_supported).toBe(true);
    expect(metadata.jwks_uri).toBe(`${AUTH}/api/auth/jwks`);
    expect(metadata.code_challenge_methods_supported).toEqual(["S256"]);
    expect(metadata.grant_types_supported).toEqual(["authorization_code", "refresh_token"]);
    expect(metadata.response_types_supported).toEqual(["code"]);
    expect(metadata.token_endpoint_auth_methods_supported).toContain("none");
    expect(metadata).not.toHaveProperty("dpop_signing_alg_values_supported");

    const oidc = await json(await app.request(`${AUTH}/.well-known/openid-configuration`));
    expect(oidc.issuer).toBe(AUTH);
    expect(oidc.registration_endpoint).toBe(`${AUTH}/api/auth/oauth2/register`);
    expect(oidc.client_id_metadata_document_supported).toBe(true);
    expect(oidc.code_challenge_methods_supported).toEqual(["S256"]);
  });

  it("registers a public client without an account (RFC 7591)", async () => {
    const res = await post(app, "/api/auth/oauth2/register", {
      client_name: "Flow test client",
      redirect_uris: [REDIRECT_URI],
      token_endpoint_auth_method: "none",
      grant_types: ["authorization_code", "refresh_token"],
      response_types: ["code"],
    });
    expect([200, 201]).toContain(res.status);
    const client = await json(res);
    expect(typeof client.client_id).toBe("string");
    expect(client).not.toHaveProperty("client_secret");
    expect(client.redirect_uris).toEqual([REDIRECT_URI]);
    clientId = client.client_id as string;
  });

  it("refuses to register a client for the client credentials grant", async () => {
    const res = await post(app, "/api/auth/oauth2/register", {
      client_name: "Machine client",
      redirect_uris: [REDIRECT_URI],
      grant_types: ["client_credentials"],
    });
    expect(res.status).toBeGreaterThanOrEqual(400);
  });

  it("rejects authorization requests without PKCE or with the plain method", async () => {
    const { challenge } = pkce();
    for (const extra of [
      {},
      { code_challenge: challenge },
      { code_challenge: challenge, code_challenge_method: "plain" },
    ]) {
      const res = await app.request(authorizeUrl(clientId, extra));
      expect(res.status, JSON.stringify(extra)).toBe(302);
      const location = new URL(res.headers.get("location") ?? "");
      expect(`${location.origin}${location.pathname}`).toBe(REDIRECT_URI);
      expect(location.searchParams.get("error")).toBe("invalid_request");
      expect(location.searchParams.get("state")).toBe("state-1");
    }
  });

  it("rejects resources other than the MCP endpoint (RFC 8707)", async () => {
    const { challenge } = pkce();
    const res = await app.request(
      authorizeUrl(clientId, {
        code_challenge: challenge,
        code_challenge_method: "S256",
        resource: "http://localhost:8080/v1",
      }),
    );
    expect(res.status).toBe(302);
    const location = new URL(res.headers.get("location") ?? "");
    expect(location.searchParams.get("error")).toBe("invalid_target");
  });

  it("signs in, asks for consent and issues a resource-bound access token", async () => {
    const jar = new Jar();
    const { verifier, challenge } = pkce();

    const authorize = await app.request(
      authorizeUrl(clientId, { code_challenge: challenge, code_challenge_method: "S256" }),
    );
    expect(authorize.status).toBe(302);
    const signInUrl = authorize.headers.get("location") ?? "";
    expect(signInUrl.startsWith(`${AUTH}/oauth/sign-in?`)).toBe(true);
    expect(new URL(signInUrl).searchParams.get("sig")).toBeTruthy();
    expect((await app.request(signInUrl)).status).toBe(200);

    // The sign-in page posts the credentials with the signed query.
    const signIn = await post(app, "/api/auth/sign-in/email", {
      email,
      password,
      oauth_query: signedQuery(signInUrl),
    });
    expect(signIn.status).toBe(200);
    jar.store(signIn);
    const afterSignIn = await json(signIn);
    expect(afterSignIn.redirect).toBe(true);
    const consentUrl = afterSignIn.url as string;
    expect(consentUrl.startsWith(`${AUTH}/oauth/consent?`)).toBe(true);
    expect(new URL(consentUrl).searchParams.get("client_id")).toBe(clientId);
    expect((await app.request(consentUrl)).status).toBe(200);

    const consent = await post(
      app,
      "/api/auth/oauth2/consent",
      { accept: true, oauth_query: signedQuery(consentUrl) },
      jar.header(),
    );
    expect(consent.status).toBe(200);
    const callback = new URL((await json(consent)).url as string);
    expect(`${callback.origin}${callback.pathname}`).toBe(REDIRECT_URI);
    expect(callback.searchParams.get("state")).toBe("state-1");
    expect(callback.searchParams.get("iss")).toBe(AUTH);
    const code = callback.searchParams.get("code") ?? "";
    expect(code).not.toBe("");

    const issued = await token(app, {
      grant_type: "authorization_code",
      code,
      redirect_uri: REDIRECT_URI,
      client_id: clientId,
      code_verifier: verifier,
      resource: RESOURCE,
    });
    expect(issued.status).toBe(200);
    const tokens = await json(issued);
    expect(tokens.token_type).toBe("Bearer");
    expect(tokens.expires_in).toBe(900);
    expect(typeof tokens.refresh_token).toBe("string");

    // The claims ariad-server checks: EdDSA key from the JWKS, issuer, the
    // MCP audience, the user as subject and a 15-minute expiry.
    const access = decode(tokens.access_token as string);
    expect(access.header.alg).toBe("EdDSA");
    expect(typeof access.header.kid).toBe("string");
    expect(access.payload.iss).toBe(AUTH);
    expect(audiences(access.payload.aud)).toContain(RESOURCE);
    expect(audiences(access.payload.aud)).not.toContain(AUTH);
    expect(access.payload.sub).toBe(userId);
    expect(access.payload.exp - access.payload.iat).toBe(900);
    expect(access.payload.client_id).toBe(clientId);

    const jwks = await json(await app.request(`${AUTH}/api/auth/jwks`));
    const jwk = (jwks.keys as Array<Record<string, string>>).find(
      (key) => key.kid === access.header.kid,
    );
    expect(jwk).toBeDefined();
    const key = createPublicKey({ key: jwk as JsonWebKeyInput["key"], format: "jwk" });
    expect(verify(null, Buffer.from(access.signed), key, access.signature)).toBe(true);

    // The session's ordinary API JWT keeps the auth origin as its audience,
    // so ariad-server's MCP endpoint refuses it.
    const apiJwt = await app.request(`${AUTH}/api/auth/token`, { headers: jar.header() });
    expect(apiJwt.status).toBe(200);
    const api = decode((await json(apiJwt)).token as string);
    expect(api.payload.aud).toBe(AUTH);
    expect(api.payload.sub).toBe(userId);

    // A refresh keeps the resource binding. A DPoP proof is ignored, so the
    // client still gets a Bearer token that ariad-server accepts.
    const refreshed = await token(
      app,
      {
        grant_type: "refresh_token",
        refresh_token: tokens.refresh_token as string,
        client_id: clientId,
        resource: RESOURCE,
      },
      { dpop: "not-a-proof" },
    );
    expect(refreshed.status).toBe(200);
    const renewed = await json(refreshed);
    expect(renewed.token_type).toBe("Bearer");
    const renewedAccess = decode(renewed.access_token as string);
    expect(audiences(renewedAccess.payload.aud)).toContain(RESOURCE);
    expect(renewedAccess.payload.sub).toBe(userId);
    expect(renewedAccess.payload).not.toHaveProperty("cnf");

    // Consent is remembered: the next authorization returns a code directly,
    // and that code is useless without the matching PKCE verifier.
    const second = pkce();
    const again = await app.request(
      authorizeUrl(clientId, { code_challenge: second.challenge, code_challenge_method: "S256" }),
      { headers: jar.header() },
    );
    expect(again.status).toBe(302);
    const secondCode = new URL(again.headers.get("location") ?? "").searchParams.get("code") ?? "";
    expect(secondCode).not.toBe("");
    const wrongVerifier = await token(app, {
      grant_type: "authorization_code",
      code: secondCode,
      redirect_uri: REDIRECT_URI,
      client_id: clientId,
      code_verifier: pkce().verifier,
      resource: RESOURCE,
    });
    expect(wrongVerifier.status).toBe(400);
    expect((await json(wrongVerifier)).error).toBe("invalid_grant");
  });

  it("refuses the client credentials grant at the token endpoint", async () => {
    const res = await token(app, { grant_type: "client_credentials", client_id: clientId });
    expect(res.status).toBe(400);
    expect((await json(res)).error).toBe("unsupported_grant_type");
  });
});
