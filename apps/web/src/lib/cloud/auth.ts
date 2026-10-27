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

const SESSION_EXPIRED_MESSAGE =
  "Your session has ended. Sign in again; the file you chose stays here.";

export class CloudAuth {
  readonly #baseUrl: string;
  readonly #fetch: typeof fetch;
  readonly #listeners = new Set<() => void>();
  #token: { value: string; until: number; userId: string | null; epoch: number } | null = null;
  #user: AccountUser | null = null;
  /** Known to have no session, so `bearer` skips asking for a token. */
  #anonymous = false;
  /** Known signed-in user whose session later expired; fails closed until resolved. */
  #sessionExpired = false;
  /** Incremented on every auth state transition to invalidate in-flight requests. */
  #epoch = 0;
  #pendingSession: Promise<AccountUser | null> | null = null;
  #pendingToken: Promise<string | undefined> | null = null;

  constructor(baseUrl: string, fetchImpl?: typeof fetch) {
    this.#baseUrl = baseUrl.replace(/\/+$/, "");
    this.#fetch = fetchImpl ?? ((input, init) => globalThis.fetch(input, init));
  }

  get user(): AccountUser | null {
    return this.#user;
  }

  get sessionExpired(): boolean {
    return this.#sessionExpired;
  }

  subscribe(listener: () => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  #notify() {
    for (const listener of this.#listeners) {
      listener();
    }
  }

  #transition(next: {
    user: AccountUser | null;
    anonymous: boolean;
    sessionExpired: boolean;
    token?: string | null;
    notify?: boolean;
  }) {
    this.#epoch += 1;
    this.#user = next.user;
    this.#anonymous = next.anonymous;
    this.#sessionExpired = next.sessionExpired;
    this.#token = null;
    this.#pendingSession = null;
    this.#pendingToken = null;
    if (next.user && next.token) {
      this.#remember(next.token, this.#epoch, next.user.id);
    }
    if (next.notify !== false) {
      this.#notify();
    }
  }

  async #request(path: string, body?: unknown): Promise<Response> {
    return this.#fetch(`${this.#baseUrl}/api/auth/${path}`, {
      method: body === undefined ? "GET" : "POST",
      credentials: "include",
      headers: body === undefined ? undefined : { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  }

  async #json<T = unknown>(
    path: string,
    body?: unknown,
  ): Promise<{ data: T | null; jwt: string | null }> {
    const response = await this.#request(path, body);
    let data: T | null = null;
    try {
      const text = await response.text();
      data = text ? (JSON.parse(text) as T) : null;
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
    return { data, jwt: response.headers.get("set-auth-jwt") };
  }

  #remember(
    token: string | null | undefined,
    expectedEpoch = this.#epoch,
    userId = this.#user?.id ?? null,
  ) {
    if (
      !token ||
      this.#epoch !== expectedEpoch ||
      (this.#user?.id ?? null) !== userId ||
      this.#anonymous ||
      this.#sessionExpired
    ) {
      return;
    }
    this.#token = {
      value: token,
      until: Date.now() + TOKEN_REUSE_MS,
      userId,
      epoch: this.#epoch,
    };
  }

  #expireSession() {
    this.#transition({ user: null, anonymous: false, sessionExpired: true });
  }

  /** The signed-in user, or `null` without a session. */
  async session(): Promise<AccountUser | null> {
    if (!this.#pendingSession) {
      const epoch = this.#epoch;
      const pending = this.#fetchSession(epoch).finally(() => {
        if (this.#pendingSession === pending) {
          this.#pendingSession = null;
        }
      });
      this.#pendingSession = pending;
    }
    return this.#pendingSession;
  }

  async #fetchSession(epoch: number): Promise<AccountUser | null> {
    const hadUser = this.#user !== null || this.#sessionExpired;
    try {
      const { data, jwt } = await this.#json<{ user?: unknown }>("get-session");
      if (this.#epoch !== epoch) {
        return this.#user;
      }
      const user = asUser(data?.user);
      if (!user) {
        if (hadUser) {
          this.#expireSession();
          throw new AuthError(401, "session_expired", SESSION_EXPIRED_MESSAGE);
        }
        this.#transition({ user: null, anonymous: true, sessionExpired: false });
        return null;
      }
      if (this.#user?.id !== user.id || this.#anonymous || this.#sessionExpired) {
        this.#transition({ user, anonymous: false, sessionExpired: false, token: jwt });
      } else {
        this.#user = user;
        if (jwt) {
          this.#remember(jwt, epoch, user.id);
        }
        this.#notify();
      }
      return user;
    } catch (error) {
      if (this.#epoch !== epoch) {
        throw error;
      }
      if (error instanceof AuthError && error.status === 401) {
        if (hadUser) {
          this.#expireSession();
          throw new AuthError(401, "session_expired", SESSION_EXPIRED_MESSAGE);
        }
        this.#transition({ user: null, anonymous: true, sessionExpired: false });
      } else if (!this.#user && !this.#sessionExpired) {
        this.#transition({ user: null, anonymous: true, sessionExpired: false });
      }
      throw error;
    }
  }

  async #completeAuth(
    rawUser: unknown,
    jwt: string | null,
    epoch: number,
    noSessionMessage: string,
  ): Promise<AccountUser | null> {
    // If another transition (such as continueAnonymously, signOut, or dialog
    // cancel) occurred while the auth request was in flight, discard its result
    // and any Set-Cookie / set-auth-jwt headers it returned. The API only ever
    // accepts short-lived bearer JWTs from `bearer()`, so an unused cookie on
    // the auth origin never authenticates API calls during this session.
    if (this.#epoch !== epoch) {
      return asUser(rawUser);
    }
    const direct = asUser(rawUser);
    if (direct) {
      this.#transition({ user: direct, anonymous: false, sessionExpired: false, token: jwt });
      return direct;
    }
    const { data, jwt: sessionJwt } = await this.#json<{ user?: unknown }>("get-session");
    if (this.#epoch !== epoch) {
      return asUser(data?.user);
    }
    const fetched = asUser(data?.user);
    if (!fetched) {
      throw new AuthError(500, "no_session", noSessionMessage);
    }
    this.#transition({
      user: fetched,
      anonymous: false,
      sessionExpired: false,
      token: sessionJwt ?? jwt,
    });
    return fetched;
  }

  async signIn(email: string, password: string): Promise<SignInResult> {
    const epoch = this.#epoch;
    const { data, jwt } = await this.#json<{
      twoFactorRedirect?: unknown;
      user?: unknown;
    }>("sign-in/email", { email, password });
    if (data?.twoFactorRedirect === true && this.#epoch === epoch) {
      return { status: "two-factor" };
    }
    const user = await this.#completeAuth(
      data?.user,
      jwt,
      epoch,
      "Signed in, but the account service sent no session.",
    );
    return { status: "signed-in", user: user ?? { id: "", email, name: "" } };
  }

  async signUp(name: string, email: string, password: string): Promise<AccountUser> {
    const epoch = this.#epoch;
    const { data, jwt } = await this.#json<{ user?: unknown }>("sign-up/email", {
      name,
      email,
      password,
    });
    const user = await this.#completeAuth(
      data?.user,
      jwt,
      epoch,
      "The account was created, but no session started.",
    );
    return user ?? { id: "", email, name };
  }

  /** Finishes a sign-in that needs two-factor authentication. */
  async verifyCode(code: string, kind: "totp" | "backup" = "totp"): Promise<AccountUser> {
    const epoch = this.#epoch;
    const path = kind === "totp" ? "two-factor/verify-totp" : "two-factor/verify-backup-code";
    const { data, jwt } = await this.#json<{ user?: unknown }>(path, { code });
    const user = await this.#completeAuth(
      data?.user,
      jwt,
      epoch,
      "The code was accepted, but no session started.",
    );
    return user ?? { id: "", email: "", name: "" };
  }

  async signOut(): Promise<void> {
    const epoch = this.#epoch;
    await this.#json("sign-out", {});
    if (this.#epoch === epoch) {
      this.#transition({ user: null, anonymous: true, sessionExpired: false });
    }
  }

  /** Explicitly opts into anonymous mode after a signed-in session expired or was canceled. */
  continueAnonymously() {
    this.#transition({ user: null, anonymous: true, sessionExpired: false });
  }

  /** Invalidates any in-flight sign-in, sign-up, or verification request when the dialog closes. */
  cancelPendingAuth() {
    this.#epoch += 1;
    this.#pendingSession = null;
    this.#pendingToken = null;
    if (this.#token && this.#user && !this.#anonymous && !this.#sessionExpired) {
      this.#token = { ...this.#token, epoch: this.#epoch };
    } else {
      this.#token = null;
    }
  }

  /** Forgets the cached JWT, for example after the API answered 401. */
  forgetToken() {
    this.#epoch += 1;
    this.#token = null;
    this.#pendingToken = null;
  }

  /**
   * The current JWT for the API, or `undefined` without a session, in which
   * case requests go out anonymously.
   */
  readonly bearer = async (): Promise<string | undefined> => {
    while (this.#pendingSession) {
      await this.#pendingSession.catch(() => null);
    }
    if (this.#sessionExpired) {
      throw new AuthError(401, "session_expired", SESSION_EXPIRED_MESSAGE);
    }
    if (
      this.#token &&
      this.#token.epoch === this.#epoch &&
      this.#token.userId === (this.#user?.id ?? null) &&
      Date.now() < this.#token.until
    ) {
      return this.#token.value;
    }
    if (this.#anonymous) {
      return undefined;
    }
    if (!this.#pendingToken) {
      const epoch = this.#epoch;
      const userId = this.#user?.id ?? null;
      const pending = this.#fetchToken(epoch, userId).finally(() => {
        if (this.#pendingToken === pending) {
          this.#pendingToken = null;
        }
      });
      this.#pendingToken = pending;
    }
    return this.#pendingToken;
  };

  async #fetchToken(epoch: number, userId: string | null): Promise<string | undefined> {
    let response: Response;
    try {
      response = await this.#request("token");
    } catch {
      if (this.#pendingSession) {
        await this.#pendingSession.catch(() => null);
      }
      if (this.#epoch !== epoch || (this.#user?.id ?? null) !== userId) {
        return this.bearer();
      }
      this.#token = null;
      if (this.#user) {
        throw new AuthError(
          503,
          "auth_unavailable",
          "The sign-in service is unavailable right now. Try again, or convert on this device.",
        );
      }
      this.#transition({ user: null, anonymous: true, sessionExpired: false });
      return undefined;
    }
    if (this.#pendingSession) {
      await this.#pendingSession.catch(() => null);
    }
    if (this.#epoch !== epoch || (this.#user?.id ?? null) !== userId) {
      return this.bearer();
    }
    if (!response.ok) {
      this.#token = null;
      if (response.status === 401) {
        if (this.#user) {
          this.#expireSession();
          throw new AuthError(401, "session_expired", SESSION_EXPIRED_MESSAGE);
        }
        this.#transition({ user: null, anonymous: true, sessionExpired: false });
        return undefined;
      }
      if (this.#user) {
        throw new AuthError(
          response.status,
          "auth_unavailable",
          "The sign-in service is unavailable right now. Try again, or convert on this device.",
        );
      }
      this.#transition({ user: null, anonymous: true, sessionExpired: false });
      return undefined;
    }
    let data: { token?: unknown } | null = null;
    try {
      data = (await response.json()) as { token?: unknown };
    } catch {
      data = null;
    }
    if (this.#pendingSession) {
      await this.#pendingSession.catch(() => null);
    }
    if (this.#epoch !== epoch || (this.#user?.id ?? null) !== userId) {
      return this.bearer();
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
      this.#transition({ user: null, anonymous: true, sessionExpired: false });
      return undefined;
    }
    this.#remember(token, epoch, userId);
    return token;
  }
}
