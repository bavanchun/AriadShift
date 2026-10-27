// Account state for cloud mode, through the auth service (Better Auth) at its
// own origin. The session cookie belongs to that origin and is sent with
// `credentials: "include"`; the API only ever sees the short-lived JWT the
// service exchanges for it.

export interface AccountUser {
  id: string;
  email: string;
  name: string;
}

export type SignInResult = { status: "signed-in"; user: AccountUser } | { status: "two-factor" };

/** A refused auth request, with the service's message when it sent one. */
export class AuthError extends Error {
  override readonly name = "AuthError";

  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

/** Reuse a JWT for 14 of its 15 minutes, so it never reaches the API expired. */
const TOKEN_REUSE_MS = 14 * 60 * 1000;

const MESSAGES: Record<string, string> = {
  INVALID_EMAIL_OR_PASSWORD: "The email or password is not correct.",
  USER_ALREADY_EXISTS: "An account with this email already exists. Sign in instead.",
  USER_ALREADY_EXISTS_USE_ANOTHER_EMAIL:
    "An account with this email already exists. Sign in instead.",
  PASSWORD_TOO_SHORT: "The password is too short. Use at least 8 characters.",
  INVALID_CODE: "The code is not correct. Check your authenticator app and try again.",
  INVALID_BACKUP_CODE: "The backup code is not correct.",
};

function asUser(value: unknown): AccountUser | null {
  if (typeof value !== "object" || value === null) {
    return null;
  }
  const user = value as Record<string, unknown>;
  if (typeof user.id !== "string") {
    return null;
  }
  return {
    id: user.id,
    email: typeof user.email === "string" ? user.email : "",
    name: typeof user.name === "string" ? user.name : "",
  };
}

export class CloudAuth {
  readonly #baseUrl: string;
  readonly #fetch: typeof fetch;
  #token: { value: string; until: number } | null = null;
  #user: AccountUser | null = null;
  /** Known to have no session, so `bearer` skips asking for a token. */
  #anonymous = false;
  #pendingSession: Promise<AccountUser | null> | null = null;
  #pendingToken: Promise<string | undefined> | null = null;

  constructor(baseUrl: string, fetchImpl?: typeof fetch) {
    this.#baseUrl = baseUrl.replace(/\/+$/, "");
    this.#fetch = fetchImpl ?? ((input, init) => globalThis.fetch(input, init));
  }

  async #request(path: string, body?: unknown): Promise<Response> {
    return this.#fetch(`${this.#baseUrl}/api/auth/${path}`, {
      method: body === undefined ? "GET" : "POST",
      credentials: "include",
      headers: body === undefined ? undefined : { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  }

  async #json(path: string, body?: unknown): Promise<unknown> {
    const response = await this.#request(path, body);
    let data: unknown = null;
    try {
      const text = await response.text();
      data = text ? JSON.parse(text) : null;
    } catch {
      data = null;
    }
    if (!response.ok) {
      const detail = (typeof data === "object" && data !== null ? data : {}) as {
        code?: unknown;
        message?: unknown;
      };
      const code = typeof detail.code === "string" ? detail.code : `http_${response.status}`;
      const message =
        MESSAGES[code] ??
        (response.status === 429
          ? "Too many attempts. Wait a minute and try again."
          : typeof detail.message === "string" && detail.message
            ? detail.message
            : "The account service refused the request.");
      throw new AuthError(response.status, code, message);
    }
    this.#remember(response.headers.get("set-auth-jwt"));
    return data;
  }

  #remember(token: string | null) {
    if (token) {
      this.#token = { value: token, until: Date.now() + TOKEN_REUSE_MS };
    }
  }

  /** The signed-in user, or `null` without a session. */
  async session(): Promise<AccountUser | null> {
    this.#pendingSession ??= this.#fetchSession().finally(() => {
      this.#pendingSession = null;
    });
    return this.#pendingSession;
  }

  async #fetchSession(): Promise<AccountUser | null> {
    try {
      const data = (await this.#json("get-session")) as { user?: unknown } | null;
      const user = asUser(data?.user);
      this.#user = user;
      this.#anonymous = user === null;
      if (!user) {
        this.#token = null;
      }
      return user;
    } catch (error) {
      if (error instanceof AuthError && error.status === 401) {
        this.#user = null;
        this.#anonymous = true;
        this.#token = null;
      } else if (!this.#user) {
        this.#anonymous = true;
        this.#token = null;
      }
      throw error;
    }
  }

  async signIn(email: string, password: string): Promise<SignInResult> {
    const data = (await this.#json("sign-in/email", { email, password })) as {
      twoFactorRedirect?: unknown;
      user?: unknown;
    } | null;
    if (data?.twoFactorRedirect === true) {
      return { status: "two-factor" };
    }
    const user = asUser(data?.user) ?? (await this.session());
    if (!user) {
      throw new AuthError(500, "no_session", "Signed in, but the account service sent no session.");
    }
    this.#signedIn(user);
    return { status: "signed-in", user };
  }

  async signUp(name: string, email: string, password: string): Promise<AccountUser> {
    const data = (await this.#json("sign-up/email", { name, email, password })) as {
      user?: unknown;
    } | null;
    const user = asUser(data?.user) ?? (await this.session());
    if (!user) {
      throw new AuthError(500, "no_session", "The account was created, but no session started.");
    }
    this.#signedIn(user);
    return user;
  }

  /** Finishes a sign-in that needs two-factor authentication. */
  async verifyCode(code: string, kind: "totp" | "backup" = "totp"): Promise<AccountUser> {
    const path = kind === "totp" ? "two-factor/verify-totp" : "two-factor/verify-backup-code";
    const data = (await this.#json(path, { code })) as { user?: unknown } | null;
    const user = asUser(data?.user) ?? (await this.session());
    if (!user) {
      throw new AuthError(500, "no_session", "The code was accepted, but no session started.");
    }
    this.#signedIn(user);
    return user;
  }

  async signOut(): Promise<void> {
    this.#token = null;
    await this.#json("sign-out", {});
    this.#user = null;
    this.#anonymous = true;
  }

  #signedIn(user: AccountUser) {
    this.#user = user;
    this.#token = null;
    this.#anonymous = false;
  }

  /** Forgets the cached JWT, for example after the API answered 401. */
  forgetToken() {
    this.#token = null;
  }

  /**
   * The current JWT for the API, or `undefined` without a session, in which
   * case requests go out anonymously.
   */
  readonly bearer = async (): Promise<string | undefined> => {
    if (this.#pendingSession) {
      await this.#pendingSession.catch(() => null);
    }
    if (this.#token && Date.now() < this.#token.until) {
      return this.#token.value;
    }
    if (this.#anonymous) {
      return undefined;
    }
    this.#pendingToken ??= this.#fetchToken().finally(() => {
      this.#pendingToken = null;
    });
    return this.#pendingToken;
  };

  async #fetchToken(): Promise<string | undefined> {
    let response: Response;
    try {
      response = await this.#request("token");
    } catch {
      this.#token = null;
      if (this.#user) {
        throw new AuthError(
          503,
          "auth_unavailable",
          "The sign-in service is unavailable right now. Try again, or convert on this device.",
        );
      }
      this.#anonymous = true;
      return undefined;
    }
    if (!response.ok) {
      this.#token = null;
      if (response.status === 401) {
        this.#user = null;
        this.#anonymous = true;
        return undefined;
      }
      if (this.#user) {
        throw new AuthError(
          response.status,
          "auth_unavailable",
          "The sign-in service is unavailable right now. Try again, or convert on this device.",
        );
      }
      this.#anonymous = true;
      return undefined;
    }
    let data: { token?: unknown } | null = null;
    try {
      data = (await response.json()) as { token?: unknown };
    } catch {
      data = null;
    }
    const token = data?.token;
    if (typeof token !== "string" || !token) {
      this.#token = null;
      if (this.#user) {
        throw new AuthError(
          502,
          "auth_unavailable",
          "The sign-in service is unavailable right now. Try again, or convert on this device.",
        );
      }
      this.#anonymous = true;
      return undefined;
    }
    this.#remember(token);
    return token;
  }
}
