import { expect, test } from "@playwright/test";

test("isolates only the converter route", async ({ request }) => {
  const app = await request.get("/app");
  expect(app.headers()["cross-origin-opener-policy"]).toBe("same-origin");
  expect(app.headers()["cross-origin-embedder-policy"]).toBe("credentialless");

  for (const path of ["/", "/formats", "/convert/markdown-to-html"]) {
    const response = await request.get(path);
    expect(response.ok()).toBe(true);
    expect(response.headers()["cross-origin-opener-policy"]).toBeUndefined();
    expect(response.headers()["cross-origin-embedder-policy"]).toBeUndefined();
  }
});

test("site links load /app as a cross-origin isolated document and leave it", async ({
  page,
  browserName,
}) => {
  test.skip(
    browserName === "webkit",
    "WebKit does not implement Cross-Origin-Embedder-Policy: credentialless",
  );
  await page.goto("/");
  expect(await page.evaluate(() => window.crossOriginIsolated)).toBe(false);
  await page
    .getByRole("navigation", { name: "Main" })
    .getByRole("link", { name: "Converter" })
    .click();
  await expect(page).toHaveURL(/\/app$/);
  await expect.poll(() => page.evaluate(() => window.crossOriginIsolated)).toBe(true);
  await page
    .getByRole("navigation", { name: "Main" })
    .getByRole("link", { name: "Formats" })
    .click();
  await expect(page).toHaveURL(/\/formats$/);
  await expect.poll(() => page.evaluate(() => window.crossOriginIsolated)).toBe(false);
});

test("the landing page loads no conversion engine", async ({ page }) => {
  const wasm: string[] = [];
  page.on("request", (request) => {
    if (request.url().endsWith(".wasm")) {
      wasm.push(request.url());
    }
  });
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(
    "Convert documents on your own device",
  );
  await page.waitForLoadState("networkidle");
  expect(wasm).toEqual([]);
});

test("format-pair pages deep-link into the converter", async ({ page }) => {
  await page.goto("/convert/markdown-to-html");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Convert Markdown to HTML");
  await page.getByRole("link", { name: "Convert Markdown to HTML now" }).click();
  await expect(page).toHaveURL(/\/app\?from=markdown&to=html$/);
});

test("serves the sitemap and robots file", async ({ request }) => {
  const sitemap = await (await request.get("/sitemap.xml")).text();
  expect(sitemap).toContain("https://ariadshift.ariadnev.com/convert/markdown-to-html");
  const robots = await (await request.get("/robots.txt")).text();
  expect(robots).toContain("Sitemap: https://ariadshift.ariadnev.com/sitemap.xml");
});
