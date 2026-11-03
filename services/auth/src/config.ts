// Environment contract of the auth service. Parsing is pure so tests cover
// every rule without a database or a running server.

/** Postgres schema that holds every Better Auth table. */
export const AUTH_SCHEMA = "auth";

/** Shortest accepted BETTER_AUTH_SECRET, in characters. */
export const MIN_SECRET_LENGTH = 32;

/** Path of the remote MCP endpoint on the API's public origin. */
export const MCP_PATH = "/mcp";

export interface OAuthClient {
  clientId: string;
  clientSecret: string;
}

export interface AuthServiceConfig {
  databaseUrl: string;
  secret: string;
  /** Public origin of this service, e.g. `https://auth.example.com`. */
  baseUrl: string;
  /**
   * The remote MCP resource, `<API public origin>/mcp`. OAuth access tokens
   * carry it as `aud`, exactly as ariad-server derives it from the same
   * ARIAD_PUBLIC_URL.
   */
  mcpResource: string;
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
  /** Explicit operator permission to create password or social accounts. */
  allowSignUp: boolean;
  /** Explicit permission for non-loopback HTTP without remote MCP OAuth. */
  allowInsecureHttp: boolean;
  cimdEnabled: boolean;
  cimdAllowedHosts: string[];
  cimdDeniedHosts: string[];
  /** Why remote MCP OAuth is disabled, or undefined when it is available. */
  oauthDisabledReason: string | undefined;
  /** Optional signup challenge; absent for self-contained local deployments. */
  turnstileSecret?: string;
  /** Public Turnstile site key paired with turnstileSecret. */
  turnstileSiteKey?: string;
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

function secretOption(env: Env, name: string): string | undefined {
  const value = read(env, name);
  if (value?.startsWith("replace-with")) {
    throw new ConfigError(`${name} must be a generated secret, not a template placeholder`);
  }
  return value;
}

/** Only the literal boolean values are accepted; blank values are unset. */
function booleanOption(env: Env, name: string): boolean {
  const value = read(env, name);
  if (value === undefined || value === "false") return false;
  if (value === "true") return true;
  throw new ConfigError(`${name} must be true or false`);
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

/** `localhost`, `*.localhost`, 127.0.0.0/8 or `[::1]`. */
function isLoopbackHost(hostname: string): boolean {
  return (
    hostname === "localhost" ||
    hostname.endsWith(".localhost") ||
    hostname === "[::1]" ||
    /^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(hostname)
  );
}

/**
 * OAuth 2.1 and MCP require TLS for the issuer and the protected resource,
 * except on loopback hosts during development. Better Auth also rewrites a
 * plain-HTTP non-loopback issuer to https in its discovery document, which
 * would no longer match the `iss` of the tokens it signs.
 */
function tlsProblem(name: string, origin: string): string | undefined {
  const url = new URL(origin);
  return url.protocol !== "https:" && !isLoopbackHost(url.hostname)
    ? `${name} must use https unless it is a loopback host: ${origin}`
    : undefined;
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
  const clientSecret = secretOption(env, `${provider}_CLIENT_SECRET`);
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

function parseCimdHosts(env: Env, name: string): string[] {
  return [
    ...new Set(
      (read(env, name) ?? "")
        .split(",")
        .map((host) => host.trim())
        .filter(Boolean)
        .map((host) => {
          let url: URL;
          try {
            url = new URL(`https://${host}/`);
          } catch {
            throw new ConfigError(`${name} must contain canonical hostnames only`);
          }
          if (
            url.hostname !== host ||
            url.port ||
            url.username ||
            url.password ||
            url.pathname !== "/"
          ) {
            throw new ConfigError(`${name} must contain canonical hostnames only`);
          }
          return host;
        }),
    ),
  ];
}

export function parseConfig(env: Env): AuthServiceConfig {
  const secret = secretOption(env, "BETTER_AUTH_SECRET") ?? required(env, "BETTER_AUTH_SECRET");
  if (secret.length < MIN_SECRET_LENGTH) {
    throw new ConfigError(`BETTER_AUTH_SECRET must be at least ${MIN_SECRET_LENGTH} characters`);
  }
  const allowSignUp = booleanOption(env, "AUTH_ALLOW_SIGN_UP");
  const allowInsecureHttp = booleanOption(env, "AUTH_ALLOW_INSECURE_HTTP");
  const baseUrl = parseOrigin("BETTER_AUTH_URL", required(env, "BETTER_AUTH_URL"));
  // The same variable gives ariad-server its public URL, so both services
  // agree on the MCP resource without a second setting.
  const apiOrigin = parseOrigin("ARIAD_PUBLIC_URL", required(env, "ARIAD_PUBLIC_URL"));
  const oauthDisabledReason =
    tlsProblem("BETTER_AUTH_URL", baseUrl) ?? tlsProblem("ARIAD_PUBLIC_URL", apiOrigin);
  if (oauthDisabledReason && !allowInsecureHttp) {
    throw new ConfigError(
      `${oauthDisabledReason}; set AUTH_ALLOW_INSECURE_HTTP=true to allow accounts without remote MCP OAuth`,
    );
  }

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
  const turnstileSecret = secretOption(env, "AUTH_TURNSTILE_SECRET");
  const turnstileSiteKey = read(env, "AUTH_TURNSTILE_SITE_KEY");
  if (allowSignUp && turnstileSecret && !turnstileSiteKey) {
    throw new ConfigError(
      "AUTH_TURNSTILE_SITE_KEY is required when signup is enabled and AUTH_TURNSTILE_SECRET is set",
    );
  }

  return {
    databaseUrl: parseDatabaseUrl(required(env, "DATABASE_URL")),
    secret,
    baseUrl,
    mcpResource: `${apiOrigin}${MCP_PATH}`,
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
    allowSignUp,
    allowInsecureHttp,
    cimdEnabled: booleanOption(env, "AUTH_CIMD_ENABLED"),
    cimdAllowedHosts: parseCimdHosts(env, "AUTH_CIMD_ALLOWED_HOSTS"),
    cimdDeniedHosts: parseCimdHosts(env, "AUTH_CIMD_DENIED_HOSTS"),
    oauthDisabledReason,
    ...(turnstileSecret ? { turnstileSecret } : {}),
    ...(turnstileSiteKey ? { turnstileSiteKey } : {}),
  };
}
