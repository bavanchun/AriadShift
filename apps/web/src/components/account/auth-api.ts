// The account pages talk to the auth service through this interface. The
// implementation below wraps Better Auth's browser client and its passkey,
// two-factor and API-key plugins; tests pass their own implementation.

import { apiKeyClient } from "@better-auth/api-key/client";
import { passkeyClient } from "@better-auth/passkey/client";
import { createAuthClient } from "better-auth/client";
import { twoFactorClient } from "better-auth/client/plugins";

export interface AccountUser {
  id: string;
  name: string;
  email: string;
  twoFactorEnabled: boolean;
}

export interface PasskeyInfo {
  id: string;
  name: string | null;
  createdAt: string;
}

export interface ApiKeyInfo {
  id: string;
  name: string | null;
  /** The first characters of the key, for recognising it. */
  start: string | null;
  createdAt: string;
  expiresAt: string | null;
  enabled: boolean;
}

export interface CreatedApiKey extends ApiKeyInfo {
  /** The full key. The auth service never returns it again. */
  key: string;
}

export interface TwoFactorSetup {
  /** `otpauth://` URI for authenticator apps. */
  totpUri: string;
  backupCodes: string[];
}

export interface AuthApi {
  /** The signed-in user, or null without a session. */
  getSession(): Promise<AccountUser | null>;
  /** Signs in; `twoFactorRequired` means a TOTP or backup code must follow. */
  signInEmail(email: string, password: string): Promise<{ twoFactorRequired: boolean }>;
  signUpEmail(name: string, email: string, password: string, signupToken?: string): Promise<void>;
  signInPasskey(): Promise<void>;
  /** Finishes a two-factor sign-in, or confirms TOTP enrolment. */
  verifyTotp(code: string, trustDevice?: boolean): Promise<void>;
  verifyBackupCode(code: string): Promise<void>;
  signOut(): Promise<void>;
  updateName(name: string): Promise<void>;
  changePassword(
    currentPassword: string,
    newPassword: string,
    revokeOtherSessions: boolean,
  ): Promise<void>;
  listPasskeys(): Promise<PasskeyInfo[]>;
  addPasskey(name: string): Promise<void>;
  deletePasskey(id: string): Promise<void>;
  enableTwoFactor(password: string): Promise<TwoFactorSetup>;
  disableTwoFactor(password: string): Promise<void>;
  regenerateBackupCodes(password: string): Promise<string[]>;
  deleteAccount(password: string): Promise<void>;
  listApiKeys(): Promise<ApiKeyInfo[]>;
  /** `expiresInDays` null creates a key that does not expire. */
  createApiKey(name: string, expiresInDays: number | null): Promise<CreatedApiKey>;
  deleteApiKey(id: string): Promise<void>;
  /** A JWT for the API, reused until shortly before it expires; undefined without a session. */
  apiToken(): Promise<string | undefined>;
}

/** A refused auth request. `message` comes from the service and is safe to show. */
export class AuthError extends Error {
  override readonly name = "AuthError";

  constructor(
    message: string,
    readonly status?: number,
    readonly code?: string,
  ) {
    super(message);
  }
}

type BetterFetchError = { message?: string; status?: number; statusText?: string; code?: string };
type Result = { data?: unknown; error?: BetterFetchError | null };

/** Returns `data`, or throws the result's error as an `AuthError`. */
function unwrap(result: Result): unknown {
  const error = result.error;
  if (error) {
    const message = error.message || error.statusText || "The auth service refused the request.";
    throw new AuthError(message, error.status, error.code);
  }
  return result.data;
}

function text(value: unknown): string | null {
  return typeof value === "string" && value !== "" ? value : null;
}

/** Dates arrive as ISO strings over JSON, or as `Date` objects from the client's parser. */
function isoDate(value: unknown): string | null {
  if (value instanceof Date) {
    return value.toISOString();
  }
  return typeof value === "string" ? value : null;
}

function record(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
}

function toApiKey(value: unknown): ApiKeyInfo {
  const key = record(value);
  return {
    id: String(key.id),
    name: text(key.name),
    start: text(key.start),
    createdAt: isoDate(key.createdAt) ?? "",
    expiresAt: isoDate(key.expiresAt),
    enabled: key.enabled !== false,
  };
}

/** Lifetime of the API's JWTs is 15 minutes; refresh one minute early. */
const TOKEN_REUSE_MS = 14 * 60 * 1000;

const DAY_SECONDS = 24 * 60 * 60;

/** Creates the Better Auth implementation for one auth service origin. */
export function createBetterAuthApi(authUrl: string, fetchImpl?: typeof fetch): AuthApi {
  const client = createAuthClient({
    baseURL: `${authUrl}/api/auth`,
    fetchOptions: {
      // Session cookies belong to the auth origin; the web origin is same-site.
      credentials: "include",
      ...(fetchImpl ? { customFetchImpl: fetchImpl } : {}),
    },
    plugins: [twoFactorClient(), passkeyClient(), apiKeyClient()],
  });

  // Resolve the global lazily so it is called with the right `this`.
  const fetchToken: typeof fetch = fetchImpl ?? ((input, init) => globalThis.fetch(input, init));
  let cachedToken: { token: string; until: number } | undefined;
  const call = (pending: Promise<unknown>) => pending.then((result) => unwrap(result as Result));

  return {
    async getSession() {
      const data = record(await call(client.getSession()));
      const user = record(data.user);
      if (typeof user.id !== "string") {
        return null;
      }
      return {
        id: user.id,
        name: typeof user.name === "string" ? user.name : "",
        email: typeof user.email === "string" ? user.email : "",
        twoFactorEnabled: user.twoFactorEnabled === true,
      };
    },

    async signInEmail(email, password) {
      const data = record(await call(client.signIn.email({ email, password })));
      return { twoFactorRequired: data.twoFactorRedirect === true };
    },

    async signUpEmail(name, email, password, signupToken) {
      await call(
        client.signUp.email(
          { name, email, password },
          {
            ...(signupToken ? { headers: { "x-captcha-response": signupToken } } : {}),
          },
        ),
      );
    },

    async signInPasskey() {
      await call(client.signIn.passkey());
    },

    async verifyTotp(code, trustDevice = false) {
      await call(client.twoFactor.verifyTotp({ code, trustDevice }));
    },

    async verifyBackupCode(code) {
      await call(client.twoFactor.verifyBackupCode({ code }));
    },

    async signOut() {
      cachedToken = undefined;
      await call(client.signOut());
    },

    async updateName(name) {
      await call(client.updateUser({ name }));
    },

    async changePassword(currentPassword, newPassword, revokeOtherSessions) {
      await call(client.changePassword({ currentPassword, newPassword, revokeOtherSessions }));
    },

    async listPasskeys() {
      const data = await call(client.passkey.listUserPasskeys());
      return (Array.isArray(data) ? data : []).map((value) => {
        const passkey = record(value);
        return {
          id: String(passkey.id),
          name: text(passkey.name),
          createdAt: isoDate(passkey.createdAt) ?? "",
        };
      });
    },

    async addPasskey(name) {
      await call(client.passkey.addPasskey({ name }));
    },

    async deletePasskey(id) {
      await call(client.passkey.deletePasskey({ id }));
    },

    async enableTwoFactor(password) {
      const data = record(await call(client.twoFactor.enable({ password })));
      return {
        totpUri: String(data.totpURI ?? ""),
        backupCodes: Array.isArray(data.backupCodes) ? data.backupCodes.map(String) : [],
      };
    },

    async disableTwoFactor(password) {
      await call(client.twoFactor.disable({ password }));
    },

    async regenerateBackupCodes(password) {
      const data = record(await call(client.twoFactor.generateBackupCodes({ password })));
      return Array.isArray(data.backupCodes) ? data.backupCodes.map(String) : [];
    },

    async deleteAccount(password) {
      try {
        await call(client.deleteUser({ password }));
      } catch (error) {
        // Better Auth serves the endpoint only when the service enables user deletion.
        if (error instanceof AuthError && error.status === 404) {
          throw new AuthError(
            "This server does not allow deleting accounts from the web app. Ask its operator to delete it.",
            404,
          );
        }
        throw error;
      }
      cachedToken = undefined;
    },

    async listApiKeys() {
      const data = await call(client.apiKey.list());
      // Better Auth 1.7 returns `{ apiKeys, total }`; earlier releases returned the array.
      const keys = Array.isArray(data) ? data : record(data).apiKeys;
      return (Array.isArray(keys) ? keys : []).map(toApiKey);
    },

    async createApiKey(name, expiresInDays) {
      const data = record(
        await call(
          client.apiKey.create({
            name,
            ...(expiresInDays === null ? {} : { expiresIn: expiresInDays * DAY_SECONDS }),
          }),
        ),
      );
      return { ...toApiKey(data), key: String(data.key ?? "") };
    },

    async deleteApiKey(id) {
      await call(client.apiKey.delete({ keyId: id }));
    },

    async apiToken() {
      if (cachedToken && Date.now() < cachedToken.until) {
        return cachedToken.token;
      }
      const response = await fetchToken(`${authUrl}/api/auth/token`, {
        credentials: "include",
      });
      if (!response.ok) {
        cachedToken = undefined;
        let body: Record<string, unknown> = {};
        try {
          body = record(await response.json());
        } catch {
          // Non-JSON error bodies still surface a typed AuthError below.
        }
        const message =
          typeof body.message === "string" && body.message !== ""
            ? body.message
            : "Your session has ended. Sign in again.";
        const code =
          typeof body.code === "string" && body.code !== ""
            ? body.code
            : response.status === 401
              ? "unauthorized"
              : "token_refresh_failed";
        throw new AuthError(message, response.status, code);
      }
      let data: unknown;
      try {
        data = await response.json();
      } catch {
        cachedToken = undefined;
        throw new AuthError(
          "The sign-in service returned an invalid API token.",
          response.status,
          "invalid_token_response",
        );
      }
      const token = record(data).token;
      if (typeof token !== "string" || token === "") {
        cachedToken = undefined;
        throw new AuthError(
          "The sign-in service returned an invalid API token.",
          response.status,
          "invalid_token_response",
        );
      }
      cachedToken = { token, until: Date.now() + TOKEN_REUSE_MS };
      return token;
    },
  };
}
