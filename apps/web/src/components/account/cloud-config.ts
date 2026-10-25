// Where the cloud services live. Both origins are compiled into the site from
// NEXT_PUBLIC_* variables, so a local-only build (no variables) shows the
// "cloud not configured" state instead of calling anything.

export interface CloudConfig {
  /** Origin of ariad-server, which serves `/v1`. */
  apiUrl: string;
  /** Origin of the auth service, which serves `/api/auth/*`. */
  authUrl: string;
}

/** Returns the origin of an http(s) URL without a trailing slash, or null. */
function origin(value: string | undefined): string | null {
  const trimmed = value?.trim();
  if (!trimmed) {
    return null;
  }
  try {
    const url = new URL(trimmed);
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return null;
    }
    return url.origin;
  } catch {
    return null;
  }
}

/** Builds the config from raw values; null unless both origins are valid. */
export function resolveCloudConfig(values: {
  apiUrl?: string;
  authUrl?: string;
}): CloudConfig | null {
  const apiUrl = origin(values.apiUrl);
  const authUrl = origin(values.authUrl);
  return apiUrl && authUrl ? { apiUrl, authUrl } : null;
}

/** The config of this build. The property accesses must stay literal for Next to inline them. */
export function cloudConfig(): CloudConfig | null {
  return resolveCloudConfig({
    apiUrl: process.env.NEXT_PUBLIC_ARIAD_API_URL,
    authUrl: process.env.NEXT_PUBLIC_ARIAD_AUTH_URL,
  });
}

/** The host name shown to people, for example `api.example.com`. */
export function displayHost(url: string): string {
  return new URL(url).host;
}
