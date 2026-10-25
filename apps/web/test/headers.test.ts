import { createHash } from "node:crypto";
import {
  cloudSources,
  commonHeaders,
  contentSecurityPolicy,
  inlineScriptHashes,
  netlifyHeaders,
  nginxServer,
  pagePaths,
  serverRouteHeaders,
  staticHeaderRules,
} from "../route-headers.mjs";

type Header = { key: string; value: string };

const sha256 = (text: string) => `sha256-${createHash("sha256").update(text).digest("base64")}`;

/** Splits a policy into its directives, keyed by name. */
function directives(policy: string | undefined): Map<string, string[]> {
  const entries = (policy ?? "").split(";").map((directive) => {
    const [name = "", ...sources] = directive.trim().split(/\s+/);
    return [name, sources] as const;
  });
  return new Map(entries);
}

function header(headers: Header[] | undefined, key: string): string | undefined {
  return headers?.find((candidate) => candidate.key === key)?.value;
}

function cspOf(headers: Header[] | undefined): Map<string, string[]> {
  return directives(header(headers, "Content-Security-Policy"));
}

const THEME = "document.documentElement.classList.toggle('dark',true)";
const page = (body: string) =>
  `<head><script>${THEME}</script><script src="/c.js" async=""></script></head>${body}`;
const payload = (n: number) => `self.__next_f.push([${n}])`;
const PAGES = [
  { file: "index.html", html: page(`<script>${payload(0)}</script>`) },
  { file: "app.html", html: page(`<script>${payload(1)}</script>`) },
  { file: "convert/markdown-to-html.html", html: page(`<script>${payload(2)}</script>`) },
  { file: "404.html", html: page(`<script>${payload(3)}</script>`) },
];
const NO_CLOUD = {};
const CLOUD = {
  NEXT_PUBLIC_ARIAD_API_URL: "https://api.example.com/v1/",
  NEXT_PUBLIC_ARIAD_AUTH_URL: "https://auth.example.com/",
  NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY: "0x4AAAAAAA",
  ARIADSHIFT_WEB_STORAGE_ORIGINS: "https://storage.example.com https://*.r2.example.com",
};
const EMPTY = { connect: [], script: [], frame: [] };

describe("inline script hashes", () => {
  it("hashes inline scripts and skips scripts with a src", () => {
    const hashes = inlineScriptHashes(page("<script>run()</script>"));
    expect(hashes).toEqual([sha256(THEME), sha256("run()")].sort());
  });

  it("hashes each distinct script once", () => {
    const hashes = inlineScriptHashes("<script>a()</script><script>a()</script>");
    expect(hashes).toEqual([sha256("a()")]);
  });

  it("normalizes line endings as the HTML parser does", () => {
    const hashes = inlineScriptHashes("<script>a();\r\nb();\rc()</script>");
    expect(hashes).toEqual([sha256("a();\nb();\nc()")]);
  });

  it("does not mistake a data attribute for src", () => {
    const hashes = inlineScriptHashes('<script data-src="x">a()</script>');
    expect(hashes).toEqual([sha256("a()")]);
  });
});

describe("content security policy", () => {
  const hash = sha256("a()");

  it("keeps site pages to their own origin and hashed scripts, with no workers or frames", () => {
    const policy = directives(contentSecurityPolicy("site", { inlineScripts: [hash] }));
    expect(policy.get("default-src")).toEqual(["'self'"]);
    expect(policy.get("script-src")).toEqual(["'self'", `'${hash}'`]);
    expect(policy.get("style-src")).toEqual(["'self'", "'unsafe-inline'"]);
    expect(policy.get("img-src")).toEqual(["'self'"]);
    expect(policy.get("connect-src")).toEqual(["'self'"]);
    expect(policy.get("worker-src")).toEqual(["'none'"]);
    expect(policy.get("frame-src")).toEqual(["'none'"]);
    expect(policy.get("object-src")).toEqual(["'none'"]);
    expect(policy.get("base-uri")).toEqual(["'none'"]);
    expect(policy.get("form-action")).toEqual(["'self'"]);
    expect(policy.get("frame-ancestors")).toEqual(["'none'"]);
  });

  it("lets the converter compile WebAssembly and start same-origin workers, never eval", () => {
    const policy = directives(contentSecurityPolicy("converter", { inlineScripts: [hash] }));
    expect(policy.get("script-src")).toEqual(["'self'", `'${hash}'`, "'wasm-unsafe-eval'"]);
    expect(policy.get("worker-src")).toEqual(["'self'"]);
    expect(policy.get("img-src")).toEqual(["'self'"]);
  });

  it("allows inline scripts only where they cannot be hashed", () => {
    const loose = directives(contentSecurityPolicy("site", { inlineScripts: "unsafe-inline" }));
    expect(loose.get("script-src")).toEqual(["'self'", "'unsafe-inline'"]);
    for (const profile of ["site", "converter"] as const) {
      const strict = directives(contentSecurityPolicy(profile, { inlineScripts: [] }));
      expect(strict.get("script-src")).not.toContain("'unsafe-inline'");
    }
  });

  it("adds eval for next dev only", () => {
    const dev = directives(contentSecurityPolicy("converter", { inlineScripts: [], dev: true }));
    expect(dev.get("script-src")).toContain("'unsafe-eval'");
  });

  it("adds cloud origins to the converter only", () => {
    const cloud = cloudSources(CLOUD);
    const converter = directives(contentSecurityPolicy("converter", { inlineScripts: [], cloud }));
    expect(converter.get("connect-src")).toEqual([
      "'self'",
      "https://api.example.com",
      "https://auth.example.com",
      "https://storage.example.com",
      "https://*.r2.example.com",
    ]);
    expect(converter.get("script-src")).toContain("https://challenges.cloudflare.com");
    expect(converter.get("frame-src")).toEqual(["https://challenges.cloudflare.com"]);
    const site = directives(contentSecurityPolicy("site", { inlineScripts: [], cloud }));
    expect(site.get("connect-src")).toEqual(["'self'"]);
    expect(site.get("frame-src")).toEqual(["'none'"]);
  });
});

describe("cloud sources", () => {
  it("is empty without an API URL, whatever else is set", () => {
    expect(cloudSources(NO_CLOUD)).toEqual(EMPTY);
    expect(cloudSources({ ...CLOUD, NEXT_PUBLIC_ARIAD_API_URL: " " })).toEqual(EMPTY);
  });

  it("uses origins only and skips Turnstile without a site key", () => {
    const sources = cloudSources({
      NEXT_PUBLIC_ARIAD_API_URL: "http://localhost:8080/prefix",
      ARIADSHIFT_WEB_STORAGE_ORIGINS: " http://localhost:8333 ",
    });
    const connect = ["http://localhost:8080", "http://localhost:8333"];
    expect(sources).toEqual({ ...EMPTY, connect });
  });

  it("refuses an API URL that is not an http or https URL", () => {
    expect(() => cloudSources({ NEXT_PUBLIC_ARIAD_API_URL: "not a url" })).toThrow(/not a URL/);
    const ftp = { NEXT_PUBLIC_ARIAD_API_URL: "ftp://api.example.com" };
    expect(() => cloudSources(ftp)).toThrow(/http or https/);
  });

  it("refuses storage entries that are not origins", () => {
    for (const storage of ["https:", "*", "https://storage.example.com/bucket", "'unsafe-eval'"]) {
      const env = {
        NEXT_PUBLIC_ARIAD_API_URL: "https://api.example.com",
        ARIADSHIFT_WEB_STORAGE_ORIGINS: storage,
      };
      expect(() => cloudSources(env)).toThrow(/ARIADSHIFT_WEB_STORAGE_ORIGINS/);
    }
  });
});

describe("next start headers", () => {
  const table = serverRouteHeaders({ env: NO_CLOUD });
  const entry = (source: string) => table.find((candidate) => candidate.source === source);

  it("puts the catch-all first so later entries override it", () => {
    expect(table[0]?.source).toBe("/:path*");
    for (const { key, value } of commonHeaders) {
      expect(header(table[0]?.headers, key)).toBe(value);
    }
    expect(cspOf(table[0]?.headers).get("script-src")).toEqual(["'self'", "'unsafe-inline'"]);
  });

  it("isolates the converter only and gives it the converter policy", () => {
    const isolated = table.filter(({ headers }) => header(headers, "Cross-Origin-Opener-Policy"));
    expect(isolated.map(({ source }) => source)).toEqual(["/app", "/app/:path*"]);
    for (const { headers } of isolated) {
      expect(header(headers, "Cross-Origin-Embedder-Policy")).toBe("credentialless");
      expect(cspOf(headers).get("script-src")).toContain("'wasm-unsafe-eval'");
    }
    expect(cspOf(table[0]?.headers).get("script-src")).not.toContain("'wasm-unsafe-eval'");
  });

  it("lets worker scripts compile WebAssembly and nothing else", () => {
    for (const source of ["/_next/static/chunks/:path*", "/sw.js"]) {
      const policy = cspOf(entry(source)?.headers);
      expect(policy.get("script-src")).toEqual(["'self'", "'wasm-unsafe-eval'"]);
      expect(policy.get("object-src")).toEqual(["'none'"]);
    }
    const chunks = entry("/_next/static/chunks/:path*")?.headers;
    expect(header(chunks, "Cross-Origin-Embedder-Policy")).toBe("credentialless");
    expect(header(entry("/sw.js")?.headers, "Cross-Origin-Embedder-Policy")).toBeUndefined();
  });

  it("adds eval in development", () => {
    const dev = serverRouteHeaders({ env: NO_CLOUD, dev: true });
    expect(cspOf(dev[1]?.headers).get("script-src")).toContain("'unsafe-eval'");
    expect(cspOf(entry("/app")?.headers).get("script-src")).not.toContain("'unsafe-eval'");
  });
});

describe("static export headers", () => {
  const table = staticHeaderRules({ pages: PAGES, env: CLOUD });
  const rule = (source: string) => table.rules.find((candidate) => candidate.source === source);
  const scripts = (source: string) => cspOf(rule(source)?.headers).get("script-src") ?? [];

  it("maps exported files to the paths that serve them", () => {
    expect(pagePaths("index.html")).toEqual(["/", "/index.html"]);
    expect(pagePaths("app.html")).toEqual(["/app", "/app.html"]);
    expect(pagePaths("convert/a-to-b.html")).toEqual(["/convert/a-to-b", "/convert/a-to-b.html"]);
    expect(pagePaths("docs\\index.html")).toEqual(["/docs/", "/docs/index.html"]);
  });

  it("pins each page to its own inline scripts", () => {
    for (const { file, html } of PAGES) {
      const expected = inlineScriptHashes(html).map((hash) => `'${hash}'`);
      for (const source of pagePaths(file)) {
        expect(scripts(source)).toEqual(expect.arrayContaining(expected));
        expect(scripts(source)).not.toContain("'unsafe-inline'");
        expect(scripts(source)).not.toContain("'unsafe-eval'");
      }
    }
    expect(scripts("/")).not.toContain(`'${sha256(payload(1))}'`);
  });

  it("gives the converter its profile and isolation, and other pages neither", () => {
    for (const source of ["/app", "/app.html"]) {
      expect(header(rule(source)?.headers, "Cross-Origin-Opener-Policy")).toBe("same-origin");
      expect(scripts(source)).toContain("'wasm-unsafe-eval'");
      expect(cspOf(rule(source)?.headers).get("connect-src")).toContain("https://api.example.com");
    }
    for (const source of ["/", "/convert/markdown-to-html", "/404"]) {
      expect(header(rule(source)?.headers, "Cross-Origin-Opener-Policy")).toBeUndefined();
      expect(scripts(source)).not.toContain("'wasm-unsafe-eval'");
      expect(cspOf(rule(source)?.headers).get("connect-src")).toEqual(["'self'"]);
    }
  });

  it("never sets one header twice on a path", () => {
    const common = commonHeaders.map(({ key }) => key);
    const seen = new Set<string>();
    for (const { source, headers } of table.rules) {
      expect(seen.has(source)).toBe(false);
      seen.add(source);
      const keys = headers.map(({ key }) => key);
      expect(new Set(keys).size).toBe(keys.length);
      expect(keys.filter((key) => common.includes(key))).toEqual([]);
    }
  });

  it("writes Netlify headers with HSTS on every path and no CSP in the catch-all", () => {
    const blocks = netlifyHeaders(table).split("\n\n");
    const catchAll = blocks[0] ?? "";
    expect(catchAll).toContain("\n/*\n  Referrer-Policy: strict-origin-when-cross-origin\n");
    expect(catchAll).toContain("  Strict-Transport-Security: max-age=31536000");
    expect(catchAll).not.toContain("Content-Security-Policy");
    const app = blocks.find((block) => block.startsWith("/app\n")) ?? "";
    expect(app).toContain("  Cross-Origin-Opener-Policy: same-origin\n");
    expect(app).toContain(`'${sha256(payload(1))}'`);
  });

  it("writes an nginx server whose every location repeats the common headers", () => {
    const conf = nginxServer(table);
    const locations = conf.split("    location ").slice(1);
    expect(locations).toHaveLength(3 + PAGES.length * 2);
    for (const location of locations) {
      for (const { key } of commonHeaders) {
        expect(location).toContain(`add_header ${key} "`);
      }
    }
    const find = (prefix: string) => locations.find((location) => location.startsWith(prefix));
    const app = find("= /app {") ?? "";
    expect(app).toContain('add_header Cross-Origin-Opener-Policy "same-origin" always;');
    expect(app).toContain(`'${sha256(payload(1))}'`);
    expect(app).toContain("try_files /app.html =404;");
    const home = find("= / {") ?? "";
    expect(home).toContain("try_files /index.html =404;");
    expect(home).not.toContain("Cross-Origin-Opener-Policy");
    const chunks = find("^~ /_next/static/chunks/ {") ?? "";
    expect(chunks).toContain('add_header Cross-Origin-Embedder-Policy "credentialless" always;');
    expect(conf).toContain("error_page 404 /404.html;");
    expect(conf).not.toContain("Strict-Transport-Security");
  });

  it("refuses header values nginx would misread", () => {
    for (const value of ['a" always; add_header X-Evil "b', "$host", "a\nb"]) {
      const unsafe = { common: [{ key: "X-Test", value }], rules: [] };
      expect(() => nginxServer(unsafe)).toThrow(/nginx/);
    }
  });
});
