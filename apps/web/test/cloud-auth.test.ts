import { AuthError, CloudAuth } from "@/lib/cloud/auth";

type Handler = (request: Request) => Response | Promise<Response>;

function scripted(...handlers: Handler[]) {
  const requests: Request[] = [];
  const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init);
    requests.push(request);
    const handler = handlers.shift();
    if (!handler) {
      throw new Error(`unexpected ${request.method} ${request.url}`);
    }
    return handler(request);
  }) as typeof fetch;
  return { fetch: fetchImpl, requests };
}

const USER = { id: "u1", email: "ada@example.com", name: "Ada" };

describe("CloudAuth", () => {
  it("reads the session with credentials and reuses the JWT it carries", async () => {
    const { fetch, requests } = scripted(() =>
      Response.json(
        { session: { id: "s1" }, user: USER },
        { headers: { "set-auth-jwt": "jwt-1" } },
      ),
    );
    const auth = new CloudAuth("https://auth.example.com/", fetch);
    await expect(auth.session()).resolves.toEqual(USER);
    expect(requests[0]?.url).toBe("https://auth.example.com/api/auth/get-session");
    expect(requests[0]?.credentials).toBe("include");
    await expect(auth.bearer()).resolves.toBe("jwt-1");
    expect(requests).toHaveLength(1);
  });

  it("goes out anonymously without a session and stops asking for tokens", async () => {
    const { fetch, requests } = scripted(() => Response.json(null));
    const auth = new CloudAuth("https://auth.example.com", fetch);
    await expect(auth.session()).resolves.toBeNull();
    await expect(auth.bearer()).resolves.toBeUndefined();
    await expect(auth.bearer()).resolves.toBeUndefined();
    expect(requests).toHaveLength(1);
  });

  it("signs in, then fetches a fresh token", async () => {
    const { fetch, requests } = scripted(
      () => Response.json({ redirect: false, token: "session", user: USER }),
      () => Response.json({ token: "jwt-2" }),
    );
    const auth = new CloudAuth("https://auth.example.com", fetch);
    await expect(auth.signIn("ada@example.com", "secret")).resolves.toEqual({
      status: "signed-in",
      user: USER,
    });
    expect(await requests[0]?.json()).toEqual({ email: "ada@example.com", password: "secret" });
    await expect(auth.bearer()).resolves.toBe("jwt-2");
    expect(requests[1]?.url).toBe("https://auth.example.com/api/auth/token");
  });

  it("asks for a second factor when the account has one", async () => {
    const { fetch, requests } = scripted(
      () => Response.json({ twoFactorRedirect: true }),
      () => Response.json({ token: "session", user: USER }),
    );
    const auth = new CloudAuth("https://auth.example.com", fetch);
    await expect(auth.signIn("ada@example.com", "secret")).resolves.toEqual({
      status: "two-factor",
    });
    await expect(auth.verifyCode("123456")).resolves.toEqual(USER);
    expect(requests[1]?.url).toBe("https://auth.example.com/api/auth/two-factor/verify-totp");
  });

  it("turns refusals into plain messages", async () => {
    const { fetch } = scripted(
      () =>
        Response.json(
          { code: "INVALID_EMAIL_OR_PASSWORD", message: "Invalid email or password" },
          { status: 401 },
        ),
      () => new Response(null, { status: 429 }),
    );
    const auth = new CloudAuth("https://auth.example.com", fetch);
    const wrong = auth.signIn("ada@example.com", "nope");
    await expect(wrong).rejects.toBeInstanceOf(AuthError);
    await expect(wrong).rejects.toThrow("The email or password is not correct.");
    await expect(auth.signUp("Ada", "ada@example.com", "secret12")).rejects.toThrow(
      "Too many attempts",
    );
  });

  it("forgets the token on sign-out", async () => {
    const { fetch, requests } = scripted(
      () => Response.json({ user: USER }, { headers: { "set-auth-jwt": "jwt-3" } }),
      () => Response.json({ success: true }),
    );
    const auth = new CloudAuth("https://auth.example.com", fetch);
    await auth.session();
    await auth.signOut();
    expect(requests[1]?.method).toBe("POST");
    expect(requests[1]?.url).toBe("https://auth.example.com/api/auth/sign-out");
    await expect(auth.bearer()).resolves.toBeUndefined();
  });

  it("marks the client anonymous when session() fails without a known user", async () => {
    const { fetch, requests } = scripted(
      () => {
        throw new TypeError("Failed to fetch");
      },
      () => new Response("unavailable", { status: 503 }),
    );
    const unreachableAuth = new CloudAuth("https://auth.example.com", fetch);
    await expect(unreachableAuth.session()).rejects.toThrow("Failed to fetch");
    await expect(unreachableAuth.bearer()).resolves.toBeUndefined();

    const errorAuth = new CloudAuth("https://auth.example.com", fetch);
    await expect(errorAuth.session()).rejects.toBeInstanceOf(AuthError);
    await expect(errorAuth.bearer()).resolves.toBeUndefined();
    expect(requests.map((r) => new URL(r.url).pathname)).toEqual([
      "/api/auth/get-session",
      "/api/auth/get-session",
    ]);
  });

  it("fails closed when /token is unreachable or returns 5xx for a signed-in user", async () => {
    const { fetch, requests } = scripted(
      () => Response.json({ redirect: false, token: "session", user: USER }),
      () => new Response("bad gateway", { status: 502 }),
      () => {
        throw new TypeError("Failed to fetch");
      },
      () => Response.json({ token: "jwt-recovered" }),
    );
    const auth = new CloudAuth("https://auth.example.com", fetch);
    await auth.signIn("ada@example.com", "secret");

    await expect(auth.bearer()).rejects.toThrow(/sign-in service is unavailable/i);
    await expect(auth.bearer()).rejects.toThrow(/sign-in service is unavailable/i);
    await expect(auth.bearer()).resolves.toBe("jwt-recovered");
    expect(requests.filter((r) => r.url.endsWith("/api/auth/token"))).toHaveLength(3);
  });

  it("deduplicates concurrent /token fetches", async () => {
    const { fetch, requests } = scripted(
      () => Response.json({ redirect: false, token: "session", user: USER }),
      () => Response.json({ token: "jwt-shared" }),
    );
    const auth = new CloudAuth("https://auth.example.com", fetch);
    await auth.signIn("ada@example.com", "secret");

    const tokens = await Promise.all([auth.bearer(), auth.bearer(), auth.bearer()]);
    expect(tokens).toEqual(["jwt-shared", "jwt-shared", "jwt-shared"]);
    expect(requests.filter((r) => r.url.endsWith("/api/auth/token"))).toHaveLength(1);
  });

  it("ignores a late /token response that resolves after sign-out succeeds", async () => {
    let resolveToken: ((response: Response) => void) | undefined;
    const tokenPromise = new Promise<Response>((resolve) => {
      resolveToken = resolve;
    });
    const { fetch, requests } = scripted(
      () => Response.json({ redirect: false, token: "session", user: USER }),
      () => tokenPromise,
      () => Response.json({ success: true }),
    );
    const auth = new CloudAuth("https://auth.example.com", fetch);
    await auth.signIn("ada@example.com", "secret");

    const pendingBearer = auth.bearer();
    expect(requests.map((r) => new URL(r.url).pathname)).toEqual([
      "/api/auth/sign-in/email",
      "/api/auth/token",
    ]);

    await auth.signOut();
    resolveToken?.(Response.json({ token: "jwt-late" }));

    await expect(pendingBearer).resolves.toBeUndefined();
    await expect(auth.bearer()).resolves.toBeUndefined();
    expect(requests.map((r) => new URL(r.url).pathname)).toEqual([
      "/api/auth/sign-in/email",
      "/api/auth/token",
      "/api/auth/sign-out",
    ]);
  });

  it("fails closed on 401 from /token or a refreshed session for a known signed-in user", async () => {
    const { fetch: tokenFetch } = scripted(
      () => Response.json({ redirect: false, token: "session", user: USER }),
      () => new Response("expired", { status: 401 }),
    );
    const tokenAuth = new CloudAuth("https://auth.example.com", tokenFetch);
    await tokenAuth.signIn("ada@example.com", "secret");
    await expect(tokenAuth.bearer()).rejects.toMatchObject({
      status: 401,
      code: "session_expired",
    });
    expect(tokenAuth.user).toBeNull();
    expect(tokenAuth.sessionExpired).toBe(true);
    await expect(tokenAuth.bearer()).rejects.toMatchObject({
      status: 401,
      code: "session_expired",
    });
    tokenAuth.continueAnonymously();
    expect(tokenAuth.sessionExpired).toBe(false);
    await expect(tokenAuth.bearer()).resolves.toBeUndefined();

    const { fetch: sessionFetch } = scripted(
      () => Response.json({ user: USER }, { headers: { "set-auth-jwt": "jwt-1" } }),
      () => new Response("expired", { status: 401 }),
    );
    const sessionAuth = new CloudAuth("https://auth.example.com", sessionFetch);
    await expect(sessionAuth.session()).resolves.toEqual(USER);
    await expect(sessionAuth.session()).rejects.toMatchObject({
      status: 401,
      code: "session_expired",
    });
    expect(sessionAuth.user).toBeNull();
    expect(sessionAuth.sessionExpired).toBe(true);
    await expect(sessionAuth.bearer()).rejects.toMatchObject({
      status: 401,
      code: "session_expired",
    });
  });

  it("clears the previous account's JWT when a session refresh switches to another user without set-auth-jwt", async () => {
    const userB = { id: "u2", email: "bob@example.com", name: "Bob" };
    const { fetch, requests } = scripted(
      () => Response.json({ user: USER }, { headers: { "set-auth-jwt": "jwt-a" } }),
      () => Response.json({ user: userB }),
      () => Response.json({ token: "jwt-b" }),
    );
    const auth = new CloudAuth("https://auth.example.com", fetch);
    await expect(auth.session()).resolves.toEqual(USER);
    await expect(auth.bearer()).resolves.toBe("jwt-a");

    await expect(auth.session()).resolves.toEqual(userB);
    expect(auth.user).toEqual(userB);
    await expect(auth.bearer()).resolves.toBe("jwt-b");
    expect(requests.map((r) => new URL(r.url).pathname)).toEqual([
      "/api/auth/get-session",
      "/api/auth/get-session",
      "/api/auth/token",
    ]);
  });

  it("keeps the newer pending session handle when an older refresh settles across sign-out and sign-in", async () => {
    let sessionCount = 0;
    let resolveOld: ((response: Response) => void) | undefined;
    let resolveNew: ((response: Response) => void) | undefined;
    const pendingOld = new Promise<Response>((resolve) => {
      resolveOld = resolve;
    });
    const pendingNew = new Promise<Response>((resolve) => {
      resolveNew = resolve;
    });
    const fetchImpl = (async (input: RequestInfo | URL) => {
      const path = new URL(input instanceof Request ? input.url : String(input)).pathname;
      if (path.endsWith("/get-session")) {
        sessionCount += 1;
        if (sessionCount === 1) {
          return Response.json({ user: { id: "a", email: "a@example.com", name: "A" } });
        }
        return sessionCount === 2 ? pendingOld : pendingNew;
      }
      if (path.endsWith("/sign-out")) {
        return Response.json({ success: true });
      }
      if (path.endsWith("/sign-in/email")) {
        return Response.json(
          { user: { id: "b", email: "b@example.com", name: "B" } },
          { headers: { "set-auth-jwt": "jwt-b" } },
        );
      }
      if (path.endsWith("/token")) {
        return Response.json({ token: "jwt-b" });
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;

    const auth = new CloudAuth("https://auth.example.com", fetchImpl);
    await auth.session();
    const oldRefresh = auth.session();
    await auth.signOut();
    await auth.signIn("b@example.com", "password");
    const currentRefresh = auth.session();

    resolveOld?.(Response.json({ user: { id: "a", email: "a@example.com", name: "A" } }));
    await oldRefresh;
    expect(auth.user?.id).toBe("b");

    let bearerSettled = false;
    const pendingBearer = auth.bearer().finally(() => {
      bearerSettled = true;
    });
    await Promise.resolve();
    expect(bearerSettled).toBe(false);

    resolveNew?.(new Response("", { status: 401 }));
    await expect(currentRefresh).rejects.toMatchObject({
      status: 401,
      code: "session_expired",
    });
    await expect(pendingBearer).rejects.toMatchObject({
      status: 401,
      code: "session_expired",
    });
    expect(auth.user).toBeNull();
    expect(auth.sessionExpired).toBe(true);
  });

  it("ignores delayed sign-in, sign-up, and verification results after continueAnonymously or cancel", async () => {
    for (const op of ["sign-in", "sign-up", "verify"] as const) {
      for (const interrupt of ["anonymous", "cancel"] as const) {
        let resolveOp: ((response: Response) => void) | undefined;
        const pendingOp = new Promise<Response>((resolve) => {
          resolveOp = resolve;
        });
        const fetchImpl = (async () => pendingOp) as typeof fetch;
        const auth = new CloudAuth("https://auth.example.com", fetchImpl);
        auth.continueAnonymously();

        const promise =
          op === "sign-in"
            ? auth.signIn("ada@example.com", "secret")
            : op === "sign-up"
              ? auth.signUp("Ada", "ada@example.com", "secret12")
              : auth.verifyCode("123456");

        if (interrupt === "anonymous") {
          auth.continueAnonymously();
        } else {
          auth.cancelPendingAuth();
        }

        resolveOp?.(
          Response.json({ user: USER }, { headers: { "set-auth-jwt": `jwt-${op}-${interrupt}` } }),
        );
        await promise;

        expect(auth.user).toBeNull();
        await expect(auth.bearer()).resolves.toBeUndefined();
      }
    }
  });

  it("maintains the JWT ownership invariant across interleaved transitions with deferred responses", async () => {
    type TransitionName =
      | "signInA"
      | "signUpB"
      | "verifyA"
      | "sessionSwitchB"
      | "signOut"
      | "continueAnonymously"
      | "cancelPendingAuth";

    const transitions: TransitionName[] = [
      "signInA",
      "signUpB",
      "verifyA",
      "sessionSwitchB",
      "signOut",
      "continueAnonymously",
      "cancelPendingAuth",
    ];

    const userA = { id: "a", email: "a@example.com", name: "Ada" };
    const userB = { id: "b", email: "b@example.com", name: "Bob" };

    async function assertInvariant(auth: CloudAuth) {
      if (auth.sessionExpired) {
        expect(auth.user).toBeNull();
        await expect(auth.bearer()).rejects.toMatchObject({
          status: 401,
          code: "session_expired",
        });
        return;
      }
      const token = await auth.bearer();
      if (auth.user === null) {
        expect(token).toBeUndefined();
      } else {
        expect(token).toBe(`jwt-${auth.user.id}`);
      }
    }

    for (const first of transitions) {
      for (const second of transitions) {
        const deferred: Array<() => void> = [];
        let activeServerUser: typeof userA | null = userA;

        const fetchImpl = (async (input: RequestInfo | URL) => {
          const path = new URL(input instanceof Request ? input.url : String(input)).pathname;
          if (path.endsWith("/token")) {
            if (!activeServerUser) {
              return new Response("unauthorized", { status: 401 });
            }
            return Response.json({ token: `jwt-${activeServerUser.id}` });
          }
          return new Promise<Response>((resolve) => {
            deferred.push(() => {
              if (path.endsWith("/sign-in/email") || path.endsWith("/two-factor/verify-totp")) {
                activeServerUser = userA;
                resolve(Response.json({ user: userA }, { headers: { "set-auth-jwt": "jwt-a" } }));
                return;
              }
              if (path.endsWith("/sign-up/email")) {
                activeServerUser = userB;
                resolve(Response.json({ user: userB }, { headers: { "set-auth-jwt": "jwt-b" } }));
                return;
              }
              if (path.endsWith("/get-session")) {
                activeServerUser = userB;
                resolve(Response.json({ user: userB }));
                return;
              }
              if (path.endsWith("/sign-out")) {
                activeServerUser = null;
                resolve(Response.json({ success: true }));
                return;
              }
              resolve(Response.json(null));
            });
          });
        }) as typeof fetch;

        const auth = new CloudAuth("https://auth.example.com", fetchImpl);
        const start = (step: TransitionName): Promise<unknown> => {
          switch (step) {
            case "signInA":
              return auth.signIn("a@example.com", "secret");
            case "signUpB":
              return auth.signUp("Bob", "b@example.com", "secret12");
            case "verifyA":
              return auth.verifyCode("123456");
            case "sessionSwitchB":
              return auth.session();
            case "signOut":
              return auth.signOut();
            case "continueAnonymously":
              auth.continueAnonymously();
              return Promise.resolve();
            case "cancelPendingAuth":
              auth.cancelPendingAuth();
              return Promise.resolve();
          }
        };

        // Start `first` (which may queue a deferred response), then run `second`,
        // resolve `second` before `first` so `first` arrives late, and check the invariant.
        const p1 = start(first);
        const p2 = start(second);
        while (deferred.length > 0) {
          const flush = deferred.pop();
          flush?.();
          await Promise.resolve();
        }
        await Promise.allSettled([p1, p2]);
        // Synchronize activeServerUser with auth.user when a valid user is active.
        activeServerUser = auth.user?.id === "a" ? userA : auth.user?.id === "b" ? userB : null;
        await assertInvariant(auth);
      }
    }

    // Explicit multi-step sequence B: sign-in A -> session switch to B (no JWT header) -> anonymous,
    // with a late sign-in A and late /token for A resolving after anonymous mode is chosen.
    let resolveLateTokenA: ((response: Response) => void) | undefined;
    let resolveLateSignInA: ((response: Response) => void) | undefined;
    let sessionStep = 0;
    const multiFetch = (async (input: RequestInfo | URL) => {
      const path = new URL(input instanceof Request ? input.url : String(input)).pathname;
      if (path.endsWith("/sign-in/email")) {
        sessionStep += 1;
        if (sessionStep === 1) {
          return Response.json({ user: userA }, { headers: { "set-auth-jwt": "jwt-a" } });
        }
        return new Promise<Response>((resolve) => {
          resolveLateSignInA = resolve;
        });
      }
      if (path.endsWith("/get-session")) {
        return Response.json({ user: userB });
      }
      if (path.endsWith("/token")) {
        if (!resolveLateTokenA) {
          return new Promise<Response>((resolve) => {
            resolveLateTokenA = resolve;
          });
        }
        return Response.json({ token: "jwt-b" });
      }
      return Response.json({ success: true });
    }) as typeof fetch;

    const multiAuth = new CloudAuth("https://auth.example.com", multiFetch);
    await multiAuth.signIn("a@example.com", "secret");
    await assertInvariant(multiAuth);
    expect(multiAuth.user?.id).toBe("a");

    await multiAuth.session();
    expect(multiAuth.user?.id).toBe("b");
    const pendingTokenForB = multiAuth.bearer();
    const lateSignIn = multiAuth.signIn("a@example.com", "secret");
    multiAuth.continueAnonymously();

    resolveLateTokenA?.(Response.json({ token: "jwt-a" }));
    resolveLateSignInA?.(Response.json({ user: userA }, { headers: { "set-auth-jwt": "jwt-a" } }));
    await Promise.allSettled([pendingTokenForB, lateSignIn]);

    expect(multiAuth.user).toBeNull();
    await expect(pendingTokenForB).resolves.toBeUndefined();
    await assertInvariant(multiAuth);
  });
});
