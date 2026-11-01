import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import https from "node:https";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import {
  CIMD_DEFAULT_CACHE_TTL_MS,
  CIMD_MAX_BODY_BYTES,
  CIMD_MAX_CACHE_TTL_MS,
  CIMD_TIMEOUT_MS,
  type CimdError,
  type CimdRawResponse,
  classifyIpAddress,
  createCimdFetcher,
  isAllowedCimdAddress,
  isCimdClientId,
  parseCacheControlTtlMs,
  validateCimdMetadataDocument,
} from "../src/cimd.ts";
import {
  AUTH,
  audiences,
  authorizeUrl,
  config,
  createService,
  decode,
  Jar,
  json,
  pkce,
  post,
  REDIRECT_URI,
  RESOURCE,
  signedQuery,
  token,
} from "./oauth-helpers.ts";

function generateTestCertificate(hostname: string): { key: string; cert: string } {
  const dir = mkdtempSync(join(tmpdir(), "ariad-cimd-cert-"));
  try {
    const keyPath = join(dir, "key.pem");
    const certPath = join(dir, "cert.pem");
    execFileSync(
      "openssl",
      [
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        keyPath,
        "-out",
        certPath,
        "-days",
        "1",
        "-subj",
        `/CN=${hostname}`,
        "-addext",
        `subjectAltName=DNS:${hostname}`,
      ],
      { stdio: "ignore" },
    );
    return {
      key: readFileSync(keyPath, "utf8"),
      cert: readFileSync(certPath, "utf8"),
    };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

describe("CIMD IP address checker", () => {
  it("refuses loopback addresses for IPv4 and IPv6", () => {
    for (const ip of [
      "127.0.0.1",
      "127.0.0.0",
      "127.255.255.254",
      "::1",
      "[::1]",
      "0:0:0:0:0:0:0:1",
      "::ffff:127.0.0.1",
    ]) {
      expect(classifyIpAddress(ip), ip).toBe("loopback");
      expect(isAllowedCimdAddress(ip), ip).toBe(false);
    }
  });

  it("refuses RFC 1918 and RFC 4193 private addresses for IPv4 and IPv6", () => {
    for (const ip of [
      "10.0.0.1",
      "10.255.255.254",
      "172.16.0.1",
      "172.31.255.254",
      "192.168.0.1",
      "192.168.255.254",
      "fc00::1",
      "fd00::1",
      "fd12:3456:789a::1",
      "fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
      "fec0::1",
      "::ffff:10.0.0.1",
      "::ffff:172.16.0.1",
      "::ffff:192.168.1.1",
    ]) {
      expect(classifyIpAddress(ip), ip).toBe("private");
      expect(isAllowedCimdAddress(ip), ip).toBe(false);
    }
  });

  it("refuses link-local addresses for IPv4 and IPv6", () => {
    for (const ip of [
      "169.254.0.1",
      "169.254.169.254",
      "169.254.255.254",
      "fe80::1",
      "fe80::dead:beef",
      "febf:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
      "::ffff:169.254.169.254",
    ]) {
      expect(classifyIpAddress(ip), ip).toBe("link-local");
      expect(isAllowedCimdAddress(ip), ip).toBe(false);
    }
  });

  it("refuses CGNAT (100.64.0.0/10) addresses", () => {
    for (const ip of [
      "100.64.0.0",
      "100.64.0.1",
      "100.100.100.100",
      "100.127.255.254",
      "100.127.255.255",
      "::ffff:100.64.0.1",
    ]) {
      expect(classifyIpAddress(ip), ip).toBe("cgnat");
      expect(isAllowedCimdAddress(ip), ip).toBe(false);
    }
    expect(isAllowedCimdAddress("100.63.255.255")).toBe(true);
    expect(isAllowedCimdAddress("100.128.0.0")).toBe(true);
  });

  it("refuses multicast addresses for IPv4 and IPv6", () => {
    for (const ip of [
      "224.0.0.1",
      "239.255.255.250",
      "239.255.255.255",
      "ff00::1",
      "ff02::1",
      "ff0e::1",
      "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
      "::ffff:224.0.0.1",
    ]) {
      expect(classifyIpAddress(ip), ip).toBe("multicast");
      expect(isAllowedCimdAddress(ip), ip).toBe(false);
    }
  });

  it("refuses unspecified addresses for IPv4 and IPv6", () => {
    for (const ip of [
      "0.0.0.0",
      "0.0.0.1",
      "0.255.255.255",
      "::",
      "[::]",
      "0:0:0:0:0:0:0:0",
      "::ffff:0.0.0.0",
    ]) {
      expect(classifyIpAddress(ip), ip).toBe("unspecified");
      expect(isAllowedCimdAddress(ip), ip).toBe(false);
    }
  });

  it("refuses reserved, tunnel, and malformed addresses while allowing public IPs", () => {
    for (const ip of [
      "240.0.0.1",
      "255.255.255.255",
      "192.0.0.1",
      "198.18.0.1",
      "192.0.2.1",
      "198.51.100.1",
      "203.0.113.1",
      "192.88.99.1",
      "2001:20::1",
      "3fff::1",
      "5f00::1",
      "2001:db8::1",
      "2001::1",
      "64:ff9b::127.0.0.1",
      "2002:7f00:1::1",
      "fe80::1%eth0",
      "localhost",
      "",
    ]) {
      expect(isAllowedCimdAddress(ip), ip).toBe(false);
    }

    for (const ip of [
      "93.184.216.34",
      "1.1.1.1",
      "8.8.8.8",
      "172.15.255.255",
      "172.32.0.0",
      "192.167.255.255",
      "192.169.0.0",
      "2606:2800:220:1:248:1893:25c8:1946",
      "2606:4700:4700::1111",
    ]) {
      expect(classifyIpAddress(ip), ip).toBe("public");
      expect(isAllowedCimdAddress(ip), ip).toBe(true);
    }
  });
});

describe("CIMD URL validation and fetch policy", () => {
  it("does not persist the untrusted metadata passthrough", () => {
    const id = "https://client.example/client.json";
    const document = validateCimdMetadataDocument(
      {
        client_id: id,
        redirect_uris: [REDIRECT_URI],
        metadata: { arbitrary: "x".repeat(8_000) },
      },
      id,
    );
    expect(document).not.toHaveProperty("metadata");
  });
  it("rejects unsupported remote key sets", () => {
    const id = "https://client.example/client.json";
    expect(() =>
      validateCimdMetadataDocument(
        {
          client_id: id,
          redirect_uris: [REDIRECT_URI],
          token_endpoint_auth_method: "private_key_jwt",
          jwks_uri: "https://client.example/jwks.json",
        },
        id,
      ),
    ).toThrow(/jwks_uri is not supported/);
  });
  it("rejects noncanonical identifiers and nonstandard ports", () => {
    for (const id of [
      "https://example.com/a/%2e%2e/client.json",
      "https://example.com/a\\..\\client.json",
      "https://EXAMPLE.com/client.json",
      "https://example.com:8443/client.json",
    ])
      expect(isCimdClientId(id), id).toBe(false);
  });
  const host = "cimd.example";
  const tls = generateTestCertificate(host);
  let server: https.Server;
  let port = 0;
  let lastRequestHeaders: Record<string, string | string[] | undefined> = {};
  let redirectTargetRequested = false;

  beforeAll(async () => {
    server = https.createServer({ key: tls.key, cert: tls.cert }, (req, res) => {
      lastRequestHeaders = req.headers;
      const url = new URL(req.url ?? "/", `https://${host}`);
      const selfUrl = `https://${host}${url.pathname}`;

      if (url.pathname === "/valid/client.json") {
        res.writeHead(200, {
          "content-type": "application/json; charset=utf-8",
          "cache-control": "public, max-age=120",
        });
        res.end(
          JSON.stringify({
            client_id: selfUrl,
            client_name: "TLS Test Client",
            redirect_uris: [REDIRECT_URI],
            token_endpoint_auth_method: "none",
            grant_types: ["authorization_code", "refresh_token"],
            response_types: ["code"],
          }),
        );
        return;
      }

      if (url.pathname === "/mismatch/client.json") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(
          JSON.stringify({
            client_id: `https://${host}/other/client.json`,
            client_name: "Mismatched Client",
            redirect_uris: [REDIRECT_URI],
            token_endpoint_auth_method: "none",
          }),
        );
        return;
      }

      if (url.pathname === "/oversized-header/client.json") {
        res.writeHead(200, {
          "content-type": "application/json",
          "content-length": String(CIMD_MAX_BODY_BYTES + 1),
        });
        res.end(
          JSON.stringify({
            client_id: selfUrl,
            redirect_uris: [REDIRECT_URI],
          }),
        );
        return;
      }

      if (url.pathname === "/oversized-stream/client.json") {
        res.writeHead(200, { "content-type": "application/json" });
        res.write('{"client_id":"');
        res.write("x".repeat(CIMD_MAX_BODY_BYTES + 256));
        res.end('"}');
        return;
      }

      if (url.pathname === "/redirect/client.json") {
        res.writeHead(302, {
          location: `https://${host}/redirect-target/client.json`,
          "content-type": "application/json",
        });
        res.end();
        return;
      }

      if (url.pathname === "/redirect-target/client.json") {
        redirectTargetRequested = true;
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ client_id: selfUrl, redirect_uris: [REDIRECT_URI] }));
        return;
      }

      if (url.pathname === "/wrong-content-type/client.json") {
        res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
        res.end(
          JSON.stringify({
            client_id: selfUrl,
            redirect_uris: [REDIRECT_URI],
          }),
        );
        return;
      }

      if (url.pathname === "/slow/client.json") {
        setTimeout(() => {
          if (!res.destroyed) {
            res.writeHead(200, { "content-type": "application/json" });
            res.end(JSON.stringify({ client_id: selfUrl, redirect_uris: [REDIRECT_URI] }));
          }
        }, 250);
        return;
      }

      res.writeHead(404, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: "not_found" }));
    });

    await new Promise<void>((resolve) => {
      server.listen(0, "127.0.0.1", () => {
        port = (server.address() as AddressInfo).port;
        resolve();
      });
    });
  });

  afterAll(async () => {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  });

  it("requires https URLs with a non-root path, no credentials, and no fragment", () => {
    expect(isCimdClientId("https://client.example/oauth/metadata.json")).toBe(true);
    expect(isCimdClientId("http://client.example/oauth/metadata.json")).toBe(false);
    expect(isCimdClientId("https://client.example/")).toBe(false);
    expect(isCimdClientId("https://client.example")).toBe(false);
    expect(isCimdClientId("https://user:pass@client.example/oauth/metadata.json")).toBe(false);
    expect(isCimdClientId("https://client.example/oauth/metadata.json#section")).toBe(false);
    expect(isCimdClientId("https://client.example/oauth/../metadata.json")).toBe(false);
    expect(isCimdClientId("dcr-client-id-12345")).toBe(false);
  });

  it("accepts a valid document over HTTPS and sends no credentials or cookies", async () => {
    let resolvedHostname = "";
    const fetcher = createCimdFetcher({
      lookup: async (hostname) => {
        resolvedHostname = hostname;
        return [{ address: "93.184.216.34", family: 4 }];
      },
      https: {
        ca: tls.cert,
        connectHost: "127.0.0.1",
        connectPort: port,
      },
    });

    const clientId = `https://${host}/valid/client.json`;
    const result = await fetcher.fetchDocument(clientId);
    expect(resolvedHostname).toBe(host);
    expect(result.fromCache).toBe(false);
    expect(result.metadata.client_id).toBe(clientId);
    expect(result.metadata.client_name).toBe("TLS Test Client");
    expect(result.metadata.redirect_uris).toEqual([REDIRECT_URI]);
    expect(lastRequestHeaders.cookie).toBeUndefined();
    expect(lastRequestHeaders.authorization).toBeUndefined();
  });

  it("refuses DNS results that resolve to forbidden IP addresses before connecting", async () => {
    for (const forbidden of [
      { address: "127.0.0.1", family: 4 as const },
      { address: "::1", family: 6 as const },
      { address: "10.0.0.1", family: 4 as const },
      { address: "172.16.0.1", family: 4 as const },
      { address: "192.168.1.1", family: 4 as const },
      { address: "fc00::1", family: 6 as const },
      { address: "169.254.169.254", family: 4 as const },
      { address: "fe80::1", family: 6 as const },
      { address: "100.64.0.1", family: 4 as const },
      { address: "224.0.0.1", family: 4 as const },
      { address: "ff02::1", family: 6 as const },
      { address: "0.0.0.0", family: 4 as const },
      { address: "::", family: 6 as const },
    ]) {
      const fetcher = createCimdFetcher({
        lookup: async () => [forbidden],
        https: { ca: tls.cert, connectHost: "127.0.0.1", connectPort: port },
      });
      await expect(
        fetcher.fetchDocument(`https://${host}/valid/client.json`),
      ).rejects.toMatchObject({ code: "forbidden_address" } satisfies Partial<CimdError>);
    }

    // Even a mixed DNS answer containing one public IP and one private IP is refused.
    const mixedFetcher = createCimdFetcher({
      lookup: async () => [
        { address: "93.184.216.34", family: 4 },
        { address: "127.0.0.1", family: 4 },
      ],
      https: { ca: tls.cert, connectHost: "127.0.0.1", connectPort: port },
    });
    await expect(
      mixedFetcher.fetchDocument(`https://${host}/valid/client.json`),
    ).rejects.toMatchObject({ code: "forbidden_address" } satisfies Partial<CimdError>);
  });

  it("refuses a document whose client_id does not match the fetched URL", async () => {
    const fetcher = createCimdFetcher({
      lookup: async () => [{ address: "93.184.216.34", family: 4 }],
      https: { ca: tls.cert, connectHost: "127.0.0.1", connectPort: port },
    });
    await expect(
      fetcher.fetchDocument(`https://${host}/mismatch/client.json`),
    ).rejects.toMatchObject({ code: "client_id_mismatch" } satisfies Partial<CimdError>);
  });

  it("refuses oversized responses exceeding 16 KiB", async () => {
    const fetcher = createCimdFetcher({
      lookup: async () => [{ address: "93.184.216.34", family: 4 }],
      https: { ca: tls.cert, connectHost: "127.0.0.1", connectPort: port },
    });
    await expect(
      fetcher.fetchDocument(`https://${host}/oversized-header/client.json`),
    ).rejects.toMatchObject({ code: "response_too_large" } satisfies Partial<CimdError>);
    await expect(
      fetcher.fetchDocument(`https://${host}/oversized-stream/client.json`),
    ).rejects.toMatchObject({ code: "response_too_large" } satisfies Partial<CimdError>);
  });

  it("enforces the request timeout", async () => {
    expect(CIMD_TIMEOUT_MS).toBe(5_000);
    const fetcher = createCimdFetcher({
      timeoutMs: 50,
      lookup: async () => [{ address: "93.184.216.34", family: 4 }],
      https: { ca: tls.cert, connectHost: "127.0.0.1", connectPort: port },
    });
    await expect(fetcher.fetchDocument(`https://${host}/slow/client.json`)).rejects.toMatchObject({
      code: "timeout",
    } satisfies Partial<CimdError>);
  });

  it("bounds a DNS lookup that never returns", async () => {
    const fetcher = createCimdFetcher({ timeoutMs: 30, lookup: () => new Promise(() => {}) });
    await expect(
      fetcher.fetchDocument("https://hanging.example/client.json"),
    ).rejects.toMatchObject({ code: "timeout" });
  });

  it("coalesces simultaneous requests and caches failures", async () => {
    let calls = 0;
    const fetcher = createCimdFetcher({
      lookup: async () => [{ address: "93.184.216.34", family: 4 }],
      transport: async () => {
        calls++;
        await new Promise((resolve) => setTimeout(resolve, 10));
        return { status: 503, headers: {}, body: "" };
      },
    });
    const url = "https://failure.example/client.json";
    await Promise.all([
      expect(fetcher.fetchDocument(url)).rejects.toThrow(),
      expect(fetcher.fetchDocument(url)).rejects.toThrow(),
    ]);
    await expect(fetcher.fetchDocument(url)).rejects.toThrow();
    expect(calls).toBe(1);
  });

  it("caches no-store documents for the minimum lifetime", async () => {
    let calls = 0;
    let time = 0;
    const id = "https://cache.example/client.json";
    const fetcher = createCimdFetcher({
      now: () => time,
      lookup: async () => [{ address: "93.184.216.34", family: 4 }],
      transport: async () => {
        calls++;
        return {
          status: 200,
          headers: { "content-type": "application/json", "cache-control": "no-store" },
          body: JSON.stringify({ client_id: id, redirect_uris: [REDIRECT_URI] }),
        };
      },
    });
    await fetcher.fetchDocument(id);
    time += 59_999;
    await fetcher.fetchDocument(id);
    expect(calls).toBe(1);
    time++;
    await fetcher.fetchDocument(id);
    expect(calls).toBe(2);
  });

  it("caps concurrent outbound resolutions", async () => {
    let release!: () => void;
    let allStarted!: () => void;
    let active = 0;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const started = new Promise<void>((resolve) => {
      allStarted = resolve;
    });
    const fetcher = createCimdFetcher({
      lookup: async () => [{ address: "93.184.216.34", family: 4 }],
      transport: async () => {
        active++;
        if (active === 8) allStarted();
        await gate;
        return { status: 503, headers: {}, body: "" };
      },
    });
    const pending = Array.from({ length: 8 }, (_, i) =>
      fetcher.fetchDocument(`https://capacity.example/${i}`).catch(() => undefined),
    );
    try {
      await started;
      await expect(
        fetcher.fetchDocument("https://capacity.example/overflow"),
      ).rejects.toMatchObject({ code: "network_error" });
    } finally {
      release();
      await Promise.all(pending);
    }
  });

  it("refuses HTTP redirects without following them", async () => {
    redirectTargetRequested = false;
    const fetcher = createCimdFetcher({
      lookup: async () => [{ address: "93.184.216.34", family: 4 }],
      https: { ca: tls.cert, connectHost: "127.0.0.1", connectPort: port },
    });
    await expect(
      fetcher.fetchDocument(`https://${host}/redirect/client.json`),
    ).rejects.toMatchObject({ code: "redirect_not_allowed" } satisfies Partial<CimdError>);
    expect(redirectTargetRequested).toBe(false);
  });

  it("refuses non-application/json content-types", async () => {
    const fetcher = createCimdFetcher({
      lookup: async () => [{ address: "93.184.216.34", family: 4 }],
      https: { ca: tls.cert, connectHost: "127.0.0.1", connectPort: port },
    });
    await expect(
      fetcher.fetchDocument(`https://${host}/wrong-content-type/client.json`),
    ).rejects.toMatchObject({ code: "invalid_content_type" } satisfies Partial<CimdError>);
  });

  it("applies bounded Cache-Control TTLs capped at 24 hours", async () => {
    expect(parseCacheControlTtlMs(undefined)).toBe(CIMD_DEFAULT_CACHE_TTL_MS);
    expect(parseCacheControlTtlMs("public, max-age=60")).toBe(60_000);
    expect(parseCacheControlTtlMs("public, max-age=999999")).toBe(CIMD_MAX_CACHE_TTL_MS);
    expect(parseCacheControlTtlMs("no-store, max-age=60")).toBe(0);
    expect(parseCacheControlTtlMs("no-cache")).toBe(0);
    expect(parseCacheControlTtlMs("max-age=0")).toBe(0);

    let currentTime = 1_000_000;
    let transportCalls = 0;
    let cacheControl = "public, max-age=999999";
    const clientId = "https://client.example/oauth/metadata.json";

    const fetcher = createCimdFetcher({
      now: () => currentTime,
      lookup: async () => [{ address: "93.184.216.34", family: 4 }],
      transport: async (): Promise<CimdRawResponse> => {
        transportCalls += 1;
        return {
          status: 200,
          headers: {
            "content-type": "application/json",
            "cache-control": cacheControl,
          },
          body: JSON.stringify({
            client_id: clientId,
            client_name: `Client v${transportCalls}`,
            redirect_uris: [REDIRECT_URI],
            token_endpoint_auth_method: "none",
          }),
        };
      },
    });

    const first = await fetcher.fetchDocument(clientId);
    expect(first.fromCache).toBe(false);
    expect(first.metadata.client_name).toBe("Client v1");
    expect(transportCalls).toBe(1);

    // Still cached just before the 24-hour cap.
    currentTime += CIMD_MAX_CACHE_TTL_MS - 1;
    const second = await fetcher.fetchDocument(clientId);
    expect(second.fromCache).toBe(true);
    expect(second.metadata.client_name).toBe("Client v1");
    expect(transportCalls).toBe(1);

    // Re-fetched once the 24-hour cap expires.
    currentTime += 1;
    cacheControl = "public, max-age=30";
    const third = await fetcher.fetchDocument(clientId);
    expect(third.fromCache).toBe(false);
    expect(third.metadata.client_name).toBe("Client v2");
    expect(transportCalls).toBe(2);

    // The minimum cache lifetime applies even to short max-age values.
    currentTime += 59_999;
    expect((await fetcher.fetchDocument(clientId)).fromCache).toBe(true);
    expect(transportCalls).toBe(2);

    currentTime += 1;
    expect((await fetcher.fetchDocument(clientId)).fromCache).toBe(false);
    expect(transportCalls).toBe(3);
  });
});

describe("OAuth authorization flow with CIMD client_id", () => {
  const cimdClientId = "https://mcp-client.example/oauth/client-metadata.json";
  const mismatchedClientId = "https://mcp-client.example/oauth/mismatch.json";
  let fetchCount = 0;

  const { app } = createService(undefined, false, {
    lookup: async () => [{ address: "93.184.216.34", family: 4 }],
    transport: async ({ url }): Promise<CimdRawResponse> => {
      fetchCount += 1;
      if (url.href === cimdClientId) {
        return {
          status: 200,
          headers: {
            "content-type": "application/json",
            "cache-control": "public, max-age=300",
          },
          body: JSON.stringify({
            client_id: cimdClientId,
            client_name: "CIMD MCP Client",
            redirect_uris: [REDIRECT_URI],
            token_endpoint_auth_method: "none",
            grant_types: ["authorization_code", "refresh_token"],
            response_types: ["code"],
          }),
        };
      }
      if (url.href === mismatchedClientId) {
        return {
          status: 200,
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            client_id: "https://other.example/client.json",
            client_name: "Spoofed Client",
            redirect_uris: [REDIRECT_URI],
            token_endpoint_auth_method: "none",
          }),
        };
      }
      return {
        status: 404,
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ error: "not_found" }),
      };
    },
  });

  const email = "cimd-user@example.com";
  const password = "correct horse battery staple";
  let userId = "";

  beforeAll(async () => {
    const signUp = await post(app, "/api/auth/sign-up/email", {
      name: "CIMD User",
      email,
      password,
    });
    expect(signUp.status).toBe(200);
    userId = ((await json(signUp)).user as { id: string }).id;
  });

  it("refuses authorization when redirect_uri is outside the CIMD document", async () => {
    const { challenge } = pkce();
    const res = await app.request(
      authorizeUrl(cimdClientId, {
        redirect_uri: "http://127.0.0.1:33418/unlisted-callback",
        code_challenge: challenge,
        code_challenge_method: "S256",
      }),
    );
    expect(res.status).toBe(302);
    const location = new URL(res.headers.get("location") ?? "");
    expect(location.origin).toBe(AUTH);
    expect(location.searchParams.get("error")).toBe("invalid_redirect");
  });

  it("refuses authorization when the CIMD document has a mismatched client_id", async () => {
    const { challenge } = pkce();
    const res = await app.request(
      authorizeUrl(mismatchedClientId, {
        code_challenge: challenge,
        code_challenge_method: "S256",
      }),
    );
    expect(res.status).toBe(302);
    const location = new URL(res.headers.get("location") ?? "");
    expect(location.origin).toBe(AUTH);
    expect(location.searchParams.get("error")).toBe("invalid_client");
  });

  it("completes authorization, consent, token exchange, and refresh for a CIMD client without prior registration", async () => {
    fetchCount = 0;
    const jar = new Jar();
    const { verifier, challenge } = pkce();

    const authorize = await app.request(
      authorizeUrl(cimdClientId, { code_challenge: challenge, code_challenge_method: "S256" }),
    );
    const initialFetchCount = fetchCount;
    expect(authorize.status).toBe(302);
    const signInUrl = authorize.headers.get("location") ?? "";
    expect(signInUrl.startsWith(`${AUTH}/oauth/sign-in?`)).toBe(true);

    const signIn = await post(app, "/api/auth/sign-in/email", {
      email,
      password,
      oauth_query: signedQuery(signInUrl),
    });
    expect(signIn.status).toBe(200);
    jar.store(signIn);
    const afterSignIn = await json(signIn);
    expect(afterSignIn.redirect).toBe(true);
    const consentUrl = afterSignIn.url as string;
    expect(consentUrl.startsWith(`${AUTH}/oauth/consent?`)).toBe(true);

    // Public client metadata endpoint serves the CIMD client_name to the consent page.
    const publicClient = await app.request(
      `${AUTH}/api/auth/oauth2/public-client?client_id=${encodeURIComponent(cimdClientId)}`,
      { headers: jar.header() },
    );
    expect(publicClient.status).toBe(200);
    expect((await json(publicClient)).client_name).toBe("CIMD MCP Client");

    const consent = await post(
      app,
      "/api/auth/oauth2/consent",
      { accept: true, oauth_query: signedQuery(consentUrl) },
      jar.header(),
    );
    expect(consent.status).toBe(200);
    const callback = new URL((await json(consent)).url as string);
    expect(`${callback.origin}${callback.pathname}`).toBe(REDIRECT_URI);
    const code = callback.searchParams.get("code") ?? "";
    expect(code).not.toBe("");

    const issued = await token(app, {
      grant_type: "authorization_code",
      code,
      redirect_uri: REDIRECT_URI,
      client_id: cimdClientId,
      code_verifier: verifier,
      resource: RESOURCE,
    });
    expect(issued.status).toBe(200);
    const tokens = await json(issued);
    expect(tokens.token_type).toBe("Bearer");
    const access = decode(tokens.access_token as string);
    expect(access.payload.sub).toBe(userId);
    expect(access.payload.client_id).toBe(cimdClientId);
    expect(audiences(access.payload.aud)).toContain(RESOURCE);

    const refreshed = await token(app, {
      grant_type: "refresh_token",
      refresh_token: tokens.refresh_token as string,
      client_id: cimdClientId,
      resource: RESOURCE,
    });
    expect(refreshed.status).toBe(200);
    // The cached CIMD document served all follow-up lookups in the flow.
    expect(fetchCount).toBe(initialFetchCount);
  });
});

describe("CIMD registration budget", () => {
  it("applies deny and allow host policies before fetching", async () => {
    let fetches = 0;
    const { app } = createService(
      undefined,
      false,
      {
        lookup: async () => [{ address: "93.184.216.34", family: 4 }],
        transport: async () => {
          fetches++;
          return { status: 404, headers: {}, body: "" };
        },
      },
      {
        ...config,
        cimdAllowedHosts: ["allowed.example", "denied.example"],
        cimdDeniedHosts: ["denied.example"],
      },
    );
    const { challenge } = pkce();
    for (const host of ["denied.example", "other.example"]) {
      const response = await app.request(
        authorizeUrl(`https://${host}/client.json`, {
          code_challenge: challenge,
          code_challenge_method: "S256",
        }),
      );
      expect(new URL(response.headers.get("location") ?? "").searchParams.get("error")).toBe(
        "invalid_client",
      );
    }
    expect(fetches).toBe(0);
  });
  it("shares the dynamic registration budget", async () => {
    let fetches = 0;
    const { app } = createService(undefined, true, {
      lookup: async () => [{ address: "93.184.216.34", family: 4 }],
      transport: async () => {
        fetches++;
        return { status: 404, headers: {}, body: "" };
      },
    });
    for (let i = 0; i < 20; i++) {
      const response = await post(app, "/api/auth/oauth2/register", {
        client_name: `Client ${i}`,
        redirect_uris: [REDIRECT_URI],
        token_endpoint_auth_method: "none",
      });
      expect([200, 201]).toContain(response.status);
    }
    const { challenge } = pkce();
    const response = await app.request(
      authorizeUrl("https://budget.example/client.json", {
        code_challenge: challenge,
        code_challenge_method: "S256",
      }),
    );
    expect(response.status).toBe(302);
    expect(new URL(response.headers.get("location") ?? "").searchParams.get("error")).toBe(
      "invalid_client",
    );
    expect(fetches).toBe(0);
  });
});
