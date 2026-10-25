// Better Auth setup. ariad-server depends on three details here: the JWT
// claims (iss/aud = this origin, sub = user id, EdDSA keys at
// /api/auth/jwks), the apikey table it reads (default config id, user
// references, SHA-256 hashing, rate limit off) and the `auth` schema.

import { apiKey } from "@better-auth/api-key";
import { passkey } from "@better-auth/passkey";
import { type BetterAuthOptions, betterAuth } from "better-auth";
import { jwt, twoFactor } from "better-auth/plugins";
import pg from "pg";
import { AUTH_SCHEMA, type AuthServiceConfig } from "./config.ts";

export const APP_NAME = "AriadShift";

/** Where Better Auth is mounted; ariad-server derives the JWKS URL from it. */
export const BASE_PATH = "/api/auth";

/** Lifetime of the JWTs the API accepts. */
export const JWT_LIFETIME = "15m";

/** Prefix of API keys, so leaked keys are easy to recognise. */
export const API_KEY_PREFIX = "ashift_";

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
  });
}

function socialProviders(config: AuthServiceConfig): BetterAuthOptions["socialProviders"] {
  return {
    ...(config.github ? { github: config.github } : {}),
    ...(config.google ? { google: config.google } : {}),
  };
}

export function buildAuthOptions(config: AuthServiceConfig, database: pg.Pool) {
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
      // Self-hosted stacks have no mail transport, so addresses are not verified.
      requireEmailVerification: false,
    },
    socialProviders: socialProviders(config),
    rateLimit: {
      enabled: true,
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
        jwks: { keyPairConfig: { alg: "EdDSA", crv: "Ed25519" } },
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
        origin: config.trustedOrigins,
      }),
      twoFactor({ issuer: APP_NAME }),
    ],
  } satisfies BetterAuthOptions;
}

export function createAuth(options: ReturnType<typeof buildAuthOptions>) {
  return betterAuth(options);
}

export type Auth = ReturnType<typeof createAuth>;
