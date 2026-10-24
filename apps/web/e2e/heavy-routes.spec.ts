import { readFile } from "node:fs/promises";
import path from "node:path";
import { expect, type Page, test } from "@playwright/test";

test.setTimeout(120_000);

const fixture = (name: string) => path.resolve(import.meta.dirname, "../../../fixtures", name);
const MARKDOWN =
  "# Browser document\n\nTiếng Việt: khu vườn yên tĩnh.\n\n- first\n- second\n\n| Name | Value |\n| --- | --- |\n| a | 1 |\n";
async function pick(
  page: Page,
  name: string,
  buffer: Buffer,
  mimeType = "application/octet-stream",
) {
  await page.locator("input[type=file]").setInputFiles({ name, buffer, mimeType });
  await expect(page.getByTestId("input-name")).toHaveText(name);
}
async function convert(page: Page, label: string) {
  await page.getByTestId("target-select").click();
  await page.getByRole("option", { name: label, exact: true }).click();
  await expect(page.getByTestId("route")).toBeVisible();
  await page.getByRole("button", { name: "Convert", exact: true }).click();
  await expect(page.getByTestId("result")).toBeVisible({ timeout: 120_000 });
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: /^Download / }).click();
  return readFile(await (await download).path());
}
async function reset(page: Page) {
  await page.getByRole("button", { name: "Start over and delete files" }).click();
  await expect(page.locator("input[type=file]")).toBeAttached();
}

test("reads a real DOCX entirely in the browser", async ({ page }) => {
  await page.goto("/app");
  await pick(page, "route.docx", await readFile(fixture("docx/vi-route-table.docx")));
  const output = await convert(page, "Markdown");
  expect(output.toString()).toContain("Bảng hành trình ven sông");
  expect(output.toString()).toContain("|");
});

test("writes DOCX then reads its text and structure back", async ({ page }) => {
  await page.goto("/app");
  await pick(page, "browser.md", Buffer.from(MARKDOWN));
  const output = await convert(page, "Word (DOCX)");
  expect(output.subarray(0, 2).toString()).toBe("PK");
  await reset(page);
  await pick(page, "browser.docx", output);
  const back = (await convert(page, "Markdown")).toString();
  expect(back).toContain("# Browser document");
  expect(back).toContain("Tiếng Việt: khu vườn yên tĩnh.");
  expect(back).toContain("| Name");
});

test("reads EPUB with its embedded image into HTML", async ({ page }) => {
  await page.goto("/app");
  await pick(page, "book.epub", await readFile(fixture("epub/vi-epub-table-image.epub")));
  const output = (await convert(page, "HTML")).toString();
  expect(output).toContain("<table>");
  expect(output).toContain("data:image/png;base64,");
});

test("writes EPUB from HTML and reads it back", async ({ page }) => {
  await page.goto("/app");
  await pick(
    page,
    "book.html",
    Buffer.from("<!doctype html><h1>Browser book</h1><p>Hello <strong>world</strong>.</p>"),
  );
  const output = await convert(page, "EPUB");
  expect(output.subarray(0, 2).toString()).toBe("PK");
  await reset(page);
  await pick(page, "book.epub", output);
  const back = (await convert(page, "HTML")).toString();
  expect(back).toContain("Browser book");
  expect(back).toContain("<strong>world</strong>");
});

test("reads the text of a digital PDF", async ({ page }) => {
  await page.goto("/app");
  await pick(page, "map.pdf", await readFile(fixture("pdf/en-map-room.pdf")));
  expect((await convert(page, "Markdown")).toString()).toContain("Map Room");
});

test("loads Typst only for PDF output and preserves Vietnamese text", async ({ page }) => {
  const requests: string[] = [];
  page.on("request", (request) => requests.push(request.url()));
  await page.goto("/app");
  await pick(page, "browser.md", Buffer.from(MARKDOWN));
  await convert(page, "HTML");
  expect(requests.some((url) => /ariad_typst.*wasm|pandoc\.wasm/.test(url))).toBe(false);
  const output = await convert(page, "PDF");
  expect(output.subarray(0, 5).toString()).toBe("%PDF-");
  expect(requests.some((url) => /ariad_typst.*wasm/.test(url))).toBe(true);
  await reset(page);
  await pick(page, "browser.pdf", output);
  const back = (await convert(page, "Markdown")).toString();
  expect(back).toContain("Browser document");
  expect(back).toContain("Tiếng Việt");
});

test("offers OCR through Desktop without offering cloud processing", async ({ page }) => {
  await page.goto("/app?to=docx&profile=editable");
  await pick(page, "scan.pdf", await readFile(fixture("pdf/vi-garden-image-only.pdf")));
  await expect(page.getByTestId("desktop-handoff")).toContainText("Some pages need OCR");
  await expect(page.getByRole("link", { name: "Open in Desktop" })).toHaveAttribute(
    "href",
    "ariadshift://open?route=pdf-docx&profile=editable",
  );
  await expect(page.getByText("Process in Cloud")).toHaveCount(0);
});

for (const [source, target, targetLabel] of [
  ["docx", "epub", "EPUB"],
  ["epub", "docx", "Word (DOCX)"],
] as const) {
  test(`converts ${source} to ${target} through IR`, async ({ page }) => {
    await page.goto("/app");
    const input = source === "docx" ? "docx/vi-route-table.docx" : "epub/vi-epub-chapters.epub";
    await pick(page, `document.${source}`, await readFile(fixture(input)));
    const output = await convert(page, targetLabel);
    expect(output.subarray(0, 2).toString()).toBe("PK");
    await reset(page);
    await pick(page, `document.${target}`, output);
    const back = (await convert(page, "Markdown")).toString();
    expect(back).toContain(source === "docx" ? "Bảng hành trình ven sông" : "#");
  });
}

test("reuses the cached Pandoc engine and lazy chunks offline", async ({
  page,
  context,
  browserName,
}) => {
  test.skip(
    browserName !== "chromium",
    "Chromium supports reliable service worker offline emulation",
  );
  await page.goto("/app");
  await pick(page, "offline.md", Buffer.from(MARKDOWN));
  const output = await convert(page, "Word (DOCX)");
  const resources = await page.workers()[0]?.evaluate(() =>
    performance
      .getEntriesByType("resource")
      .map((entry) => entry.name)
      .filter((name) => name.includes("/_next/static/") && name.endsWith(".js")),
  );
  expect(resources?.length).toBeGreaterThan(0);
  await expect
    .poll(() =>
      page.evaluate(async () =>
        (await (await caches.open("ariadshift-app-v1")).keys()).map((key) => key.url),
      ),
    )
    .toEqual(expect.arrayContaining(resources ?? []));
  await expect
    .poll(() =>
      page.evaluate(async () =>
        (await (await caches.open("ariadshift-engines-v1")).keys()).map((key) => key.url),
      ),
    )
    .toEqual(expect.arrayContaining(resources ?? []));
  await context.setOffline(true);
  await page.reload();
  await expect(page.getByTestId("device-indicator")).toContainText(/browser storage|In memory/);
  await pick(page, "offline.docx", output);
  expect((await convert(page, "Markdown")).toString()).toContain("Browser document");
  await context.setOffline(false);
});
