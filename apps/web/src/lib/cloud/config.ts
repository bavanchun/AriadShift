// Build-time configuration of cloud mode. Next.js inlines `NEXT_PUBLIC_*`
// variables only when they are read by their literal names, so they are read
// here once. Without an API base URL the site has no cloud mode at all: the
// converter stays on this device and never offers an upload.

export interface CloudConfig {
  /** The API origin (plus any path prefix), without a trailing slash. */
  apiUrl: string;
  /** The API host as people see it, for example `api.example.com`. */
  apiHost: string;
  /** The auth service origin; `null` offers anonymous cloud use only. */
  authUrl: string | null;
  /** Cloudflare Turnstile site key for anonymous uploads, when the deployment requires it. */
  turnstileSiteKey: string | null;
  /** Who runs the server, when the deployment names itself. */
  operator: string | null;
}

export interface CloudEnvironment {
  api?: string;
  auth?: string;
  turnstileSiteKey?: string;
  operator?: string;
}

function baseUrl(value: string | undefined): URL | null {
  const trimmed = value?.trim();
  if (!trimmed) {
    return null;
  }
  try {
    const url = new URL(trimmed);
    if (url.protocol !== "https:" && url.protocol !== "http:") {
      return null;
    }
    return url;
  } catch {
    return null;
  }
}

function stripSlash(url: URL): string {
  return `${url.origin}${url.pathname.replace(/\/+$/, "")}`;
}

function text(value: string | undefined): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

/** Reads cloud mode's configuration; `null` when cloud mode is not configured. */
export function readCloudConfig(env: CloudEnvironment): CloudConfig | null {
  const api = baseUrl(env.api);
  if (!api) {
    return null;
  }
  const auth = baseUrl(env.auth);
  return {
    apiUrl: stripSlash(api),
    apiHost: api.host,
    authUrl: auth ? auth.origin : null,
    turnstileSiteKey: text(env.turnstileSiteKey),
    operator: text(env.operator),
  };
}

function envConfig(): CloudEnvironment {
  return {
    api: process.env.NEXT_PUBLIC_ARIAD_API_URL,
    auth: process.env.NEXT_PUBLIC_ARIAD_AUTH_URL,
    turnstileSiteKey: process.env.NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY,
    operator: process.env.NEXT_PUBLIC_ARIAD_OPERATOR,
  };
}

/** This build's cloud configuration, or `null` when cloud mode is hidden. */
export const CLOUD: CloudConfig | null = readCloudConfig(envConfig());

export interface AccountCloudConfig {
  /** Base URL of `ariad-server` (origin plus any path prefix, without a trailing slash). */
  apiUrl: string;
  /** Origin of the auth service, which serves `/api/auth/*`. */
  authUrl: string;
}

/** Builds the account-page config from raw values; `null` unless both URLs are valid. */
export function resolveCloudConfig(values: {
  apiUrl?: string;
  authUrl?: string;
}): AccountCloudConfig | null {
  const config = readCloudConfig({ api: values.apiUrl, auth: values.authUrl });
  return config?.authUrl ? { apiUrl: config.apiUrl, authUrl: config.authUrl } : null;
}

/** The account-page config of this build. Property accesses stay literal for Next to inline them. */
export function cloudConfig(): AccountCloudConfig | null {
  const env = envConfig();
  return resolveCloudConfig({
    apiUrl: env.api,
    authUrl: env.auth,
  });
}

/** The host name shown to people, for example `api.example.com`. */
export function displayHost(url: string): string {
  return new URL(url).host;
}
