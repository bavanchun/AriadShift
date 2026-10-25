import { AuthError, CloudAuth } from "@/lib/cloud/auth";

type Handler = (request: Request) => Response;

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
});
