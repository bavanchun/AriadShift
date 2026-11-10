import { expect, test } from "@playwright/test";

test("isolates only the converter route", async ({ request }) => {
  const app = await request.get("/en/app");
  expect(app.headers()["cross-origin-opener-policy"]).toBe("same-origin");
  expect(app.headers()["cross-origin-embedder-policy"]).toBe("credentialless");
  const localized = await request.get("/vi/app");
  expect(localized.headers()["cross-origin-opener-policy"]).toBe("same-origin");

  for (const path of ["/en", "/en/formats", "/en/convert/markdown-to-html", "/vi/quality"]) {
    const response = await request.get(path);
    expect(response.ok()).toBe(true);
    expect(response.headers()["cross-origin-opener-policy"]).toBeUndefined();
    expect(response.headers()["cross-origin-embedder-policy"]).toBeUndefined();
  }
});

test("old unprefixed paths redirect to the default locale", async ({ request }) => {
  for (const [from, to] of [
    ["/app", "/en/app"],
    ["/convert/markdown-to-html", "/en/convert/markdown-to-html"],
    ["/formats", "/en/formats"],
    ["/privacy", "/en/privacy"],
    ["/login", "/en/sign-in"],
    ["/account", "/en/account"],
  ] as const) {
    const response = await request.get(from, { maxRedirects: 0 });
    expect(response.status(), from).toBe(308);
    expect(response.headers().location, from).toBe(to);
  }
});

test("site links load the converter as a cross-origin isolated document and leave it", async ({
  page,
  browserName,
}) => {
  test.skip(
    browserName === "webkit",
    "WebKit does not implement Cross-Origin-Embedder-Policy: credentialless",
  );
  await page.goto("/en");
  expect(await page.evaluate(() => window.crossOriginIsolated)).toBe(false);
  await page
    .getByRole("navigation", { name: "Main" })
    .getByRole("link", { name: "Converter" })
    .click();
  await expect(page).toHaveURL(/\/en\/app$/);
  await expect.poll(() => page.evaluate(() => window.crossOriginIsolated)).toBe(true);
  await page
    .getByRole("navigation", { name: "Main" })
    .getByRole("link", { name: "Formats" })
    .click();
  await expect(page).toHaveURL(/\/en\/formats$/);
  await expect.poll(() => page.evaluate(() => window.crossOriginIsolated)).toBe(false);
});

test("the landing page loads no conversion engine", async ({ page }) => {
  const wasm: string[] = [];
  page.on("request", (request) => {
    if (new URL(request.url()).pathname.endsWith(".wasm")) {
      wasm.push(request.url());
    }
  });
  await page.goto("/en");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(
    "Convert documents on your own device",
  );
  await page.waitForLoadState("networkidle");
  expect(wasm).toEqual([]);
});

test("format-pair pages deep-link into the converter", async ({ page }) => {
  await page.goto("/en/convert/markdown-to-html");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Convert Markdown to HTML");
  await page.getByRole("link", { name: "Convert Markdown to HTML now" }).click();
  await expect(page).toHaveURL(/\/en\/app\?from=markdown&to=html$/);
});

test("serves a localized page with lang, dir, hreflang and a language switcher", async ({
  page,
}) => {
  await page.goto("/vi");
  await expect(page.locator("html")).toHaveAttribute("lang", "vi");
  await expect(page.locator("html")).toHaveAttribute("dir", "ltr");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(
    "Chuyển đổi tài liệu ngay trên thiết bị của bạn",
  );
  await expect(page.locator('link[rel="alternate"][hreflang="en"]')).toHaveAttribute(
    "href",
    /\/en$/,
  );
  await expect(page.locator('link[rel="alternate"][hreflang="x-default"]')).toHaveCount(1);

  await page.goto("/en/formats?x=1#top");
  await page.locator("summary[aria-label^='Language']").click();
  await page.getByRole("link", { name: "Tiếng Việt" }).click();
  await expect(page).toHaveURL(/\/vi\/formats\?x=1#top$/);
  await expect(page.locator("html")).toHaveAttribute("lang", "vi");
});

test("a community locale carries the community badge in the picker", async ({ page }) => {
  await page.goto("/en");
  await page.locator("summary[aria-label^='Language']").click();
  const german = page.getByRole("link", { name: /Deutsch/ });
  await expect(german).toContainText("Community translation");
  await expect(page.getByRole("link", { name: /Tiếng Việt/ })).not.toContainText(
    "Community translation",
  );
});

test("the root page opens a supported locale", async ({ page }) => {
  await page.goto("/", { waitUntil: "commit" });
  await page.waitForURL(/\/en(?:\/|$)/);
  expect(page.url()).toMatch(/\/en$/);
});

test("serves the sitemap with per-locale alternates and the robots file", async ({ request }) => {
  const sitemap = await (await request.get("/sitemap.xml")).text();
  expect(sitemap).toContain("https://ariadshift.ariadnev.com/en/convert/markdown-to-html");
  expect(sitemap).toContain("https://ariadshift.ariadnev.com/vi/convert/markdown-to-html");
  expect(sitemap).toContain('hreflang="vi"');
  expect(sitemap).toContain('hreflang="x-default"');
  const robots = await (await request.get("/robots.txt")).text();
  expect(robots).toContain("Sitemap: https://ariadshift.ariadnev.com/sitemap.xml");
});
