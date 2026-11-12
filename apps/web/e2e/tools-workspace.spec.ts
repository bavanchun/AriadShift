import { readFile } from "node:fs/promises";
import { expect, type Locator, type Page, test } from "@playwright/test";
import { declaredPageCount, makePdf, pageMarkers, pageRotations } from "./pdf-fixtures";

test.describe.configure({ timeout: 180_000 });

const ENGINE_BOOT = { timeout: 90_000 };

async function openWorkspace(page: Page, tool?: string) {
  await page.goto(tool ? `/en/app?tool=${tool}` : "/en/app?view=workspace");
  await expect(page.getByLabel("Choose a tool")).toBeVisible(ENGINE_BOOT);
}

const workspace = (page: Page) => page.locator('[data-slot="document-workspace"]');

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
  const tray = page.getByRole("list", { name: "Files in this workspace" });
  await expect(tray.getByRole("listitem")).toHaveCount(files.length, ENGINE_BOOT);
  await expect(page.getByText("What is in this PDF", { exact: true })).toBeVisible(ENGINE_BOOT);
}

async function chooseTool(page: Page, id: string) {
  await page.getByLabel("Choose a tool").selectOption(id);
  await expect(page.getByLabel("Choose a tool")).toHaveValue(id);
}

async function runChosenTool(page: Page) {
  await page.getByRole("button", { name: "Run tool on this device" }).click();
  await expect(page.locator('[data-slot="tool-result"]')).toBeVisible(ENGINE_BOOT);
}

async function resultPages(page: Page) {
  const row = page.locator('[data-slot="tool-result"] tbody tr', { hasText: "Pages" });
  return ((await row.locator("td").last().textContent()) ?? "").trim();
}

async function downloadResult(page: Page): Promise<Buffer> {
  const downloading = page.waitForEvent("download");
  await page
    .locator('[data-slot="tool-result"]')
    .getByRole("button", { name: /^Download / })
    .first()
    .click();
  const download = await downloading;
  expect(download.suggestedFilename()).toMatch(/\.pdf$/);
  return readFile(await download.path());
}

async function openPages(page: Page) {
  await page.getByRole("tab", { name: "Pages" }).click();
  await expect(page.getByRole("toolbar", { name: "Page edits" })).toBeVisible(ENGINE_BOOT);
}

const grip = (page: Page, pageNumber: number): Locator =>
  page.getByRole("button", { name: `Reorder page ${pageNumber}` });

test("merges two PDFs in the browser and downloads one valid PDF", async ({ page }) => {
  await openWorkspace(page);
  await addPdfs(page, [
    { name: "first.pdf", pages: 2, label: "A" },
    { name: "second.pdf", pages: 3, label: "B" },
  ]);
  await chooseTool(page, "pdf.merge");
  await runChosenTool(page);
  await expect(page.locator('[data-slot="tool-result"]')).toContainText("Result: Merge");
  expect(await resultPages(page)).toBe("5");

  const output = await downloadResult(page);
  expect(output.subarray(0, 5).toString("latin1")).toBe("%PDF-");
  expect(output.toString("latin1")).toContain("%%EOF");
  expect(declaredPageCount(output)).toBe(5);
  expect(pageMarkers(output, "[AB]")).toEqual(["A-1", "A-2", "B-1", "B-2", "B-3"]);
});

test("rotates a page from the page grid and downloads the rotated PDF", async ({ page }) => {
  await openWorkspace(page);
  await addPdfs(page, [{ name: "four.pdf", pages: 4, label: "R" }]);
  await openPages(page);
  await page.getByRole("button", { name: "Rotate page 2" }).click();
  await expect(page.getByText("1 pending change")).toBeVisible();
  await page.getByRole("button", { name: "Apply changes" }).click();
  await expect(page.locator('[data-slot="tool-result"]')).toBeVisible(ENGINE_BOOT);
  await expect(page.locator('[data-slot="tool-result"]')).toContainText("Result: Rotate");
  expect(await resultPages(page)).toBe("4");

  const output = await downloadResult(page);
  expect(pageRotations(output)).toEqual([0, 90, 0, 0]);
  expect(pageMarkers(output, "R")).toEqual(["R-1", "R-2", "R-3", "R-4"]);
});

test("reorders pages with the keyboard and downloads them in the new order", async ({ page }) => {
  await openWorkspace(page);
  await addPdfs(page, [{ name: "four.pdf", pages: 4, label: "K" }]);
  await openPages(page);
  await grip(page, 1).focus();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("status").filter({ hasText: "moved to position 3" })).toBeAttached();
  await page.getByRole("button", { name: "Apply changes" }).click();
  await expect(page.locator('[data-slot="tool-result"]')).toContainText(
    "Result: Reorder",
    ENGINE_BOOT,
  );

  const output = await downloadResult(page);
  expect(pageMarkers(output, "K")).toEqual(["K-2", "K-3", "K-1", "K-4"]);
});

test("reorders pages by dragging a page onto another", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "Native drag events are driven in Chromium only");
  // Tall enough that every page card is on screen, so the pointer needs no scrolling mid-drag.
  await page.setViewportSize({ width: 1280, height: 3600 });
  await openWorkspace(page);
  await addPdfs(page, [{ name: "four.pdf", pages: 4, label: "D" }]);
  await openPages(page);
  const from = await grip(page, 4).boundingBox();
  const to = await grip(page, 1).boundingBox();
  if (!(from && to)) throw new Error("the page grips are not on screen");
  // Native drag and drop needs real pointer movement, in several steps, to start a drag.
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(from.x + from.width / 2 + 8, from.y + from.height / 2 + 8, { steps: 4 });
  await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 12 });
  await page.mouse.up();
  await expect(page.getByText("1 pending change")).toBeVisible();
  await page.getByRole("button", { name: "Apply changes" }).click();
  await expect(page.locator('[data-slot="tool-result"]')).toBeVisible(ENGINE_BOOT);

  const markers = pageMarkers(await downloadResult(page), "D");
  expect(markers).toHaveLength(4);
  expect(markers[0]).not.toBe("D-1");
  expect([...markers].sort()).toEqual(["D-1", "D-2", "D-3", "D-4"]);
});

test("nothing is uploaded while tools run", async ({ page }) => {
  const outside: string[] = [];
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (
      url.origin !==
      new URL(page.url() === "about:blank" ? "http://localhost:3100" : page.url()).origin
    )
      outside.push(request.url());
  });
  await openWorkspace(page);
  await addPdfs(page, [
    { name: "a.pdf", pages: 2, label: "A" },
    { name: "b.pdf", pages: 2, label: "B" },
  ]);
  await chooseTool(page, "pdf.merge");
  await runChosenTool(page);
  expect(outside).toEqual([]);
});

test("a 1,000-page PDF opens in the viewer and scrolls to page 500 and page 1,000", async ({
  page,
  browserName,
}) => {
  test.slow();
  await openWorkspace(page);
  await addPdfs(page, [{ name: "thousand.pdf", pages: 1000, label: "T" }]);
  await expect(page.getByText("1,000 pages").first()).toBeVisible(ENGINE_BOOT);

  const surface = page.locator('[data-slot="pdf-viewer-surface"]');
  await expect(surface).toBeVisible(ENGINE_BOOT);
  const pageInput = page.getByRole("spinbutton", { name: "Page number" });
  await expect(pageInput).toBeVisible(ENGINE_BOOT);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));

  const domSize = () => page.evaluate(() => document.getElementsByTagName("*").length);
  const shownPage = async () => {
    await page.locator("body").click({ position: { x: 5, y: 5 } }); // drops the typed draft
    return Number(await pageInput.inputValue());
  };
  const first = await domSize();

  for (const target of [500, 1000]) {
    // A jump made before the viewer has laid out its pages is ignored, so retry until it lands.
    await expect(async () => {
      await pageInput.fill(String(target));
      await pageInput.press("Enter");
      await page.waitForTimeout(500);
      // The viewer reports the page most in view, which may be the one after the target's top edge.
      expect(Math.abs((await shownPage()) - target)).toBeLessThanOrEqual(1);
    }).toPass({ timeout: 60_000 });
    // The pages around the target are drawn, and only those.
    await expect
      .poll(async () => surface.locator("canvas, img").count(), { timeout: 30_000 })
      .toBeGreaterThan(0);
    expect(await domSize(), `DOM size after jumping to page ${target}`).toBeLessThan(first + 4000);
  }

  // Playwright's wheel input moves the scroller reliably in Chromium only.
  if (browserName === "chromium") {
    // A wheel scroll back up moves the viewer's scroller and draws the pages it reaches. (The page
    // box is not asserted here: it follows jumps, but not yet a wheel scroll.)
    const scrollTop = () =>
      surface.evaluate((node) => {
        const scrollers = [...node.querySelectorAll("*")].filter(
          (element) => element.scrollHeight > element.clientHeight + 1000,
        );
        return Math.max(0, ...scrollers.map((element) => element.scrollTop));
      });
    // Page sizes settle as pages are measured, which can still move the scroller after a jump.
    let before = await scrollTop();
    await expect(async () => {
      const now = await scrollTop();
      const settled = now === before;
      before = now;
      expect(settled).toBe(true);
    }).toPass({ intervals: [700], timeout: 30_000 });
    expect(before).toBeGreaterThan(0);
    await surface.scrollIntoViewIfNeeded();
    const area = await surface.boundingBox();
    if (!area) throw new Error("the viewer is not on screen");
    await page.mouse.move(area.x + area.width / 2, area.y + area.height / 2);
    await page.mouse.wheel(0, -4000);
    await expect.poll(scrollTop, { timeout: 30_000 }).toBeLessThan(before - 2000);
    await expect
      .poll(async () => surface.locator("canvas, img").count(), { timeout: 30_000 })
      .toBeGreaterThan(0);
    expect(await domSize(), "DOM size after scrolling").toBeLessThan(first + 4000);
  }
  expect(errors).toEqual([]);
});
