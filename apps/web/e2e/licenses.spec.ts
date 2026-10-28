import { existsSync, readFileSync } from "node:fs";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import path from "node:path";
import { expect, test } from "@playwright/test";

const webDir = path.resolve(import.meta.dirname, "..");

function localLinks(html: string): string[] {
  return [...html.matchAll(/href="(\/[^"]+)"/g)]
    .map((match) => match[1] as string)
    .filter((href) => href.startsWith("/notices/") || href.startsWith("/wasm/pandoc/"));
}

test("/licenses links resolve in next start", async ({ request }) => {
  const page = await request.get("/licenses");
  expect(page.ok()).toBe(true);
  const html = await page.text();
  const links = localLinks(html);
  expect(links).toEqual(
    expect.arrayContaining([
      "/wasm/pandoc/COPYING.md",
      "/wasm/pandoc/COPYRIGHT",
      "/wasm/pandoc/SOURCE.txt",
      "/notices/NOTICE.txt",
      "/notices/THIRD_PARTY_LICENSES.txt",
      "/notices/THIRD_PARTY_JS_LICENSES.txt",
    ]),
  );

  for (const href of links) {
    const response = await request.get(href);
    expect(response.status(), href).toBe(200);
    if (href.endsWith(".txt")) {
      expect(response.headers()["content-type"], href).toMatch(/^text\/plain\b/);
    }
    expect((await response.text()).length, href).toBeGreaterThan(0);
  }
});

test("/licenses links resolve in the static export when out/ is present", async () => {
  const outDir = path.join(webDir, "out");
  test.skip(!existsSync(path.join(outDir, "licenses.html")), "static export out/ is not built");

  const server = createServer((req, res) => {
    const urlPath = new URL(req.url ?? "/", "http://127.0.0.1").pathname;
    const candidates = [
      path.join(outDir, urlPath),
      path.join(outDir, `${urlPath}.html`),
      path.join(outDir, urlPath, "index.html"),
    ];
    const target = candidates.find((candidate) => {
      try {
        return (
          candidate.startsWith(outDir) &&
          existsSync(candidate) &&
          readFileSync(candidate).length >= 0
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
    const page = await fetch(`http://127.0.0.1:${port}/licenses`);
    expect(page.status).toBe(200);
    const html = await page.text();
    const links = localLinks(html);
    expect(links).toEqual(
      expect.arrayContaining([
        "/wasm/pandoc/COPYING.md",
        "/wasm/pandoc/COPYRIGHT",
        "/wasm/pandoc/SOURCE.txt",
        "/notices/NOTICE.txt",
        "/notices/THIRD_PARTY_LICENSES.txt",
        "/notices/THIRD_PARTY_JS_LICENSES.txt",
      ]),
    );
    for (const href of links) {
      const response = await fetch(`http://127.0.0.1:${port}${href}`);
      expect(response.status, href).toBe(200);
      if (href.endsWith(".txt")) {
        expect(response.headers.get("content-type"), href).toMatch(/^text\/plain\b/);
      }
      expect((await response.text()).length, href).toBeGreaterThan(0);
    }
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});
