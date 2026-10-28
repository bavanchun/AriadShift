import { existsSync, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import path from "node:path";
import { expect, test } from "@playwright/test";

const webDir = path.resolve(import.meta.dirname, "..");

function noticeLinks(html: string): string[] {
  return [...html.matchAll(/href="((?:notices|wasm\/pandoc)\/[^"]+)"/g)].map(
    (match) => match[1] as string,
  );
}

test("/licenses links resolve in next start", async ({ request }) => {
  const page = await request.get("/licenses");
  expect(page.ok()).toBe(true);
  const html = await page.text();
  const links = noticeLinks(html);
  expect(links).toEqual(
    expect.arrayContaining([
      "wasm/pandoc/COPYING.md",
      "wasm/pandoc/COPYRIGHT",
      "wasm/pandoc/SOURCE.txt",
      "notices/NOTICE.txt",
      "notices/THIRD_PARTY_LICENSES.txt",
      "notices/THIRD_PARTY_JS_LICENSES.txt",
    ]),
  );

  for (const href of links) {
    const resolvedPath = new URL(href, "http://127.0.0.1:3100/licenses").pathname;
    const response = await request.get(resolvedPath);
    expect(response.status(), resolvedPath).toBe(200);
    if (resolvedPath.endsWith(".txt")) {
      expect(response.headers()["content-type"], resolvedPath).toMatch(/^text\/plain\b/);
    }
    expect((await response.text()).length, resolvedPath).toBeGreaterThan(0);
  }
});

test("/licenses links resolve in the static export at root and under a path prefix", async () => {
  const outDir = path.join(webDir, "out");
  expect(
    existsSync(path.join(outDir, "licenses.html")),
    "static export out/licenses.html must exist; run `pnpm --filter @ariadshift/web export` before e2e",
  ).toBe(true);

  const prefix = "/AriadShift";
  const server = createServer((req, res) => {
    const rawPath = new URL(req.url ?? "/", "http://127.0.0.1").pathname;
    const urlPath = rawPath.startsWith(`${prefix}/`)
      ? rawPath.slice(prefix.length)
      : rawPath === prefix
        ? "/"
        : rawPath;
    const candidates = [
      path.join(outDir, urlPath),
      path.join(outDir, `${urlPath}.html`),
      path.join(outDir, urlPath, "index.html"),
    ];
    const target = candidates.find((candidate) => {
      try {
        return (
          candidate.startsWith(outDir) && existsSync(candidate) && statSync(candidate).isFile()
        );
      } catch {
        return false;
      }
    });
    if (!target) {
      res.writeHead(404);
      res.end();
      return;
    }
    const contentType = target.endsWith(".html")
      ? "text/html; charset=utf-8"
      : target.endsWith(".txt")
        ? "text/plain; charset=utf-8"
        : "application/octet-stream";
    res.writeHead(200, { "content-type": contentType });
    res.end(readFileSync(target));
  });

  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address() as AddressInfo;
  try {
    for (const basePagePath of ["/licenses", `${prefix}/licenses`]) {
      const pageUrl = `http://127.0.0.1:${port}${basePagePath}`;
      const page = await fetch(pageUrl);
      expect(page.status, pageUrl).toBe(200);
      const html = await page.text();
      const links = noticeLinks(html);
      expect(links).toEqual(
        expect.arrayContaining([
          "wasm/pandoc/COPYING.md",
          "wasm/pandoc/COPYRIGHT",
          "wasm/pandoc/SOURCE.txt",
          "notices/NOTICE.txt",
          "notices/THIRD_PARTY_LICENSES.txt",
          "notices/THIRD_PARTY_JS_LICENSES.txt",
        ]),
      );
      for (const href of links) {
        const targetUrl = new URL(href, pageUrl).toString();
        const response = await fetch(targetUrl);
        expect(response.status, targetUrl).toBe(200);
        if (targetUrl.endsWith(".txt")) {
          expect(response.headers.get("content-type"), targetUrl).toMatch(/^text\/plain\b/);
        }
        expect((await response.text()).length, targetUrl).toBeGreaterThan(0);
      }
    }
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});
