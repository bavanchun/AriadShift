import { AuthError, createBetterAuthApi } from "@/components/account/auth-api";

const AUTH = "http://localhost:3901";

interface Call {
  url: string;
  method: string;
  body: unknown;
  credentials?: RequestCredentials;
}

/** A fetch that records requests and answers from `routes` keyed by "METHOD /path". */
function fakeFetch(routes: Record<string, () => Response>) {
  const calls: Call[] = [];
  const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : undefined;
    const url = request ? request.url : String(input);
    const method = (init?.method ?? request?.method ?? "GET").toUpperCase();
    const rawBody = init?.body ?? (request ? await request.clone().text() : undefined);
    calls.push({
      url,
      method,
      body: typeof rawBody === "string" && rawBody ? JSON.parse(rawBody) : undefined,
      credentials: init?.credentials ?? request?.credentials,
    });
    const route = routes[`${method} ${new URL(url).pathname}`];
    return route ? route() : Response.json({ message: "not found" }, { status: 404 });
  }) as typeof fetch;
  return { calls, fetchImpl };
}

describe("Better Auth adapter", () => {
  it("lists API keys from the 1.7 response shape", async () => {
    const { calls, fetchImpl } = fakeFetch({
      "GET /api/auth/api-key/list": () =>
        Response.json({
          apiKeys: [
            {
              id: "key-1",
              name: "CI",
              start: "ashift_ab",
              prefix: "ashift_",
              createdAt: "2026-10-01T09:00:00.000Z",
              expiresAt: null,
              enabled: true,
            },
          ],
          total: 1,
        }),
    });
    const auth = createBetterAuthApi(AUTH, fetchImpl);
    const keys = await auth.listApiKeys();
    expect(keys).toHaveLength(1);
    expect(keys[0]).toMatchObject({ id: "key-1", name: "CI", start: "ashift_ab", enabled: true });
    expect(keys[0]?.expiresAt).toBeNull();
    expect(calls[0]?.url.startsWith(`${AUTH}/api/auth/api-key/list`)).toBe(true);
  });

  it("creates a key with its lifetime in seconds and returns the secret", async () => {
    const { calls, fetchImpl } = fakeFetch({
      "POST /api/auth/api-key/create": () =>
        Response.json({
          id: "key-2",
          name: "Deploy",
          start: "ashift_cd",
          createdAt: "2026-10-25T09:00:00.000Z",
          expiresAt: "2027-01-23T09:00:00.000Z",
          enabled: true,
          key: "ashift_cdSECRET",
        }),
    });
    const auth = createBetterAuthApi(AUTH, fetchImpl);
    const created = await auth.createApiKey("Deploy", 90);
    expect(created.key).toBe("ashift_cdSECRET");
    expect(calls[0]?.body).toEqual({ name: "Deploy", expiresIn: 90 * 24 * 60 * 60 });
  });

  it("revokes a key by id", async () => {
    const { calls, fetchImpl } = fakeFetch({
      "POST /api/auth/api-key/delete": () => Response.json({ success: true }),
    });
    await createBetterAuthApi(AUTH, fetchImpl).deleteApiKey("key-1");
    expect(calls[0]?.body).toEqual({ keyId: "key-1" });
  });

  it("reports a two-factor sign-in", async () => {
    const { fetchImpl } = fakeFetch({
      "POST /api/auth/sign-in/email": () =>
        Response.json({ twoFactorRedirect: true, twoFactorMethods: ["totp"] }),
    });
    const auth = createBetterAuthApi(AUTH, fetchImpl);
    await expect(auth.signInEmail("ada@example.com", "pw")).resolves.toEqual({
      twoFactorRequired: true,
    });
  });

  it("turns a refusal into an AuthError with the service's message", async () => {
    const { fetchImpl } = fakeFetch({
      "POST /api/auth/sign-in/email": () =>
        Response.json(
          { code: "INVALID_EMAIL_OR_PASSWORD", message: "Invalid email or password" },
          { status: 401 },
        ),
    });
    const auth = createBetterAuthApi(AUTH, fetchImpl);
    const failure = await auth.signInEmail("ada@example.com", "wrong").catch((error) => error);
    expect(failure).toBeInstanceOf(AuthError);
    expect(failure.message).toBe("Invalid email or password");
    expect(failure.status).toBe(401);
  });

  it("explains a server that does not allow account deletion", async () => {
    const { fetchImpl } = fakeFetch({});
    const failure = await createBetterAuthApi(AUTH, fetchImpl)
      .deleteAccount("pw")
      .catch((error) => error);
    expect(failure).toBeInstanceOf(AuthError);
    expect(failure.message).toContain("does not allow deleting accounts");
  });

  it("detects whether an account has a password credential and supports passwordless deletion", async () => {
    let providerId = "github";
    const { calls, fetchImpl } = fakeFetch({
      "GET /api/auth/list-accounts": () => Response.json([{ id: "acc-1", providerId }]),
      "POST /api/auth/delete-user": () => Response.json({ success: true, message: "User deleted" }),
    });
    const auth = createBetterAuthApi(AUTH, fetchImpl);
    expect(await auth.hasPassword()).toBe(false);
    providerId = "credential";
    expect(await auth.hasPassword()).toBe(true);

    await auth.deleteAccount();
    const deleteCall = calls.find((call) => call.url === `${AUTH}/api/auth/delete-user`);
    expect(deleteCall?.body).toEqual({});
  });

  it("maps a stale session on account deletion to reauth_required", async () => {
    const { fetchImpl } = fakeFetch({
      "POST /api/auth/delete-user": () =>
        Response.json(
          { code: "reauth_required", message: "Sign in again before deleting your account." },
          { status: 400 },
        ),
    });
    const failure = await createBetterAuthApi(AUTH, fetchImpl)
      .deleteAccount()
      .catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(AuthError);
    expect(failure).toMatchObject({
      message: "Sign in again before deleting your account.",
      status: 400,
      code: "reauth_required",
    });
  });

  it("lists and revokes OAuth consents and registered clients", async () => {
    const { calls, fetchImpl } = fakeFetch({
      "GET /api/auth/oauth2/get-consents": () =>
        Response.json([
          {
            id: "consent-1",
            clientId: "client-1",
            clientName: "Claude Code",
            scopes: ["openid", "offline_access"],
            createdAt: "2026-10-30T10:00:00.000Z",
            updatedAt: "2026-10-30T11:00:00.000Z",
          },
        ]),
      "POST /api/auth/oauth2/delete-consent": () => Response.json(null),
      "GET /api/auth/oauth2/get-clients": () =>
        Response.json([
          {
            client_id: "client-2",
            client_name: "Cursor MCP",
            redirect_uris: ["http://127.0.0.1:33418/callback"],
            scope: "openid offline_access",
            client_id_issued_at: 1_761_818_400,
          },
        ]),
      "POST /api/auth/oauth2/delete-client": () => Response.json(null),
    });
    const auth = createBetterAuthApi(AUTH, fetchImpl);

    await expect(auth.listOAuthConsents()).resolves.toEqual([
      {
        id: "consent-1",
        clientId: "client-1",
        clientName: "Claude Code",
        scopes: ["openid", "offline_access"],
        createdAt: "2026-10-30T10:00:00.000Z",
        updatedAt: "2026-10-30T11:00:00.000Z",
      },
    ]);
    await auth.deleteOAuthConsent("consent-1");
    expect(
      calls.find((call) => call.url === `${AUTH}/api/auth/oauth2/delete-consent`)?.body,
    ).toEqual({ id: "consent-1" });

    await expect(auth.listOAuthClients()).resolves.toEqual([
      {
        clientId: "client-2",
        clientName: "Cursor MCP",
        redirectUris: ["http://127.0.0.1:33418/callback"],
        scopes: ["openid", "offline_access"],
        createdAt: new Date(1_761_818_400 * 1000).toISOString(),
      },
    ]);
    await auth.deleteOAuthClient("client-2");
    expect(
      calls.find((call) => call.url === `${AUTH}/api/auth/oauth2/delete-client`)?.body,
    ).toEqual({ client_id: "client-2" });
  });

  it("maps the session's user", async () => {
    const { fetchImpl } = fakeFetch({
      "GET /api/auth/get-session": () =>
        Response.json({
          session: { id: "s" },
          user: { id: "user-1", name: "Ada", email: "ada@example.com", twoFactorEnabled: true },
        }),
    });
    await expect(createBetterAuthApi(AUTH, fetchImpl).getSession()).resolves.toEqual({
      id: "user-1",
      name: "Ada",
      email: "ada@example.com",
      twoFactorEnabled: true,
    });
  });

  it("returns null without a session", async () => {
    const { fetchImpl } = fakeFetch({
      "GET /api/auth/get-session": () => Response.json(null),
    });
    await expect(createBetterAuthApi(AUTH, fetchImpl).getSession()).resolves.toBeNull();
  });

  it("reuses the API token and forgets it on sign-out", async () => {
    let issued = 0;
    const { calls, fetchImpl } = fakeFetch({
      "GET /api/auth/token": () => {
        issued += 1;
        return Response.json({ token: `jwt-${issued}` });
      },
      "POST /api/auth/sign-out": () => Response.json({ success: true }),
    });
    const auth = createBetterAuthApi(AUTH, fetchImpl);
    expect(await auth.apiToken()).toBe("jwt-1");
    expect(await auth.apiToken()).toBe("jwt-1");
    const tokenCall = calls.find((call) => call.url === `${AUTH}/api/auth/token`);
    expect(tokenCall?.credentials).toBe("include");
    await auth.signOut();
    expect(await auth.apiToken()).toBe("jwt-2");
  });

  it("distinguishes 401 session expiry from 503, 429 and network failures on token refresh", async () => {
    const unauthorizedFetch = fakeFetch({
      "GET /api/auth/token": () => new Response(null, { status: 401 }),
    });
    const unauthorized = await createBetterAuthApi(AUTH, unauthorizedFetch.fetchImpl)
      .apiToken()
      .catch((error: unknown) => error);
    expect(unauthorized).toBeInstanceOf(AuthError);
    expect(unauthorized).toMatchObject({
      message: "Your session has ended. Sign in again.",
      status: 401,
      code: "unauthorized",
    });

    const unavailableFetch = fakeFetch({
      "GET /api/auth/token": () => new Response(null, { status: 503 }),
    });
    const unavailable = await createBetterAuthApi(AUTH, unavailableFetch.fetchImpl)
      .apiToken()
      .catch((error: unknown) => error);
    expect(unavailable).toBeInstanceOf(AuthError);
    expect(unavailable).toMatchObject({
      message: "The server cannot verify your sign-in right now. Try again shortly.",
      status: 503,
      code: "unavailable",
    });

    const rateLimitedFetch = fakeFetch({
      "GET /api/auth/token": () => new Response(null, { status: 429 }),
    });
    const rateLimited = await createBetterAuthApi(AUTH, rateLimitedFetch.fetchImpl)
      .apiToken()
      .catch((error: unknown) => error);
    expect(rateLimited).toBeInstanceOf(AuthError);
    expect(rateLimited).toMatchObject({
      message: "Too many requests. Try again shortly.",
      status: 429,
      code: "rate_limited",
    });

    const networkFailure = await createBetterAuthApi(AUTH, (async () => {
      throw new TypeError("Failed to fetch");
    }) as typeof fetch)
      .apiToken()
      .catch((error: unknown) => error);
    expect(networkFailure).toBeInstanceOf(AuthError);
    expect(networkFailure).toMatchObject({
      message: "The sign-in service cannot be reached. Check your connection and try again.",
      status: undefined,
      code: "network_error",
    });
  });

  it("throws a typed AuthError when the token endpoint returns a malformed body", async () => {
    let step = 0;
    const { fetchImpl } = fakeFetch({
      "GET /api/auth/token": () => {
        step += 1;
        if (step === 1) {
          return new Response("not-json", {
            status: 200,
            headers: { "content-type": "application/json" },
          });
        }
        return Response.json({ token: "" });
      },
    });
    const auth = createBetterAuthApi(AUTH, fetchImpl);
    const nonJson = await auth.apiToken().catch((error: unknown) => error);
    expect(nonJson).toBeInstanceOf(AuthError);
    expect(nonJson).toMatchObject({ status: 200, code: "invalid_token_response" });

    const missingToken = await auth.apiToken().catch((error: unknown) => error);
    expect(missingToken).toBeInstanceOf(AuthError);
    expect(missingToken).toMatchObject({ status: 200, code: "invalid_token_response" });
  });
});
