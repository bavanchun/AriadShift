// Client ID Metadata Documents (CIMD, draft-ietf-oauth-client-id-metadata-document)
// for remote MCP OAuth. When a client sends an `https://` URL with a non-root
// path as `client_id`, the authorization server resolves DNS first, refuses
// non-public IPv4 and IPv6 addresses, connects directly to the vetted IP over
// HTTPS without following redirects or sending credentials, validates the JSON
// metadata document, and caches it with a bounded TTL.

import dns from "node:dns";
import https from "node:https";
import net from "node:net";
import {
  type ClientDiscovery,
  type OAuthClient,
  type OAuthOptions,
  type OAuthProviderExtension,
  oauthClientMetadataSchema,
  type SchemaClient,
  type Scope,
} from "@better-auth/oauth-provider";
import {
  isForbiddenCimdClientMetadataField,
  registerClientMetadataDocument,
} from "@better-auth/oauth-provider/internal";
import { getIP } from "better-auth/api";

/** Client discovery provenance identifier stored on CIMD `oauthClient` rows. */
export const CIMD_DISCOVERY_ID = "cimd";

/** Retain unused clients long enough for abandoned authorization flows to expire. */
export const CIMD_UNUSED_RETENTION_MS = 30 * 24 * 60 * 60 * 1000;
export const CIMD_CLEANUP_INTERVAL_MS = 60 * 60 * 1000;

/** Delete a bounded batch atomically; foreign keys protect concurrent uses. */
export async function cleanupUnusedCimdClients(
  query: (sql: string, parameters: [string, string]) => Promise<unknown>,
  now = Date.now(),
): Promise<void> {
  await query(
    `DELETE FROM "oauthClient" WHERE "clientId" IN (
      SELECT c."clientId" FROM "oauthClient" c
      WHERE c."clientDiscoveryId" = $1 AND c."disabled" IS NOT TRUE
        AND COALESCE(c."updatedAt", c."createdAt") < $2
        AND NOT EXISTS (SELECT 1 FROM "oauthConsent" x WHERE x."clientId" = c."clientId")
        AND NOT EXISTS (SELECT 1 FROM "oauthAccessToken" x WHERE x."clientId" = c."clientId")
        AND NOT EXISTS (SELECT 1 FROM "oauthRefreshToken" x WHERE x."clientId" = c."clientId")
      LIMIT 100
    )`,
    [CIMD_DISCOVERY_ID, new Date(now - CIMD_UNUSED_RETENTION_MS).toISOString()],
  );
}

/** Deadline (ms) covering DNS, TLS, headers, and the response body. */
export const CIMD_TIMEOUT_MS = 5_000;

/** Maximum accepted CIMD response body size in bytes (16 KiB). */
export const CIMD_MAX_BODY_BYTES = 16 * 1024;

/** Maximum cache lifetime for a validated CIMD document (24 hours). */
export const CIMD_MAX_CACHE_TTL_MS = 24 * 60 * 60 * 1000;

/** Default cache lifetime when `Cache-Control` omits `max-age` (5 minutes). */
export const CIMD_DEFAULT_CACHE_TTL_MS = 5 * 60 * 1000;

/** Maximum number of CIMD documents retained in the in-memory cache. */
export const CIMD_MAX_CACHE_ENTRIES = 256;
const CIMD_MIN_CACHE_TTL_MS = 60_000;
const CIMD_NEGATIVE_TTL_MS = 30_000;
const CIMD_MAX_OUTBOUND = 8;
let outboundCount = 0;

const JSON_MEDIA_TYPE = /^application\/json(?:\s*;|$)/i;
const SAFE_REDIRECT_RESERVED_SCHEMES = new Set([
  "about:",
  "blob:",
  "data:",
  "file:",
  "filesystem:",
  "ftp:",
  "http:",
  "https:",
  "javascript:",
  "mailto:",
  "tel:",
  "vbscript:",
  "ws:",
  "wss:",
]);

export type IpAddressClass =
  | "public"
  | "loopback"
  | "private"
  | "link-local"
  | "cgnat"
  | "multicast"
  | "unspecified"
  | "reserved"
  | "invalid";

export type CimdErrorCode =
  | "invalid_client_id_url"
  | "forbidden_address"
  | "dns_lookup_failed"
  | "timeout"
  | "redirect_not_allowed"
  | "unexpected_status"
  | "invalid_content_type"
  | "response_too_large"
  | "invalid_json"
  | "client_id_mismatch"
  | "forbidden_metadata_field"
  | "invalid_metadata"
  | "invalid_redirect_uri"
  | "network_error";

export class CimdError extends Error {
  readonly code: CimdErrorCode;

  constructor(code: CimdErrorCode, message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "CimdError";
    this.code = code;
  }
}

export interface ResolvedIpAddress {
  address: string;
  family: 4 | 6;
}

export type CimdDnsLookup = (
  hostname: string,
  signal: AbortSignal,
) => Promise<readonly ResolvedIpAddress[]>;

export interface CimdRawResponse {
  status: number;
  headers: Record<string, string | string[] | undefined>;
  body: AsyncIterable<Uint8Array> | Uint8Array | string;
}

export type CimdTransport = (input: {
  url: URL;
  vettedAddress: ResolvedIpAddress;
  timeoutMs: number;
  maxBodyBytes: number;
  signal: AbortSignal;
}) => Promise<CimdRawResponse>;

export interface CimdHttpsConnectOptions {
  /** Custom CA certificate(s) for testing against a local TLS server. */
  ca?: string | Buffer | Array<string | Buffer>;
  /**
   * Override the TCP target host/port after DNS and IP vetting succeed, so
   * tests can direct a vetted public hostname to a local `node:https` server.
   */
  connectHost?: string;
  connectPort?: number;
}

export interface CimdFetcherOptions {
  lookup?: CimdDnsLookup;
  transport?: CimdTransport;
  https?: CimdHttpsConnectOptions;
  timeoutMs?: number;
  maxBodyBytes?: number;
  maxCacheTtlMs?: number;
  defaultCacheTtlMs?: number;
  maxCacheEntries?: number;
  now?: () => number;
  allowedHosts?: string[];
  deniedHosts?: string[];
  registrationRateLimit?: { window: number; max: number };
}

export interface CimdFetchResult {
  metadata: OAuthClient;
  fromCache: boolean;
  expiresAt: number;
}

export interface CimdFetcher {
  fetchDocument: (clientId: string) => Promise<CimdFetchResult>;
  clearCache: () => void;
}

function stripIpv6Brackets(host: string): string {
  return host.startsWith("[") && host.endsWith("]") ? host.slice(1, -1) : host;
}

function classifyIpv4Octets(a: number, b: number, c: number, _d: number): IpAddressClass {
  // 0.0.0.0/8 - Unspecified ("this network", RFC 1122)
  if (a === 0) return "unspecified";
  // 127.0.0.0/8 - Loopback (RFC 1122)
  if (a === 127) return "loopback";
  // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16 - Private-Use (RFC 1918)
  if (a === 10) return "private";
  if (a === 172 && b >= 16 && b <= 31) return "private";
  if (a === 192 && b === 168) return "private";
  // 169.254.0.0/16 - Link-Local (RFC 3927, includes cloud metadata 169.254.169.254)
  if (a === 169 && b === 254) return "link-local";
  // 100.64.0.0/10 - Shared Address Space / CGNAT (RFC 6598)
  if (a === 100 && b >= 64 && b <= 127) return "cgnat";
  // 224.0.0.0/4 - Multicast (RFC 5771)
  if (a >= 224 && a <= 239) return "multicast";
  // 240.0.0.0/4 - Reserved for future use and 255.255.255.255 broadcast
  if (a >= 240) return "reserved";
  // 192.0.0.0/24 - IETF Protocol Assignments (RFC 6890)
  if (a === 192 && b === 0 && c === 0) return "reserved";
  // 198.18.0.0/15 - Benchmarking (RFC 2544)
  if (a === 198 && (b === 18 || b === 19)) return "reserved";
  // Documentation and deprecated 6to4 relay ranges are not public targets.
  if (
    (a === 192 && b === 0 && c === 2) ||
    (a === 198 && b === 51 && c === 100) ||
    (a === 203 && b === 0 && c === 113) ||
    (a === 192 && b === 88 && c === 99)
  )
    return "reserved";
  return "public";
}

function parseIpv4Octets(ip: string): [number, number, number, number] | null {
  if (!net.isIPv4(ip)) return null;
  const parts = ip.split(".");
  if (parts.length !== 4) return null;
  const octets: number[] = [];
  for (const part of parts) {
    if (!/^(?:0|[1-9]\d{0,2})$/.test(part)) return null;
    const value = Number(part);
    if (!Number.isInteger(value) || value < 0 || value > 255) return null;
    octets.push(value);
  }
  return octets as [number, number, number, number];
}

function parseIpv6Words(
  ip: string,
): [number, number, number, number, number, number, number, number] | null {
  if (ip.includes("%") || !net.isIPv6(ip)) return null;
  let normalized = ip;
  const lastColon = normalized.lastIndexOf(":");
  if (lastColon >= 0 && normalized.slice(lastColon + 1).includes(".")) {
    const ipv4Part = normalized.slice(lastColon + 1);
    const octets = parseIpv4Octets(ipv4Part);
    if (!octets) return null;
    const high = ((octets[0] << 8) | octets[1]).toString(16);
    const low = ((octets[2] << 8) | octets[3]).toString(16);
    normalized = `${normalized.slice(0, lastColon)}:${high}:${low}`;
  }

  let groups: string[];
  if (normalized.includes("::")) {
    const halves = normalized.split("::");
    if (halves.length !== 2) return null;
    const left = halves[0] ? halves[0].split(":") : [];
    const right = halves[1] ? halves[1].split(":") : [];
    const missing = 8 - (left.length + right.length);
    if (missing < 1) return null;
    groups = [...left, ...Array.from({ length: missing }, () => "0"), ...right];
  } else {
    groups = normalized.split(":");
  }

  if (groups.length !== 8) return null;
  const words: number[] = [];
  for (const group of groups) {
    if (!/^[0-9a-fA-F]{1,4}$/.test(group)) return null;
    words.push(Number.parseInt(group, 16));
  }
  return words as [number, number, number, number, number, number, number, number];
}

function classifyEmbeddedIpv4Words(highWord: number, lowWord: number): IpAddressClass {
  const embeddedClass = classifyIpv4Octets(
    (highWord >> 8) & 0xff,
    highWord & 0xff,
    (lowWord >> 8) & 0xff,
    lowWord & 0xff,
  );
  return embeddedClass === "public" ? "reserved" : embeddedClass;
}

/**
 * Classifies an IPv4 or IPv6 literal into `"public"` or the specific special-use
 * address class that causes CIMD resolution to refuse it.
 */
export function classifyIpAddress(address: string): IpAddressClass {
  if (typeof address !== "string" || address.length === 0) return "invalid";
  const raw = stripIpv6Brackets(address);
  if (raw.includes("%")) return "invalid";

  const family = net.isIP(raw);
  if (family === 4) {
    const octets = parseIpv4Octets(raw);
    if (!octets) return "invalid";
    return classifyIpv4Octets(octets[0], octets[1], octets[2], octets[3]);
  }
  if (family !== 6) return "invalid";

  const w = parseIpv6Words(raw);
  if (!w) return "invalid";
  if (
    (w[0] === 0x2001 && (w[1] & 0xfe00) === 0) ||
    (w[0] === 0x3fff && (w[1] & 0xf000) === 0) ||
    w[0] === 0x5f00
  )
    return "reserved";

  // ::/128 - Unspecified (RFC 4291)
  if (w.every((word) => word === 0)) return "unspecified";
  // ::1/128 - Loopback (RFC 4291)
  if (w.slice(0, 7).every((word) => word === 0) && w[7] === 1) return "loopback";
  // ::ffff:0:0/96 - IPv4-mapped IPv6 (RFC 4291)
  if (w.slice(0, 5).every((word) => word === 0) && w[5] === 0xffff) {
    return classifyEmbeddedIpv4Words(w[6], w[7]);
  }
  // ::/96 - IPv4-compatible IPv6 (deprecated RFC 4291)
  if (w.slice(0, 6).every((word) => word === 0)) {
    return classifyEmbeddedIpv4Words(w[6], w[7]);
  }
  // ::ffff:0:0:0/96 - IPv4-translated (RFC 2765)
  if (w.slice(0, 4).every((word) => word === 0) && w[4] === 0xffff && w[5] === 0) {
    return classifyEmbeddedIpv4Words(w[6], w[7]);
  }
  // 64:ff9b::/96 and 64:ff9b:1::/48 - NAT64 (RFC 6052 / RFC 8215)
  if (
    w[0] === 0x0064 &&
    w[1] === 0xff9b &&
    ((w[2] === 0 && w[3] === 0 && w[4] === 0 && w[5] === 0) || w[2] === 1)
  ) {
    return classifyEmbeddedIpv4Words(w[6], w[7]);
  }
  // 2002::/16 - 6to4 (RFC 3056)
  if (w[0] === 0x2002) {
    return classifyEmbeddedIpv4Words(w[1], w[2]);
  }
  // 2001:0000::/32 - Teredo (RFC 4380)
  if (w[0] === 0x2001 && w[1] === 0x0000) return "reserved";
  // 2001:db8::/32 - Documentation (RFC 3849)
  if (w[0] === 0x2001 && w[1] === 0x0db8) return "reserved";
  // 100::/64 - Discard-Only (RFC 6666)
  if (w[0] === 0x0100 && w[1] === 0 && w[2] === 0 && w[3] === 0) return "reserved";
  // fc00::/7 - Unique Local IPv6 Unicast (RFC 4193)
  if ((w[0] & 0xfe00) === 0xfc00) return "private";
  // fe80::/10 - Link-Local IPv6 Unicast (RFC 4291)
  if ((w[0] & 0xffc0) === 0xfe80) return "link-local";
  // fec0::/10 - Site-Local (deprecated RFC 3879)
  if ((w[0] & 0xffc0) === 0xfec0) return "private";
  // ff00::/8 - Multicast (RFC 4291)
  if ((w[0] & 0xff00) === 0xff00) return "multicast";

  return "public";
}

/**
 * Returns `true` only when `address` is a globally routable public IPv4 or IPv6
 * literal outside loopback, RFC 1918 / RFC 4193 private, link-local, CGNAT
 * (`100.64.0.0/10`), multicast, unspecified, and reserved ranges.
 */
export function isAllowedCimdAddress(address: string): boolean {
  return classifyIpAddress(address) === "public";
}

/**
 * Checks whether a `client_id` is a syntactically valid CIMD URL:
 * an `https:` URL with a non-root pathname, no credentials, no fragment, and
 * no `.` or `..` path segments.
 */
export function isCimdClientId(clientId: string): boolean {
  try {
    parseCimdClientIdUrl(clientId);
    return true;
  } catch {
    return false;
  }
}

export function parseCimdClientIdUrl(clientId: string): URL {
  if (typeof clientId !== "string" || clientId.length === 0 || clientId.length > 2048) {
    throw new CimdError("invalid_client_id_url", "CIMD client_id must be a non-empty URL string");
  }
  if (clientId.includes("#")) {
    throw new CimdError("invalid_client_id_url", "CIMD client_id must not contain a URL fragment");
  }
  let url: URL;
  try {
    url = new URL(clientId);
  } catch {
    throw new CimdError("invalid_client_id_url", "CIMD client_id is not a valid URL");
  }
  if (url.protocol !== "https:") {
    throw new CimdError("invalid_client_id_url", "CIMD client_id must use the https: scheme");
  }
  if (
    url.href !== clientId ||
    url.port !== "" ||
    clientId.includes("\\") ||
    /%2e/i.test(clientId)
  ) {
    throw new CimdError(
      "invalid_client_id_url",
      "CIMD client_id must be a canonical HTTPS URL on port 443",
    );
  }
  if (url.username !== "" || url.password !== "") {
    throw new CimdError("invalid_client_id_url", "CIMD client_id must not include credentials");
  }
  if (!url.hostname) {
    throw new CimdError("invalid_client_id_url", "CIMD client_id must include a hostname");
  }
  const rawMatch = /^https:\/\/[^/?#]+(\/[^?#]*)?(?:\?[^#]*)?$/i.exec(clientId);
  const rawPath = rawMatch?.[1] ?? "";
  if (!rawPath || rawPath === "/" || url.pathname === "/") {
    throw new CimdError("invalid_client_id_url", "CIMD client_id must include a non-root path");
  }
  const segments = rawPath.split("/");
  if (segments.some((segment) => segment === "." || segment === "..")) {
    throw new CimdError(
      "invalid_client_id_url",
      "CIMD client_id path must not contain dot segments",
    );
  }
  return url;
}

/**
 * Computes the cache TTL in milliseconds from a `Cache-Control` header value,
 * capped at `maxTtlMs` (default 24 hours). Returns `0` when caching is
 * disallowed (`no-store`, `no-cache`, or `max-age=0`).
 */
export function parseCacheControlTtlMs(
  cacheControlHeader: string | string[] | undefined,
  defaultTtlMs = CIMD_DEFAULT_CACHE_TTL_MS,
  maxTtlMs = CIMD_MAX_CACHE_TTL_MS,
): number {
  const boundedDefault = Math.max(0, Math.min(defaultTtlMs, maxTtlMs));
  const raw = Array.isArray(cacheControlHeader)
    ? cacheControlHeader.join(", ")
    : cacheControlHeader;
  if (!raw || raw.trim().length === 0) return boundedDefault;

  let maxAgeSeconds: number | undefined;
  for (const part of raw.split(",")) {
    const token = part.trim();
    if (!token) continue;
    const eqIndex = token.indexOf("=");
    const directive = (eqIndex >= 0 ? token.slice(0, eqIndex) : token).trim().toLowerCase();
    const rawValue =
      eqIndex >= 0
        ? token
            .slice(eqIndex + 1)
            .trim()
            .replace(/^"|"$/g, "")
        : undefined;

    if (directive === "no-store" || directive === "no-cache") {
      return 0;
    }
    if (directive === "max-age") {
      if (rawValue === undefined || !/^\d+$/.test(rawValue)) return 0;
      const parsed = Number.parseInt(rawValue, 10);
      if (!Number.isSafeInteger(parsed) || parsed <= 0) return 0;
      maxAgeSeconds = parsed;
    }
  }

  if (maxAgeSeconds === undefined) return boundedDefault;
  return Math.min(maxAgeSeconds * 1000, maxTtlMs);
}

function isExactLoopbackHttpRedirectUri(redirectUri: string): boolean {
  const authority = /^http:\/\/([^/?#]*)/i.exec(redirectUri)?.[1];
  if (!authority) return false;
  const hostAndPort = authority.slice(authority.lastIndexOf("@") + 1);
  const host = hostAndPort.startsWith("[")
    ? hostAndPort.slice(0, hostAndPort.indexOf("]") + 1).toLowerCase()
    : (hostAndPort.split(":")[0] ?? "").toLowerCase();
  return host === "localhost" || host === "127.0.0.1" || host === "[::1]";
}

function isValidPrivateUseSchemeRedirect(url: URL): boolean {
  const protocol = url.protocol.toLowerCase();
  if (SAFE_REDIRECT_RESERVED_SCHEMES.has(protocol)) return false;
  if (!/^[a-z][a-z0-9+.-]*:$/.test(protocol)) return false;
  if (url.host === "") {
    const schemeBase = protocol.slice(0, -1);
    return schemeBase.includes(".") && url.pathname.length > 0;
  }
  return url.pathname.length > 0;
}

function validateCimdRedirectUri(
  redirectUri: string,
  applicationType: "web" | "native" | undefined,
): void {
  let url: URL;
  try {
    url = new URL(redirectUri);
  } catch {
    throw new CimdError("invalid_redirect_uri", `Invalid redirect_uri: ${redirectUri}`);
  }
  if (redirectUri.includes("#") || url.username !== "" || url.password !== "") {
    throw new CimdError(
      "invalid_redirect_uri",
      `redirect_uri must not contain credentials or a fragment: ${redirectUri}`,
    );
  }
  if (/^localhost\.+$/i.test(url.hostname)) {
    throw new CimdError(
      "invalid_redirect_uri",
      `redirect_uri localhost must not contain trailing dots: ${redirectUri}`,
    );
  }
  const hostIp = stripIpv6Brackets(url.hostname);
  const isLoopback =
    url.hostname.toLowerCase() === "localhost" || classifyIpAddress(hostIp) === "loopback";

  if (applicationType === "web") {
    if (url.protocol !== "https:" || isLoopback) {
      throw new CimdError(
        "invalid_redirect_uri",
        `web client redirect_uri must use https on a non-loopback host: ${redirectUri}`,
      );
    }
    return;
  }

  if (url.protocol === "https:") {
    if (isLoopback) {
      throw new CimdError(
        "invalid_redirect_uri",
        `redirect_uri must not use https on a loopback host: ${redirectUri}`,
      );
    }
    return;
  }

  if (url.protocol === "http:") {
    if (!isExactLoopbackHttpRedirectUri(redirectUri)) {
      throw new CimdError(
        "invalid_redirect_uri",
        `http redirect_uri is only allowed on localhost, 127.0.0.1, or [::1]: ${redirectUri}`,
      );
    }
    return;
  }

  if (!isValidPrivateUseSchemeRedirect(url)) {
    throw new CimdError("invalid_redirect_uri", `Unsupported redirect_uri scheme: ${redirectUri}`);
  }
}

/**
 * Validates a parsed JSON value as a Client ID Metadata Document for
 * `expectedClientId`.
 */
export function validateCimdMetadataDocument(raw: unknown, expectedClientId: string): OAuthClient {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    throw new CimdError("invalid_metadata", "CIMD response body must be a JSON object");
  }
  const record = raw as Record<string, unknown>;
  if (typeof record.client_id !== "string" || record.client_id !== expectedClientId) {
    throw new CimdError(
      "client_id_mismatch",
      "CIMD client_id must exactly match the fetched document URL",
    );
  }
  for (const key of Object.keys(record)) {
    if (isForbiddenCimdClientMetadataField(key)) {
      throw new CimdError(
        "forbidden_metadata_field",
        `CIMD document contains forbidden field: ${key}`,
      );
    }
  }
  if (
    record.backchannel_logout_uri !== undefined ||
    record.backchannel_logout_session_required !== undefined
  ) {
    throw new CimdError(
      "forbidden_metadata_field",
      "CIMD document must not register a back-channel logout URI",
    );
  }

  const parsed = oauthClientMetadataSchema.safeParse(record);
  if (!parsed.success) {
    throw new CimdError(
      "invalid_metadata",
      parsed.error.issues[0]?.message ?? "Invalid CIMD client metadata",
    );
  }

  const metadata = parsed.data;
  if ("metadata" in metadata) delete metadata.metadata;
  if (!Array.isArray(metadata.redirect_uris) || metadata.redirect_uris.length === 0) {
    throw new CimdError(
      "invalid_redirect_uri",
      "CIMD document must include a non-empty redirect_uris array",
    );
  }
  for (const redirectUri of metadata.redirect_uris) {
    validateCimdRedirectUri(redirectUri, metadata.application_type);
  }

  const authMethod = metadata.token_endpoint_auth_method ?? "none";
  if (metadata.jwks_uri) {
    throw new CimdError("invalid_metadata", "CIMD jwks_uri is not supported");
  }
  if (authMethod !== "none" && authMethod !== "private_key_jwt") {
    throw new CimdError(
      "invalid_metadata",
      `CIMD client cannot use token_endpoint_auth_method ${authMethod}`,
    );
  }
  if (authMethod === "private_key_jwt" && !metadata.jwks && !metadata.jwks_uri) {
    throw new CimdError(
      "invalid_metadata",
      "CIMD private_key_jwt client requires jwks or jwks_uri",
    );
  }

  const grantTypes = metadata.grant_types ?? ["authorization_code", "refresh_token"];
  if (!grantTypes.includes("authorization_code") || grantTypes.includes("client_credentials")) {
    throw new CimdError(
      "invalid_metadata",
      "CIMD client grant_types must include authorization_code and must not include client_credentials",
    );
  }

  const responseTypes = metadata.response_types ?? ["code"];
  if (!responseTypes.includes("code")) {
    throw new CimdError("invalid_metadata", "CIMD client response_types must include code");
  }

  return {
    ...(metadata as OAuthClient),
    client_id: expectedClientId,
    redirect_uris: metadata.redirect_uris,
    token_endpoint_auth_method: authMethod,
    grant_types: grantTypes,
    response_types: responseTypes,
  };
}

const defaultDnsLookup: CimdDnsLookup = async (hostname, signal) => {
  const resolver = new dns.promises.Resolver({ timeout: CIMD_TIMEOUT_MS, tries: 1 });
  const cancel = () => resolver.cancel();
  signal.addEventListener("abort", cancel, { once: true });
  try {
    const results = await Promise.allSettled([
      resolver.resolve4(hostname),
      resolver.resolve6(hostname),
    ]);
    return results.flatMap((result, index) => {
      if (result.status === "fulfilled") {
        return result.value.map((address) => ({ address, family: (index === 0 ? 4 : 6) as 4 | 6 }));
      }
      if (!["ENODATA", "ENOTFOUND"].includes((result.reason as NodeJS.ErrnoException).code ?? "")) {
        throw result.reason;
      }
      return [];
    });
  } finally {
    signal.removeEventListener("abort", cancel);
  }
};

async function resolveAndVetHost(
  hostname: string,
  lookup: CimdDnsLookup,
  signal: AbortSignal,
): Promise<ResolvedIpAddress> {
  const bareHost = stripIpv6Brackets(hostname).toLowerCase();
  if (!bareHost || bareHost === "localhost" || bareHost.endsWith(".localhost")) {
    throw new CimdError("forbidden_address", `Refusing reserved host: ${hostname}`);
  }

  const literalFamily = net.isIP(bareHost);
  if (literalFamily === 4 || literalFamily === 6) {
    const classification = classifyIpAddress(bareHost);
    if (classification !== "public") {
      throw new CimdError(
        "forbidden_address",
        `Refusing ${classification} IP address literal: ${bareHost}`,
      );
    }
    return { address: bareHost, family: literalFamily };
  }

  let resolved: readonly ResolvedIpAddress[];
  try {
    resolved = await Promise.race([
      lookup(bareHost, signal),
      new Promise<never>((_, reject) => {
        if (signal.aborted) reject(new Error("DNS lookup aborted"));
        else
          signal.addEventListener("abort", () => reject(new Error("DNS lookup aborted")), {
            once: true,
          });
      }),
    ]);
  } catch (error) {
    if (signal.aborted) {
      throw new CimdError("timeout", "CIMD DNS lookup timed out", { cause: error });
    }
    throw new CimdError("dns_lookup_failed", `DNS lookup failed for ${bareHost}`, {
      cause: error,
    });
  }

  if (signal.aborted) {
    throw new CimdError("timeout", "CIMD DNS lookup timed out");
  }
  if (!resolved || resolved.length === 0) {
    throw new CimdError("dns_lookup_failed", `DNS lookup returned no addresses for ${bareHost}`);
  }

  for (const entry of resolved) {
    const classification = classifyIpAddress(entry.address);
    if (classification !== "public") {
      throw new CimdError(
        "forbidden_address",
        `Refusing ${classification} address ${entry.address} resolved for ${bareHost}`,
      );
    }
  }

  const first = resolved[0];
  if (!first) {
    throw new CimdError("dns_lookup_failed", `DNS lookup returned no addresses for ${bareHost}`);
  }
  return first;
}

function createHttpsTransport(httpsOptions?: CimdHttpsConnectOptions): CimdTransport {
  return ({ url, vettedAddress, maxBodyBytes, signal }) =>
    new Promise<CimdRawResponse>((resolve, reject) => {
      if (signal.aborted) {
        reject(new CimdError("timeout", "CIMD fetch timed out"));
        return;
      }

      const connectAddress = httpsOptions?.connectHost ?? vettedAddress.address;
      const connectFamily: 4 | 6 = httpsOptions?.connectHost
        ? net.isIP(stripIpv6Brackets(httpsOptions.connectHost)) === 6
          ? 6
          : 4
        : vettedAddress.family;
      const bareHost = stripIpv6Brackets(url.hostname);
      const port = httpsOptions?.connectPort ?? (url.port ? Number(url.port) : 443);

      const req = https.request({
        protocol: "https:",
        hostname: bareHost,
        port,
        path: `${url.pathname}${url.search}`,
        method: "GET",
        agent: false,
        servername: net.isIP(bareHost) ? undefined : bareHost,
        ca: httpsOptions?.ca,
        headers: {
          accept: "application/json",
          "user-agent": "AriadShift-Auth-CIMD/1.0",
          host: url.port ? `${url.hostname}:${url.port}` : url.hostname,
        },
        lookup: (_hostname, options, callback) => {
          const all = typeof options === "object" && options !== null && options.all === true;
          if (all) {
            callback(null, [{ address: connectAddress, family: connectFamily }]);
          } else {
            callback(null, connectAddress, connectFamily);
          }
        },
      });

      const onAbort = () => {
        req.destroy(new CimdError("timeout", "CIMD fetch timed out"));
      };
      signal.addEventListener("abort", onAbort, { once: true });

      req.on("error", (error) => {
        signal.removeEventListener("abort", onAbort);
        if (error instanceof CimdError) {
          reject(error);
          return;
        }
        if (signal.aborted) {
          reject(new CimdError("timeout", "CIMD fetch timed out", { cause: error }));
          return;
        }
        reject(new CimdError("network_error", error.message, { cause: error }));
      });

      req.on("response", (res) => {
        const status = res.statusCode ?? 0;
        const headers = res.headers as Record<string, string | string[] | undefined>;
        const contentLengthHeader = getHeaderValue(headers, "content-length");
        if (contentLengthHeader !== undefined) {
          const declaredLength = Number(contentLengthHeader.trim());
          if (
            !Number.isFinite(declaredLength) ||
            declaredLength < 0 ||
            declaredLength > maxBodyBytes
          ) {
            signal.removeEventListener("abort", onAbort);
            const err = new CimdError(
              "response_too_large",
              `CIMD Content-Length exceeds ${maxBodyBytes} bytes`,
            );
            res.destroy(err);
            reject(err);
            return;
          }
        }
        const chunks: Uint8Array[] = [];
        let totalBytes = 0;

        res.on("data", (chunk: Buffer | Uint8Array) => {
          totalBytes += chunk.byteLength;
          if (totalBytes > maxBodyBytes) {
            res.destroy(
              new CimdError("response_too_large", `CIMD response exceeds ${maxBodyBytes} bytes`),
            );
            return;
          }
          chunks.push(new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength));
        });

        res.on("error", (error) => {
          signal.removeEventListener("abort", onAbort);
          if (error instanceof CimdError) {
            reject(error);
            return;
          }
          if (signal.aborted) {
            reject(new CimdError("timeout", "CIMD fetch timed out", { cause: error }));
            return;
          }
          reject(new CimdError("network_error", error.message, { cause: error }));
        });

        res.on("end", () => {
          signal.removeEventListener("abort", onAbort);
          if (signal.aborted) {
            reject(new CimdError("timeout", "CIMD fetch timed out"));
            return;
          }
          const combined = new Uint8Array(totalBytes);
          let offset = 0;
          for (const part of chunks) {
            combined.set(part, offset);
            offset += part.byteLength;
          }
          resolve({ status, headers, body: combined });
        });
      });

      req.end();
    });
}

function getHeaderValue(
  headers: Record<string, string | string[] | undefined>,
  name: string,
): string | undefined {
  const direct = headers[name] ?? headers[name.toLowerCase()];
  if (typeof direct === "string") return direct;
  if (Array.isArray(direct)) return direct.join(", ");
  for (const [key, value] of Object.entries(headers)) {
    if (key.toLowerCase() === name.toLowerCase()) {
      return Array.isArray(value) ? value.join(", ") : value;
    }
  }
  return undefined;
}

async function readBoundedBody(
  body: AsyncIterable<Uint8Array> | Uint8Array | string,
  maxBodyBytes: number,
  signal: AbortSignal,
): Promise<string> {
  if (typeof body === "string") {
    const encoded = new TextEncoder().encode(body);
    if (encoded.byteLength > maxBodyBytes) {
      throw new CimdError("response_too_large", `CIMD response exceeds ${maxBodyBytes} bytes`);
    }
    return body;
  }
  if (body instanceof Uint8Array) {
    if (body.byteLength > maxBodyBytes) {
      throw new CimdError("response_too_large", `CIMD response exceeds ${maxBodyBytes} bytes`);
    }
    return new TextDecoder().decode(body);
  }

  const chunks: Uint8Array[] = [];
  let totalBytes = 0;
  for await (const chunk of body) {
    if (signal.aborted) {
      throw new CimdError("timeout", "CIMD fetch timed out");
    }
    totalBytes += chunk.byteLength;
    if (totalBytes > maxBodyBytes) {
      throw new CimdError("response_too_large", `CIMD response exceeds ${maxBodyBytes} bytes`);
    }
    chunks.push(chunk);
  }
  const bytes = new Uint8Array(totalBytes);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return new TextDecoder().decode(bytes);
}

interface CacheEntry {
  metadata: OAuthClient;
  fetchedAt: number;
  expiresAt: number;
}

/**
 * Creates a CIMD fetcher enforcing the SSRF-vetted HTTPS transport policy and
 * bounded TTL caching.
 */
export function createCimdFetcher(options: CimdFetcherOptions = {}): CimdFetcher {
  const lookup = options.lookup ?? defaultDnsLookup;
  const transport = options.transport ?? createHttpsTransport(options.https);
  const timeoutMs = options.timeoutMs ?? CIMD_TIMEOUT_MS;
  const maxBodyBytes = options.maxBodyBytes ?? CIMD_MAX_BODY_BYTES;
  const maxCacheTtlMs = options.maxCacheTtlMs ?? CIMD_MAX_CACHE_TTL_MS;
  const defaultCacheTtlMs = options.defaultCacheTtlMs ?? CIMD_DEFAULT_CACHE_TTL_MS;
  const maxCacheEntries = options.maxCacheEntries ?? CIMD_MAX_CACHE_ENTRIES;
  const now = options.now ?? Date.now;

  const cache = new Map<string, CacheEntry>();
  const failures = new Map<string, { error: CimdError; expiresAt: number }>();
  const hostFailures = new Map<string, { error: CimdError; expiresAt: number }>();
  const pending = new Map<string, Promise<CimdFetchResult>>();

  function getCached(clientId: string, currentTime: number): CacheEntry | undefined {
    const entry = cache.get(clientId);
    if (!entry) return undefined;
    if (currentTime >= entry.expiresAt) {
      cache.delete(clientId);
      return undefined;
    }
    // Refresh LRU order.
    cache.delete(clientId);
    cache.set(clientId, entry);
    return entry;
  }

  function setCached(clientId: string, entry: CacheEntry, currentTime: number): void {
    if (entry.expiresAt <= currentTime || maxCacheEntries <= 0) return;
    if (cache.has(clientId)) cache.delete(clientId);
    while (cache.size >= maxCacheEntries) {
      let evicted = false;
      for (const [key, candidate] of cache) {
        if (currentTime >= candidate.expiresAt) {
          cache.delete(key);
          evicted = true;
          break;
        }
      }
      if (!evicted) {
        const oldestKey = cache.keys().next().value;
        if (oldestKey === undefined) break;
        cache.delete(oldestKey);
      }
    }
    cache.set(clientId, entry);
  }

  async function fetchUncached(clientId: string): Promise<CimdFetchResult> {
    const url = parseCimdClientIdUrl(clientId);
    const currentTime = now();
    const cached = getCached(clientId, currentTime);
    if (cached) {
      return {
        metadata: structuredClone(cached.metadata),
        fromCache: true,
        expiresAt: cached.expiresAt,
      };
    }

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs).unref();
    try {
      const vettedAddress = await resolveAndVetHost(url.hostname, lookup, controller.signal);
      let response: CimdRawResponse;
      try {
        response = await transport({
          url,
          vettedAddress,
          timeoutMs,
          maxBodyBytes,
          signal: controller.signal,
        });
      } catch (error) {
        if (error instanceof CimdError) throw error;
        if (controller.signal.aborted) {
          throw new CimdError("timeout", "CIMD fetch timed out", { cause: error });
        }
        throw new CimdError("network_error", "CIMD fetch failed", { cause: error });
      }

      if (controller.signal.aborted) {
        throw new CimdError("timeout", "CIMD fetch timed out");
      }

      if (response.status >= 300 && response.status < 400) {
        throw new CimdError(
          "redirect_not_allowed",
          `CIMD fetch refuses HTTP ${response.status} redirect`,
        );
      }
      if (response.status !== 200) {
        throw new CimdError("unexpected_status", `CIMD fetch failed with HTTP ${response.status}`);
      }

      const contentType = getHeaderValue(response.headers, "content-type");
      if (!contentType || !JSON_MEDIA_TYPE.test(contentType.trim())) {
        throw new CimdError(
          "invalid_content_type",
          `CIMD response must have application/json content-type, got: ${contentType ?? "none"}`,
        );
      }

      const contentLengthHeader = getHeaderValue(response.headers, "content-length");
      if (contentLengthHeader !== undefined) {
        const declaredLength = Number(contentLengthHeader.trim());
        if (
          !Number.isFinite(declaredLength) ||
          declaredLength < 0 ||
          declaredLength > maxBodyBytes
        ) {
          throw new CimdError(
            "response_too_large",
            `CIMD Content-Length exceeds ${maxBodyBytes} bytes`,
          );
        }
      }

      const bodyText = await readBoundedBody(response.body, maxBodyBytes, controller.signal);
      let parsedJson: unknown;
      try {
        parsedJson = JSON.parse(bodyText);
      } catch (error) {
        throw new CimdError("invalid_json", "CIMD response body is not valid JSON", {
          cause: error,
        });
      }

      const metadata = validateCimdMetadataDocument(parsedJson, clientId);
      const completedAt = now();
      const ttlMs = Math.max(
        CIMD_MIN_CACHE_TTL_MS,
        parseCacheControlTtlMs(
          getHeaderValue(response.headers, "cache-control"),
          defaultCacheTtlMs,
          maxCacheTtlMs,
        ),
      );
      const expiresAt = completedAt + ttlMs;
      if (ttlMs > 0) {
        setCached(
          clientId,
          { metadata: structuredClone(metadata), fetchedAt: completedAt, expiresAt },
          completedAt,
        );
      }

      return { metadata, fromCache: false, expiresAt };
    } finally {
      clearTimeout(timer);
    }
  }

  async function fetchDocument(clientId: string): Promise<CimdFetchResult> {
    const host = parseCimdClientIdUrl(clientId).hostname;
    const failedHost = hostFailures.get(host);
    if (failedHost && failedHost.expiresAt > now()) throw failedHost.error;
    if (failedHost) hostFailures.delete(host);
    const failed = failures.get(clientId);
    if (failed && failed.expiresAt > now()) throw failed.error;
    if (failed) failures.delete(clientId);
    const current = pending.get(clientId);
    if (current) return current;
    const cached = getCached(clientId, now());
    if (cached)
      return {
        metadata: structuredClone(cached.metadata),
        fromCache: true,
        expiresAt: cached.expiresAt,
      };
    if (outboundCount >= CIMD_MAX_OUTBOUND) {
      throw new CimdError("network_error", "CIMD outbound capacity exhausted");
    }
    outboundCount++;
    const operation = fetchUncached(clientId)
      .catch((error: unknown) => {
        if (error instanceof CimdError && maxCacheEntries > 0) {
          failures.delete(clientId);
          while (failures.size >= maxCacheEntries) {
            const oldest = failures.keys().next().value;
            if (oldest === undefined) break;
            failures.delete(oldest);
          }
          failures.set(clientId, { error, expiresAt: now() + CIMD_NEGATIVE_TTL_MS });
          if (error.code === "dns_lookup_failed" || error.code === "forbidden_address") {
            hostFailures.delete(host);
            while (hostFailures.size >= maxCacheEntries) {
              const oldest = hostFailures.keys().next().value;
              if (oldest === undefined) break;
              hostFailures.delete(oldest);
            }
            hostFailures.set(host, { error, expiresAt: now() + CIMD_NEGATIVE_TTL_MS });
          }
        }
        throw error;
      })
      .finally(() => {
        outboundCount--;
        pending.delete(clientId);
      });
    pending.set(clientId, operation);
    return operation;
  }

  return {
    fetchDocument,
    clearCache: () => {
      cache.clear();
      failures.clear();
      hostFailures.clear();
    },
  };
}

/**
 * Builds the Better Auth `ClientDiscovery` and `OAuthProviderExtension` for
 * Client ID Metadata Documents.
 */
export function createCimdExtension(options: CimdFetcherOptions = {}): OAuthProviderExtension {
  const fetcher = createCimdFetcher(options);
  const syncedRevisionByClient = new Map<string, number>();

  const clientDiscovery: ClientDiscovery = {
    id: CIMD_DISCOVERY_ID,
    discoveryMetadata: {
      client_id_metadata_document_supported: true,
    },
    matches: (clientId: string) => isCimdClientId(clientId),
    resolve: async (ctx, clientId, existing) => {
      const host = new URL(clientId).hostname;
      if (
        options.deniedHosts?.includes(host) ||
        (options.allowedHosts?.length && !options.allowedHosts.includes(host))
      )
        return null;
      if (
        !ctx.path ||
        [
          "/oauth2/token",
          "/oauth2/revoke",
          "/oauth2/delete-consent",
          "/oauth2/delete-client",
        ].includes(ctx.path.replace(/\/+$/, ""))
      ) {
        return existing ?? null;
      }
      try {
        if (!existing && ctx.context.options.rateLimit?.enabled !== false) {
          const ip = getIP(ctx.headers ?? new Headers(), ctx.context.options) ?? "no-trusted-ip";
          const adapter = ctx.context.adapter;
          const { window, max } = options.registrationRateLimit ?? { window: 3600, max: 20 };
          const consume = async (key: string): Promise<boolean> => {
            const row = (
              await adapter.findMany<{ key: string; count: number; lastRequest: number }>({
                model: "rateLimit",
                where: [{ field: "key", value: key }],
              })
            )[0];
            const now = Date.now();
            if (!row) {
              try {
                await adapter.create({
                  model: "rateLimit",
                  data: { key, count: 1, lastRequest: now },
                });
                return true;
              } catch (error) {
                const existingRow = await adapter.findMany({
                  model: "rateLimit",
                  where: [{ field: "key", value: key }],
                });
                if (!existingRow.length) throw error;
                return consume(key);
              }
            }
            const last = Number(row.lastRequest);
            if (now - last >= window * 1000) {
              const reset = await adapter.incrementOne({
                model: "rateLimit",
                where: [
                  { field: "key", value: key },
                  { field: "lastRequest", operator: "lte", value: last },
                ],
                increment: {},
                set: { count: 1, lastRequest: now },
              });
              return reset ? true : consume(key);
            }
            if (row.count >= max) return false;
            const updated = await adapter.incrementOne({
              model: "rateLimit",
              where: [
                { field: "key", value: key },
                { field: "lastRequest", operator: "gt", value: now - window * 1000 },
                { field: "count", operator: "lt", value: max },
              ],
              increment: { count: 1 },
              set: { lastRequest: now },
            });
            return updated ? true : consume(key);
          };
          if (!(await consume(`${ip}|/oauth2/register`)) || !(await consume(`cimd-host|${host}`)))
            return null;
        }
        const result = await fetcher.fetchDocument(clientId);
        if (
          existing &&
          existing.clientDiscoveryId === CIMD_DISCOVERY_ID &&
          result.fromCache &&
          syncedRevisionByClient.get(clientId) === result.expiresAt
        ) {
          syncedRevisionByClient.delete(clientId);
          syncedRevisionByClient.set(clientId, result.expiresAt);
          return existing;
        }

        const plugin = ctx.context.getPlugin("oauth-provider") as
          | { options?: OAuthOptions<Scope[]> }
          | undefined;
        if (!plugin?.options) return null;

        const registered = await registerClientMetadataDocument(ctx, plugin.options, {
          metadata: result.metadata,
          clientId,
          clientDiscoveryId: CIMD_DISCOVERY_ID,
          existingClient: existing ?? undefined,
        });
        syncedRevisionByClient.delete(clientId);
        if (syncedRevisionByClient.size >= CIMD_MAX_CACHE_ENTRIES) {
          const oldest = syncedRevisionByClient.keys().next().value;
          if (oldest !== undefined) syncedRevisionByClient.delete(oldest);
        }
        syncedRevisionByClient.set(clientId, result.expiresAt);
        return registered.client as SchemaClient<Scope[]>;
      } catch (error) {
        if (error instanceof CimdError) {
          ctx.context.logger.warn(
            `CIMD resolution failed: ${error.code} (${new URL(clientId).hostname})`,
          );
          return null;
        }
        ctx.context.logger.error("CIMD client registration failed", error);
        throw error;
      }
    },
  };

  return {
    clientDiscovery,
  };
}
