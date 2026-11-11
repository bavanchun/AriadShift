import { readFile } from "node:fs/promises";
import { expect, type Page, test } from "@playwright/test";
import { declaredPageCount, makePdf } from "./pdf-fixtures";

test.describe.configure({ timeout: 180_000 });

const ENGINE_BOOT = { timeout: 90_000 };

const workspace = (page: Page) => page.locator('[data-slot="document-workspace"]');
const inspector = (page: Page) => page.locator('[data-slot="tool-inspector"]');
const result = (page: Page) => page.locator('[data-slot="tool-result"]');

async function openWorkspace(page: Page) {
  await page.goto("/en/app?view=workspace");
  await expect(page.getByLabel("Choose a tool")).toBeVisible(ENGINE_BOOT);
}

async function addPdfs(page: Page, files: Array<{ name: string; pages: number; label: string }>) {
  await workspace(page)
    .locator("input[type=file]")
    .setInputFiles(
      files.map((file) => ({
        name: file.name,
        mimeType: "application/pdf",
        buffer: makePdf(file.pages, file.label),
      })),
    );
  await expect(
    page.getByRole("list", { name: "Files in this workspace" }).getByRole("listitem"),
  ).toHaveCount(files.length, ENGINE_BOOT);
  await expect(page.getByText("What is in this PDF", { exact: true }).first()).toBeVisible(
    ENGINE_BOOT,
  );
}

async function chooseTool(page: Page, id: string) {
  await page.getByLabel("Choose a tool").selectOption(id);
  await expect(page.getByLabel("Choose a tool")).toHaveValue(id);
}

async function resultPages(page: Page) {
  const row = result(page).locator("tbody tr", { hasText: "Pages" });
  return Number(((await row.locator("td").last().textContent()) ?? "").trim());
}

async function downloadResult(page: Page): Promise<Buffer> {
  const downloading = page.waitForEvent("download");
  await result(page)
    .getByRole("button", { name: /^Download / })
    .first()
    .click();
  return readFile(await (await downloading).path());
}

const STUDIOS: Array<{ tool: string; slot: string }> = [
  { tool: "pdf.split", slot: "organiser-panel" },
  { tool: "pdf.nup", slot: "imposition-panel" },
  { tool: "pdf.compress", slot: "compress-panel" },
  { tool: "pdf.encrypt", slot: "security-panel" },
  { tool: "pdf.watermark", slot: "watermark-panel" },
  { tool: "pdf.redact", slot: "redact-panel" },
  { tool: "pdf.compare", slot: "compare-panel" },
];

test("opens a purpose-built studio in the inspector for each tool family", async ({ page }) => {
  await openWorkspace(page);
  await addPdfs(page, [
    { name: "first.pdf", pages: 4, label: "A" },
    { name: "second.pdf", pages: 4, label: "B" },
  ]);
  for (const { tool, slot } of STUDIOS) {
    await chooseTool(page, tool);
    await expect(inspector(page).locator(`[data-slot="${slot}"]`), tool).toBeVisible();
  }
});

test("runs N-up through its studio and downloads the combined PDF", async ({ page }) => {
  await openWorkspace(page);
  await addPdfs(page, [{ name: "four.pdf", pages: 4, label: "N" }]);
  await chooseTool(page, "pdf.nup");
  await expect(inspector(page).locator('[data-slot="imposition-panel"]')).toBeVisible();
  // The studio starts at two pages per sheet, so four pages fit on two sheets.
  await page.getByRole("button", { name: "Run tool on this device" }).click();
  await expect(result(page)).toBeVisible(ENGINE_BOOT);
  await expect.poll(() => resultPages(page)).toBe(2);
  const output = await downloadResult(page);
  expect(output.subarray(0, 5).toString("latin1")).toBe("%PDF-");
  expect(declaredPageCount(output)).toBe(2);
});

test("stamps a watermark through its studio and keeps every page", async ({ page }) => {
  await openWorkspace(page);
  await addPdfs(page, [{ name: "three.pdf", pages: 3, label: "W" }]);
  await chooseTool(page, "pdf.watermark");
  const run = page.getByRole("button", { name: "Run tool on this device" });
  await page.getByLabel("Watermark text").fill("DRAFT");
  await expect(run).toBeEnabled();
  await run.click();
  await expect(result(page)).toBeVisible(ENGINE_BOOT);
  await expect.poll(() => resultPages(page)).toBe(3);
  const output = await downloadResult(page);
  expect(output.subarray(0, 5).toString("latin1")).toBe("%PDF-");
  expect(declaredPageCount(output)).toBe(3);
});

test("runs a batch on every file and downloads one zip", async ({ page }) => {
  await openWorkspace(page);
  await addPdfs(page, [
    { name: "one.pdf", pages: 2, label: "X" },
    { name: "two.pdf", pages: 3, label: "Y" },
  ]);
  await chooseTool(page, "pdf.rotate");
  // Two files open the queue by themselves.
  const queue = page.locator('[data-slot="batch-queue"]');
  await expect(queue).toBeVisible();
  await expect(queue.getByText("Queued")).toHaveCount(2);
  await page.getByRole("button", { name: "90° CW" }).click();
  await queue.getByRole("button", { name: "Run batch" }).click();
  await expect(queue.locator('[data-slot="batch-overall"]')).toHaveText("2 of 2 done", ENGINE_BOOT);

  const downloading = page.waitForEvent("download");
  await queue.getByRole("button", { name: "Download all (.zip)" }).click();
  const download = await downloading;
  expect(download.suggestedFilename()).toMatch(/\.zip$/);
  const zip = await readFile(await download.path());
  expect(zip.subarray(0, 2).toString("latin1")).toBe("PK");
  // One stored entry per input file.
  expect(zip.toString("latin1").split("PK\u0003\u0004")).toHaveLength(3);
});

const RECIPE = {
  version: "ariad-recipe/1",
  steps: [
    { tool: "pdf.rotate", params: { angle: 90, pages: "all" } },
    { tool: "pdf.compress", params: {} },
  ],
};

async function openRecipes(page: Page) {
  await page.goto("/en/app?view=recipes");
  await expect(page.locator('[data-slot="recipe-builder"]')).toBeVisible(ENGINE_BOOT);
}

test("builds a recipe visually from the Recipes tab", async ({ page }) => {
  await openRecipes(page);
  await expect(page.getByText("Add a tool to start a recipe.")).toBeVisible();
  await page.getByLabel("Tool to add").selectOption("pdf.merge");
  await page.getByRole("button", { name: "Add step" }).click();
  await expect(page.locator('[data-slot="recipe-step"]')).toHaveCount(1);
  await expect(page.locator('[data-slot="recipe-step"]')).toContainText("pdf.merge");
  await expect(page.getByRole("button", { name: "Share link" })).toBeEnabled();
});

test("holds a #recipe= link behind the confirmation gate", async ({ page }) => {
  const fragment = Buffer.from(JSON.stringify(RECIPE)).toString("base64url");
  await page.goto(`/en/app#recipe=${fragment}`);
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("Load this recipe?")).toBeVisible(ENGINE_BOOT);
  await expect(dialog.getByText("pdf.rotate")).toBeVisible();
  await expect(dialog.getByText("pdf.compress")).toBeVisible();
  // Nothing is loaded before the user agrees.
  await expect(page.locator('[data-slot="recipe-step"]')).toHaveCount(0);
  await dialog.getByRole("button", { name: "Confirm & Load" }).click();
  await expect(page.locator('[data-slot="recipe-step"]')).toHaveCount(2);
  await expect(page).not.toHaveURL(/#recipe=/);
});

test("drops a cancelled #recipe= link without loading it", async ({ page }) => {
  const fragment = Buffer.from(JSON.stringify(RECIPE)).toString("base64url");
  await page.goto(`/en/app#recipe=${fragment}`);
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible(ENGINE_BOOT);
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.locator('[data-slot="recipe-step"]')).toHaveCount(0);
});

test("refuses a #recipe= link that carries a literal secret", async ({ page }) => {
  const risky = {
    version: "ariad-recipe/1",
    steps: [{ tool: "pdf.decrypt", params: { password: "hunter2" } }],
  };
  await page.goto(`/en/app#recipe=${Buffer.from(JSON.stringify(risky)).toString("base64url")}`);
  await expect(page.locator('[data-slot="recipe-link-problem"]')).toBeVisible(ENGINE_BOOT);
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("shares a recipe as a link that opens behind the same gate", async ({
  page,
  context,
  browserName,
}) => {
  test.skip(browserName !== "chromium", "Clipboard permissions are driven in Chromium only");
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await openRecipes(page);
  await page.getByLabel("Tool to add").selectOption("pdf.merge");
  await page.getByRole("button", { name: "Add step" }).click();
  await page.getByRole("button", { name: "Share link" }).click();
  await expect(page.locator('[data-slot="recipe-share-status"]')).toBeVisible();
  const link = await page.evaluate(() => navigator.clipboard.readText());
  expect(link).toContain("#recipe=");

  await page.goto(link);
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("Load this recipe?")).toBeVisible(ENGINE_BOOT);
  await expect(dialog.getByText("pdf.merge")).toBeVisible();
});

test("opens the recipe builder from the command palette recipe", async ({ page }) => {
  await page.goto("/en/app");
  await page.getByRole("tab", { name: "Tools" }).click();
  await expect(page.getByTestId("tool-hub")).toBeVisible(ENGINE_BOOT);
  await page.keyboard.press("Control+K");
  await page.getByRole("combobox", { name: /search tools/i }).fill("rotate 90");
  await page.getByTestId("recipe-preview").click();
  await expect(page.getByRole("tab", { name: "Recipes" })).toHaveAttribute(
    "aria-selected",
    "true",
    ENGINE_BOOT,
  );
  await expect(page.locator('[data-slot="recipe-step"]')).toHaveCount(1, ENGINE_BOOT);
  await expect(page.locator('[data-slot="recipe-step"]')).toContainText("pdf.rotate");
});

test("saves a finished run as a recipe in the builder", async ({ page }) => {
  await openWorkspace(page);
  await addPdfs(page, [{ name: "four.pdf", pages: 4, label: "S" }]);
  await chooseTool(page, "pdf.nup");
  await page.getByRole("button", { name: "Run tool on this device" }).click();
  await expect(result(page)).toBeVisible(ENGINE_BOOT);
  await result(page).getByRole("button", { name: "Save as recipe" }).click();
  await expect(page.getByRole("tab", { name: "Recipes" })).toHaveAttribute("aria-selected", "true");
  await expect(page.locator('[data-slot="recipe-step"]')).toHaveCount(1, ENGINE_BOOT);
  await expect(page.locator('[data-slot="recipe-step"]')).toContainText("pdf.nup");
});
