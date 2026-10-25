import { expect, test } from "@playwright/test";

test("activation retires obsolete engine assets and retains current chunks", async ({ page }) => {
  await page.goto("/app");
  await page.waitForFunction(() => navigator.serviceWorker.controller !== null);
  const current = await page.evaluate(async () => {
    const script = await (await fetch("/sw.js")).text();
    const manifest = /^self\.ENGINE_URLS = (.*);$/m.exec(script)?.[1];
    if (!manifest) throw new Error("Service worker has no engine manifest");
    const assets = JSON.parse(manifest) as string[];
    const asset = assets.find((url) => url.startsWith("/_next/static/") && url.endsWith(".js"));
    if (!asset) throw new Error("No current JavaScript asset in the manifest");
    const url = new URL(asset, location.href).href;
    const response = await fetch(url);
    if (!response.ok) throw new Error("Current asset could not be fetched");
    const cache = await caches.open("ariadshift-engines-v1");
    for (const key of [
      url,
      "/wasm/pandoc/pandoc.wasm?v=retired",
      "/_next/static/media/ariad_typst-retired.wasm",
      "/_next/static/chunks/retired-worker.js",
    ])
      await cache.put(key, response.clone());
    const keys = (await cache.keys()).map((key) => key.url);
    if (keys.length !== 4 || !keys.includes(url)) throw new Error("Engine cache was not seeded");
    return url;
  });
  await page.evaluate(async () => {
    const registration = await navigator.serviceWorker.register("/sw.js?cache-refresh", {
      scope: "/",
    });
    const worker = registration.installing ?? registration.waiting ?? registration.active;
    if (!worker?.scriptURL.endsWith("/sw.js?cache-refresh"))
      throw new Error("The updated service worker was not installed");
    await new Promise<void>((resolve, reject) => {
      const changed = () => {
        if (worker.state === "activated") resolve();
        else if (worker.state === "redundant") reject(new Error("Worker activation failed"));
      };
      worker.addEventListener("statechange", changed);
      changed();
    });
  });
  await expect
    .poll(() =>
      page.evaluate(async () =>
        (await (await caches.open("ariadshift-engines-v1")).keys()).map((key) => key.url),
      ),
    )
    .toEqual([current]);
});

test("the converter works offline after the first visit", async ({
  page,
  context,
  browserName,
}) => {
  test.skip(browserName !== "chromium", "service worker offline emulation is reliable in Chromium");

  await page.goto("/app");
  await expect(page.getByTestId("device-indicator")).toContainText("browser storage");
  // The service worker caches the page, its assets, the worker and the wasm module.
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const keys = await (await caches.open("ariadshift-app-v1")).keys();
        return keys.map((request) => new URL(request.url).pathname);
      }),
    )
    .toEqual(expect.arrayContaining(["/app", expect.stringMatching(/\.wasm$/)]));

  await context.setOffline(true);
  await page.reload();
  await expect(page.getByRole("heading", { name: "Convert a document" })).toBeVisible();

  const chooser = page.waitForEvent("filechooser");
  await page.getByRole("button", { name: "Choose a file" }).click();
  await (await chooser).setFiles({
    name: "offline.md",
    mimeType: "text/markdown",
    buffer: Buffer.from("# Offline\n\nStill converting.\n"),
  });
  await expect(page.getByTestId("detected-format")).toHaveText("Markdown");
  await page.getByTestId("target-select").click();
  await page.getByRole("option", { name: "HTML" }).click();
  await page.getByRole("button", { name: "Convert", exact: true }).click();
  await expect(page.getByTestId("output-name")).toHaveText("offline.html");
  await context.setOffline(false);
});
