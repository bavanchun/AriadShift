import { expect, test } from "@playwright/test";

const API = process.env.NEXT_PUBLIC_ARIAD_API_URL?.replace(/\/+$/, "");

test.setTimeout(120_000);
test.skip(!API, "needs a build with NEXT_PUBLIC_ARIAD_API_URL set");

test("choosing cloud while offline before the cloud chunk loaded falls back to this device", async ({
  page,
  context,
  browserName,
}) => {
  test.skip(browserName !== "chromium", "offline chunk failure is tested in Chromium");

  await page.goto("/app");
  await expect(page.getByRole("heading", { name: "Convert a document" })).toBeVisible();
  await expect(page.getByTestId("device-indicator")).toContainText(/browser storage|In memory/, {
    timeout: 60_000,
  });
  await expect
    .poll(
      () =>
        page.evaluate(async () => {
          const keys = await (await caches.open("ariadshift-app-v1")).keys();
          return keys.map((request) => new URL(request.url).pathname);
        }),
      { timeout: 30_000 },
    )
    .toEqual(expect.arrayContaining(["/app", expect.stringMatching(/\.wasm$/)]));

  const chooser = page.waitForEvent("filechooser");
  await page.getByRole("button", { name: "Choose a file" }).click();
  await (await chooser).setFiles({
    name: "offline.md",
    mimeType: "text/markdown",
    buffer: Buffer.from("# Offline\n\nStill on this device.\n"),
  });
  await expect(page.getByTestId("detected-format")).toHaveText("Markdown");

  const host = new URL(API as string).host;
  await context.setOffline(true);
  try {
    await page.getByRole("radio", { name: new RegExp(`On ${host.replace(/\./g, "\\.")}`) }).check();
    const fallback = page.getByTestId("cloud-load-error");
    await expect(fallback).toBeVisible();
    await expect(fallback).toContainText("Cloud mode could not load");

    await fallback.getByRole("button", { name: "Convert on this device" }).click();
    await expect(fallback).toBeHidden();
    await expect(page.getByRole("radio", { name: /On this device/ })).toBeChecked();

    await page.getByTestId("target-select").click();
    await page.getByRole("option", { name: "HTML" }).click();
    await page.getByRole("button", { name: "Convert", exact: true }).click();
    await expect(page.getByTestId("output-name")).toHaveText("offline.html");
  } finally {
    await context.setOffline(false);
  }
});
