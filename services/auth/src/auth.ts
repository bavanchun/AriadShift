// Better Auth setup. ariad-server depends on four details here: the JWT
// claims (iss/aud = this origin, sub = user id, EdDSA keys at
// /api/auth/jwks), the OAuth access tokens for remote MCP (same iss, keys
// and sub, aud = the MCP resource), the apikey table it reads (default
// config id, user references, SHA-256 hashing, rate limit off) and the
// `auth` schema.

import { apiKey } from "@better-auth/api-key";
import { oauthProvider } from "@better-auth/oauth-provider";
import { passkey } from "@better-auth/passkey";
import { type BetterAuthOptions, type BetterAuthPlugin, betterAuth } from "better-auth";
import { APIError, createAuthMiddleware, getSessionFromCtx } from "better-auth/api";
import { jwt, twoFactor } from "better-auth/plugins";
import pg from "pg";
import type { DiscoveryDocument } from "./app.ts";
import { type CimdFetcherOptions, createCimdExtension } from "./cimd.ts";
import { AUTH_SCHEMA, type AuthServiceConfig } from "./config.ts";
import { OAUTH_TOKEN_LIMITS, serializedOAuthHandler } from "./oauth-lock.ts";

export const APP_NAME = "AriadShift";

/** Where Better Auth is mounted; ariad-server derives the JWKS URL from it. */
export const BASE_PATH = "/api/auth";

/** Lifetime of the JWTs the API accepts. */
export const JWT_LIFETIME = "15m";

/** Maximum session age (seconds) accepted by `/delete-user` before re-authentication is required. */
export const DELETE_USER_FRESH_AGE_S = 15 * 60;

/** JWT plugin options are seconds, including the post-rotation publication grace. */
export const JWKS_ROTATION_INTERVAL_S = 30 * 24 * 60 * 60;
export const JWKS_GRACE_PERIOD_S = 30 * 60;

/** Shared with the signup widget; upload proofs cannot create accounts. */
export const SIGNUP_CHALLENGE_ACTION = "signup";
const SITEVERIFY_URL = "https://challenges.cloudflare.com/turnstile/v0/siteverify";
const SITEVERIFY_TIMEOUT_MS = 5_000;

/**
 * Run before Better Auth parses or writes an account, including direct POSTs.
 * The HTTP layer supplies the peer header and caps the signup request body.
 * Keep verifier errors and payloads out of logs and user-visible errors.
 */
function signupChallenge(config: AuthServiceConfig): BetterAuthPlugin["onRequest"] {
  if (!config.turnstileSecret) return undefined;
  const secret = config.turnstileSecret;
  const hostnames = new Set(config.trustedOrigins.map((origin) => new URL(origin).hostname));
  return async (request) => {
    const path = new URL(request.url).pathname.replace(/\/{2,}/g, "/").replace(/\/+$/, "");
    if (request.method !== "POST" || path !== `${BASE_PATH}/sign-up/email`) return;
    const refusal = (status: number, code: string, message: string) => ({
      response: Response.json({ code, message }, { status }),
    });
    const proof = request.headers.get("x-captcha-response")?.trim();
    if (!proof || proof.length > 2048) {
      return refusal(
        400,
        "MISSING_CAPTCHA_RESPONSE",
        "Complete the signup bot check and try again.",
      );
    }
    try {
      const response = await fetch(SITEVERIFY_URL, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          secret,
          response: proof,
          ...(request.headers.get(CLIENT_IP_HEADER)
            ? { remoteip: request.headers.get(CLIENT_IP_HEADER) }
            : {}),
        }),
        signal: AbortSignal.timeout(SITEVERIFY_TIMEOUT_MS),
        redirect: "error",
      });
      if (!response.ok || !response.body) {
        await response.body?.cancel();
        throw new Error("verifier unavailable");
      }
      const reader = response.body.getReader();
      const bytes = new Uint8Array(4096);
      let size = 0;
      try {
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          if (value.byteLength > bytes.length - size) {
            await reader.cancel();
            throw new Error("verifier response too large");
          }
          bytes.set(value, size);
          size += value.byteLength;
        }
      } finally {
        reader.releaseLock();
      }
      const result: unknown = JSON.parse(new TextDecoder().decode(bytes.subarray(0, size)));
      if (
        typeof result !== "object" ||
        result === null ||
        !("success" in result) ||
        result.success !== true ||
        !("action" in result) ||
        result.action !== SIGNUP_CHALLENGE_ACTION ||
        !("hostname" in result) ||
        typeof result.hostname !== "string" ||
        !hostnames.has(result.hostname)
      ) {
        return refusal(
          403,
          "CAPTCHA_VERIFICATION_FAILED",
          "The signup bot check was not accepted. Try a new check.",
        );
      }
    } catch {
      return refusal(
        503,
        "CAPTCHA_UNAVAILABLE",
        "The signup bot check is unavailable. Please try again.",
      );
    }
  };
}

/** Prefix of API keys, so leaked keys are easy to recognise. */
export const API_KEY_PREFIX = "ashift_";

/** Page where an OAuth authorization signs the user in (served by app.ts). */
export const OAUTH_SIGN_IN_PATH = "/oauth/sign-in";

/** Page where the user approves or denies an OAuth client. */
export const OAUTH_CONSENT_PATH = "/oauth/consent";

/**
 * Scopes an OAuth client may request. ariad-server needs only the user id
 * (`sub`); the remote MCP tools apply the same workspace entitlements as the
 * REST API, so no product scopes exist. `offline_access` yields refresh
 * tokens, so clients need not repeat the browser sign-in every 15 minutes.
 */
export const OAUTH_SCOPES = ["openid", "profile", "email", "offline_access"];

/**
 * Grants the token endpoint accepts. `client_credentials` is left out on
 * purpose: its tokens carry the client id as `sub`, which ariad-server would
 * treat as a user id, so any self-registered client could mint a workspace.
 */
export const OAUTH_GRANT_TYPES = ["authorization_code", "refresh_token"] as const;

/** Lifetime of OAuth access tokens, in seconds; the same as the API JWT. */
export const OAUTH_ACCESS_TOKEN_LIFETIME_S = 15 * 60;

/**
 * Seconds during which a rotated refresh token may be replayed for the same
 * response, so a client that retries a refresh after a lost reply is not
 * locked out. Better Auth's MCP profile uses the same window.
 */
export const OAUTH_REFRESH_REUSE_INTERVAL_S = 30;

/**
 * Per-address limit on dynamic client registration. Registration needs no
 * account and stores a row each time, so it is limited far more tightly than
 * Better Auth's default of 5 per minute; an MCP client registers once per
 * installation and reuses its client id.
 */
export const OAUTH_REGISTRATION_RATE_LIMIT = { window: 60 * 60, max: 20 } as const;

/**
 * Header that carries the client address to Better Auth's rate limiter. The
 * HTTP layer always overwrites it, so a client cannot choose its own bucket.
 */
export const CLIENT_IP_HEADER = "x-ariadshift-client-ip";

/** Opens the pool with every unqualified table name resolving to `auth`. */
export function createPool(config: AuthServiceConfig): pg.Pool {
  return new pg.Pool({
    connectionString: config.databaseUrl,
    // A search_path in DATABASE_URL takes precedence over this one.
    options: `-c search_path=${AUTH_SCHEMA}`,
    max: 10,
    connectionTimeoutMillis: OAUTH_TOKEN_LIMITS.checkoutMs,
  });
}

/**
 * Keys created before rotation was configured have no retirement date.
 * Schedule those once, retaining a full grace window even for old keys that
 * signed a token immediately before this deployment. No key material changes.
 */
export async function scheduleLegacySigningKeys(pool: pg.Pool): Promise<void> {
  await pool.query(
    'UPDATE auth.jwks SET "expiresAt" = GREATEST("createdAt" + $1 * interval \'1 second\', now()) WHERE "expiresAt" IS NULL',
    [JWKS_ROTATION_INTERVAL_S],
  );
}

function socialProviders(config: AuthServiceConfig): BetterAuthOptions["socialProviders"] {
  return {
    ...(config.github ? { github: { ...config.github, disableSignUp: !config.allowSignUp } } : {}),
    ...(config.google ? { google: { ...config.google, disableSignUp: !config.allowSignUp } } : {}),
  };
}

/**
 * The service passes its PostgreSQL pool; in-process tests pass Better
 * Auth's memory adapter.
 */
export function buildAuthOptions(
  config: AuthServiceConfig,
  database: NonNullable<BetterAuthOptions["database"]>,
  cimd?: CimdFetcherOptions,
) {
  return {
    appName: APP_NAME,
    baseURL: config.baseUrl,
    basePath: BASE_PATH,
    secret: config.secret,
    // Tables live in the schema named by the pool's search_path. Better Auth's
    // `database.schemaName` is not used: its migration issues CREATE SCHEMA,
    // which needs a database privilege the auth login does not hold.
    database,
    trustedOrigins: config.trustedOrigins,
    emailAndPassword: {
      enabled: true,
      disableSignUp: !config.allowSignUp,
      // Self-hosted stacks have no mail transport, so addresses are not verified.
      requireEmailVerification: false,
    },
    socialProviders: socialProviders(config),
    account: {
      // Password addresses are unverified, and no page offers explicit linking.
      // Keep social identities bound to their existing accounts.
      accountLinking: { enabled: false },
    },
    user: {
      // Password-backed accounts confirm their password; social-only accounts
      // confirm deletion from a fresh session.
      deleteUser: {
        enabled: true,
        // API keys use a polymorphic referenceId without a user foreign key.
        // Revoke them before deletion; a failed cleanup must stop deletion.
        beforeDelete: async (user) => {
          if (!("query" in database) || typeof database.query !== "function") {
            throw new Error("API key revocation requires a PostgreSQL connection");
          }
          await database.query('DELETE FROM "auth"."apikey" WHERE "referenceId" = $1', [user.id]);
        },
      },
    },
    hooks: {
      before: createAuthMiddleware(async (ctx) => {
        if (ctx.path === "/delete-user") {
          const session = await getSessionFromCtx(ctx);
          if (!session) return;
          const credential = await ctx.context.internalAdapter.findCredentialAccount(
            session.user.id,
          );
          if (credential?.password) {
            const password = (ctx.body as { password?: unknown } | undefined)?.password;
            if (typeof password !== "string" || password === "") {
              throw new APIError("BAD_REQUEST", {
                code: "PASSWORD_REQUIRED",
                message: "Password is required to delete this account.",
              });
            }
          }
          const createdAt =
            session.session.createdAt instanceof Date
              ? session.session.createdAt.getTime()
              : Date.parse(String(session.session.createdAt));
          if (
            !Number.isFinite(createdAt) ||
            Date.now() - createdAt > DELETE_USER_FRESH_AGE_S * 1_000
          ) {
            throw new APIError("BAD_REQUEST", {
              code: "reauth_required",
              message: "Sign in again before deleting your account.",
            });
          }
          return;
        }
        if (ctx.path === "/oauth2/delete-consent") {
          const session = await getSessionFromCtx(ctx);
          const id = (ctx.body as { id?: unknown } | undefined)?.id;
          if (!session || typeof id !== "string" || id === "") return;
          const consent = await ctx.context.adapter.findOne<{
            id: string;
            clientId: string;
            userId?: string;
          }>({
            model: "oauthConsent",
            where: [{ field: "id", value: id }],
          });
          if (!consent || consent.userId !== session.user.id) return;
          const where = [
            { field: "clientId", value: consent.clientId },
            { field: "userId", value: session.user.id },
          ];
          await ctx.context.adapter.deleteMany({ model: "oauthAccessToken", where });
          await ctx.context.adapter.deleteMany({ model: "oauthRefreshToken", where });
          return;
        }
        if (ctx.path === "/oauth2/delete-client") {
          // Ownership matches deleteClientEndpoint when neither clientPrivileges nor
          // trusted clients are configured. Update this check if either is enabled.
          const session = await getSessionFromCtx(ctx);
          const clientId = (ctx.body as { client_id?: unknown } | undefined)?.client_id;
          if (!session || typeof clientId !== "string" || clientId === "") return;
          const client = await ctx.context.adapter.findOne<{
            clientId: string;
            userId?: string;
          }>({
            model: "oauthClient",
            where: [{ field: "clientId", value: clientId }],
          });
          if (!client || client.userId !== session.user.id) return;
          const where = [{ field: "clientId", value: client.clientId }];
          await ctx.context.adapter.deleteMany({ model: "oauthAccessToken", where });
          await ctx.context.adapter.deleteMany({ model: "oauthRefreshToken", where });
          await ctx.context.adapter.deleteMany({ model: "oauthConsent", where });
          await ctx.context.adapter.deleteMany({ model: "oauthClientResource", where });
        }
      }),
      after: createAuthMiddleware(async (ctx) => {
        if (ctx.path !== "/oauth2/get-consents") return;
        const returned = ctx.context.returned;
        if (!Array.isArray(returned) || returned.length === 0) return;
        const clientIds = [
          ...new Set(
            returned
              .map((row) => (row as { clientId?: unknown })?.clientId)
              .filter((id): id is string => typeof id === "string" && id !== ""),
          ),
        ];
        if (clientIds.length === 0) return;
        const clients = await ctx.context.adapter.findMany<{
          clientId: string;
          name?: string | null;
          uri?: string | null;
        }>({
          model: "oauthClient",
          where: [{ field: "clientId", operator: "in", value: clientIds }],
        });
        const byId = new Map(clients.map((client) => [client.clientId, client]));
        return ctx.json(
          returned.map((row) => {
            const consent = row as Record<string, unknown> & { clientId?: string };
            const client = consent.clientId ? byId.get(consent.clientId) : undefined;
            return {
              ...consent,
              clientName: client?.name ?? null,
              clientUri: client?.uri ?? null,
            };
          }),
        );
      }),
    },
    rateLimit: {
      enabled: true as boolean,
      // Shared across replicas and kept across restarts.
      storage: "database",
      window: 60,
      max: 100,
    },
    advanced: {
      cookiePrefix: "ariadshift",
      // Session cookies stay host-only on this service. The web app calls it
      // with `credentials: "include"`, which needs the two origins to be
      // same-site (ports and subdomains of one site are fine).
      useSecureCookies: config.baseUrl.startsWith("https://"),
      defaultCookieAttributes: { sameSite: "lax", httpOnly: true },
      ipAddress: { ipAddressHeaders: [CLIENT_IP_HEADER] },
    },
    telemetry: { enabled: false },
    plugins: [
      jwt({
        jwks: {
          keyPairConfig: { alg: "EdDSA", crv: "Ed25519" },
          rotationInterval: JWKS_ROTATION_INTERVAL_S,
          gracePeriod: JWKS_GRACE_PERIOD_S,
        },
        jwt: {
          issuer: config.baseUrl,
          audience: config.baseUrl,
          expirationTime: JWT_LIFETIME,
          // The API needs only `sub`; keep profile data out of bearer tokens.
          definePayload: () => ({}),
        },
      }),
      apiKey({
        defaultPrefix: API_KEY_PREFIX,
        references: "user",
        // ariad-server reads the key table without writing it, so Better
        // Auth's counters would never move; the API rate-limits per workspace.
        rateLimit: { enabled: false },
        enableSessionForAPIKeys: false,
      }),
      passkey({
        rpID: config.passkeyRpId,
        rpName: APP_NAME,
        // The OAuth sign-in page is served by this service, so passkey
        // assertions made there carry this service's origin.
        origin: [...new Set([...config.trustedOrigins, config.baseUrl])],
      }),
      twoFactor({ issuer: APP_NAME }),
      ...(config.oauthDisabledReason
        ? []
        : [
            oauthProvider({
              // Absolute, so redirects and JSON `url` answers name this service
              // even when a client resolves them against another base.
              loginPage: `${config.baseUrl}${OAUTH_SIGN_IN_PATH}`,
              consentPage: `${config.baseUrl}${OAUTH_CONSENT_PATH}`,
              scopes: OAUTH_SCOPES,
              grantTypes: [...OAUTH_GRANT_TYPES],
              // RFC 8707: the only resource tokens can be issued for. A request
              // naming it gets a JWT access token with `aud` = the resource; any
              // other resource is refused with invalid_target.
              resources: [config.mcpResource],
              // RFC 7591 registration without an account, as MCP clients register
              // before the user signs in. Each new client is linked to the MCP
              // resource, and PKCE stays mandatory for every registered client.
              allowDynamicClientRegistration: true,
              allowUnauthenticatedClientRegistration: true,
              clientRegistrationDefaultResources: [config.mcpResource],
              clientRegistrationRequirePKCE: true,
              // Native clients (Claude Code, Cursor) often omit application_type
              // and use loopback or custom-scheme callbacks.
              clientRegistrationDefaultApplicationType: "infer",
              // Client ID Metadata Documents (draft-ietf-oauth-client-id-metadata-document)
              // let MCP clients identify themselves by an https:// metadata URL
              // without prior dynamic registration; RFC 7591 stays enabled alongside it.
              extensions: config.cimdEnabled
                ? [
                    createCimdExtension({
                      ...cimd,
                      allowedHosts: config.cimdAllowedHosts,
                      deniedHosts: config.cimdDeniedHosts,
                      registrationRateLimit: OAUTH_REGISTRATION_RATE_LIMIT,
                    }),
                  ]
                : [],
              accessTokenExpiresIn: OAUTH_ACCESS_TOKEN_LIFETIME_S,
              refreshTokenReuseInterval: OAUTH_REFRESH_REUSE_INTERVAL_S,
              // Keyed by CLIENT_IP_HEADER like every other Better Auth limit; the
              // HTTP layer also caps the registration body size.
              rateLimit: { register: { ...OAUTH_REGISTRATION_RATE_LIMIT } },
            }),
          ]),
      ...(config.turnstileSecret
        ? [{ id: "signup-challenge", onRequest: signupChallenge(config) }]
        : []),
    ],
  } satisfies BetterAuthOptions;
}

export function createAuth(options: ReturnType<typeof buildAuthOptions>) {
  const auth = betterAuth(options);
  if (!(options.database instanceof pg.Pool)) return auth;
  const handler = serializedOAuthHandler(
    options.database,
    auth.handler,
    (database) => betterAuth({ ...options, database }).handler,
  );
  return {
    ...auth,
    handler,
    fetch: handler,
  };
}

export type Auth = ReturnType<typeof createAuth>;

/** Reads Better Auth's RFC 8414 or OpenID discovery document for a request. */
export function discoveryDocuments(auth: Auth) {
  return (request: Request, document: DiscoveryDocument): Promise<object> =>
    document === "openid-configuration"
      ? auth.api.getOpenIdConfig({ request, asResponse: false })
      : auth.api.getOAuthServerConfig({ request, asResponse: false });
}
