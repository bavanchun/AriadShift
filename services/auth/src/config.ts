// Environment contract of the auth service. Parsing is pure so tests cover
// every rule without a database or a running server.

/** Postgres schema that holds every Better Auth table. */
export const AUTH_SCHEMA = "auth";

/** Shortest accepted BETTER_AUTH_SECRET, in characters. */
export const MIN_SECRET_LENGTH = 32;

export interface OAuthClient {
  clientId: string;
  clientSecret: string;
}

export interface AuthServiceConfig {
  databaseUrl: string;
  secret: string;
  /** Public origin of this service, e.g. `https://auth.example.com`. */
  baseUrl: string;
  /** Browser origins allowed to call the service with credentials. */
  trustedOrigins: string[];
  port: number;
  host: string;
  /** Header set by a trusted proxy that carries the client address. */
  trustedClientIpHeader: string | undefined;
  /** WebAuthn relying-party id: the web app's host or a parent domain. */
  passkeyRpId: string;
  github: OAuthClient | undefined;
  google: OAuthClient | undefined;
}

export class ConfigError extends Error {
  override name = "ConfigError";
}

type Env = Record<string, string | undefined>;

/** Reads a variable, treating an empty or blank value as unset. */
function read(env: Env, name: string): string | undefined {
  const value = env[name]?.trim();
  return value ? value : undefined;
}

function required(env: Env, name: string): string {
  const value = read(env, name);
  if (value === undefined) throw new ConfigError(`${name} is required`);
  return value;
}

/** Parses an http(s) URL that must be a bare origin (no path, query or hash). */
function parseOrigin(name: string, raw: string): string {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw new ConfigError(`${name} is not a URL: ${raw}`);
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new ConfigError(`${name} must use http or https: ${raw}`);
  }
  if (url.username || url.password || url.search || url.hash || url.pathname !== "/") {
    throw new ConfigError(`${name} must be an origin without a path: ${raw}`);
  }
  return url.origin;
}

function parseDatabaseUrl(raw: string): string {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw new ConfigError("DATABASE_URL is not a URL");
  }
  if (url.protocol !== "postgres:" && url.protocol !== "postgresql:") {
    throw new ConfigError("DATABASE_URL must be a postgres:// or postgresql:// URL");
  }
  return raw;
}

function parsePort(raw: string | undefined): number {
  if (raw === undefined) return 3000;
  const port = Number(raw);
  if (!/^\d+$/.test(raw) || !Number.isInteger(port) || port < 1 || port > 65535) {
    throw new ConfigError(`PORT must be an integer from 1 to 65535: ${raw}`);
  }
  return port;
}

/** Both halves of an OAuth client, or neither. */
function parseOAuth(env: Env, provider: "GITHUB" | "GOOGLE"): OAuthClient | undefined {
  const clientId = read(env, `${provider}_CLIENT_ID`);
  const clientSecret = read(env, `${provider}_CLIENT_SECRET`);
  if (clientId === undefined && clientSecret === undefined) return undefined;
  if (clientId === undefined || clientSecret === undefined) {
    throw new ConfigError(
      `${provider}_CLIENT_ID and ${provider}_CLIENT_SECRET must be set together`,
    );
  }
  return { clientId, clientSecret };
}

/** A lowercase HTTP header name, as Better Auth and Hono compare them. */
function parseHeaderName(name: string, raw: string | undefined): string | undefined {
  if (raw === undefined) return undefined;
  if (!/^[A-Za-z0-9-]+$/.test(raw)) throw new ConfigError(`${name} is not a header name: ${raw}`);
  return raw.toLowerCase();
}

export function parseConfig(env: Env): AuthServiceConfig {
  const secret = required(env, "BETTER_AUTH_SECRET");
  if (secret.length < MIN_SECRET_LENGTH) {
    throw new ConfigError(`BETTER_AUTH_SECRET must be at least ${MIN_SECRET_LENGTH} characters`);
  }
  const baseUrl = parseOrigin("BETTER_AUTH_URL", required(env, "BETTER_AUTH_URL"));

  const trustedOrigins = [
    ...new Set(
      (read(env, "BETTER_AUTH_TRUSTED_ORIGINS") ?? "")
        .split(",")
        .map((origin) => origin.trim())
        .filter(Boolean)
        .map((origin) => parseOrigin("BETTER_AUTH_TRUSTED_ORIGINS", origin)),
    ),
  ];
  if (trustedOrigins.length === 0) {
    throw new ConfigError("BETTER_AUTH_TRUSTED_ORIGINS must list the web app origin");
  }

  // Passkeys are created on the web app's page, so the relying party is the
  // web app's host unless an operator names a shared parent domain.
  const firstOrigin = trustedOrigins[0] as string;
  const passkeyRpId = read(env, "AUTH_PASSKEY_RP_ID") ?? new URL(firstOrigin).hostname;

  return {
    databaseUrl: parseDatabaseUrl(required(env, "DATABASE_URL")),
    secret,
    baseUrl,
    trustedOrigins,
    port: parsePort(read(env, "PORT")),
    host: read(env, "HOST") ?? "0.0.0.0",
    trustedClientIpHeader: parseHeaderName(
      "AUTH_TRUSTED_CLIENT_IP_HEADER",
      read(env, "AUTH_TRUSTED_CLIENT_IP_HEADER"),
    ),
    passkeyRpId,
    github: parseOAuth(env, "GITHUB"),
    google: parseOAuth(env, "GOOGLE"),
  };
}
