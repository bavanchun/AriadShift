import { expect, test } from "@playwright/test";

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
