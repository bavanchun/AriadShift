import { readFile } from "node:fs/promises";
import path from "node:path";
import { expect, type Page, test } from "@playwright/test";
import { makePdf } from "./pdf-fixtures";

test.describe.configure({ timeout: 240_000 });

const ENGINE_BOOT = { timeout: 90_000 };
const MODES = ["view", "annotate", "document", "page"] as const;
type Mode = (typeof MODES)[number];

const SUBSET_FIXTURE = path.join(
  process.cwd(),
  "..",
  "..",
  "fixtures",
  "pdf-tools",
  "vi-diacritics-subset-font.pdf",
);

const workspace = (page: Page) => page.locator('[data-slot="document-workspace"]');
const mode = (page: Page, name: Mode) => page.getByTestId(`workspace-mode-${name}`);

async function openWorkspace(page: Page) {
  await page.goto("/en/app?view=workspace");
  await expect(page.getByLabel("Choose a tool")).toBeVisible(ENGINE_BOOT);
}

async function addPdf(page: Page, name: string, buffer: Buffer) {
  await workspace(page)
    .locator("input[type=file]")
    .setInputFiles({ name, mimeType: "application/pdf", buffer });
  await expect(page.getByText("What is in this PDF", { exact: true })).toBeVisible(ENGINE_BOOT);
  await expect(page.getByTestId("workspace-mode-switch")).toBeVisible();
}

async function switchTo(page: Page, name: Mode) {
  await mode(page, name).click();
  await expect(mode(page, name)).toHaveAttribute("aria-pressed", "true");
}

/** Counts the files OPFS keeps for the editor's version history. */
async function savedVersionFiles(page: Page): Promise<number> {
  return page.evaluate(async () => {
    const root = await navigator.storage.getDirectory();
    let base: FileSystemDirectoryHandle;
    try {
      base = await root.getDirectoryHandle("ariadshift-editor-versions");
    } catch {
      return 0;
    }
    const count = async (directory: FileSystemDirectoryHandle): Promise<number> => {
      let files = 0;
      for await (const [, handle] of directory as unknown as AsyncIterable<
        [string, FileSystemHandle]
      >) {
        files +=
          handle.kind === "file" ? 1 : await count(handle as unknown as FileSystemDirectoryHandle);
      }
      return files;
    };
    return count(base);
  });
}

/**
 * Whether this engine can open the origin-private file system at all. Playwright's WebKit build
 * rejects `getDirectory()` with an UnknownError, so the editor's OPFS-backed history cannot exist
 * there and says so instead of pretending to save.
 */
async function opfsAvailable(page: Page): Promise<boolean> {
  return page.evaluate(async () => {
    try {
      await navigator.storage.getDirectory();
      return true;
    } catch {
      return false;
    }
  });
}

test("switches between View, Annotate, Document and Page and keeps the page", async ({ page }) => {
  await openWorkspace(page);
  await addPdf(page, "report.pdf", makePdf(3, "Report"));

  const group = page.getByTestId("workspace-mode-switch");
  await expect(group.getByRole("button")).toHaveCount(4);
  await expect(mode(page, "view")).toHaveAttribute("aria-pressed", "true");

  // Annotate: go to page 2 with the surface's own page control.
  await switchTo(page, "annotate");
  const surface = page.locator('[data-slot="annotate-forms-surface"]');
  await expect(surface).toBeVisible(ENGINE_BOOT);
  await surface.getByRole("button", { name: "Next page" }).click();
  await expect(surface.getByText("Page 2 of 3")).toBeVisible();

  // Page mode opens on that page.
  await switchTo(page, "page");
  const pageEditor = page.getByTestId("page-objects-estimated");
  await expect(pageEditor).toBeVisible(ENGINE_BOOT);
  await expect(page.getByText("Page 2 of 3")).toBeVisible();
  await expect(page.getByRole("button", { name: "Text: Report-2" })).toBeVisible();

  // Document mode selects the block nearest that page position; selecting a block on page 3
  // there moves the shared position to page 3.
  await switchTo(page, "document");
  const editor = page.getByTestId("document-mode-editor");
  await expect(editor).toBeVisible(ENGINE_BOOT);
  await editor.getByText("Report-3", { exact: true }).click();

  await switchTo(page, "annotate");
  await expect(surface).toBeVisible();
  await expect(surface.getByText("Page 3 of 3")).toBeVisible();

  // Every mode is reachable again from here.
  for (const name of MODES) await switchTo(page, name);
});

test("edits in Document mode, shows fidelity, exports, and a redaction purges history", async ({
  page,
}) => {
  await openWorkspace(page);
  await addPdf(page, "notes.pdf", makePdf(1, "Notes"));
  await switchTo(page, "document");
  const editor = page.getByTestId("document-mode-editor");
  await expect(editor).toBeVisible(ENGINE_BOOT);

  const paragraph = editor.getByText("Notes-1", { exact: true });
  await paragraph.click();
  await page.keyboard.press("End");
  await page.keyboard.type(" edited on device");
  await expect(editor.getByText("Notes-1 edited on device")).toBeVisible();

  // A manual checkpoint lands in the OPFS-backed version history. Where the engine has no OPFS,
  // the drawer says history is unavailable rather than keeping versions it could not purge.
  const historyStored = await opfsAvailable(page);
  await editor.getByRole("button", { name: "Version history" }).click();
  if (historyStored) {
    await editor.getByLabel("Checkpoint label").fill("before export");
    await editor.getByRole("button", { name: "Save checkpoint" }).last().click();
    await expect(editor.locator('[data-kind="manual"]')).toHaveCount(1);
    await expect.poll(() => savedVersionFiles(page)).toBeGreaterThan(0);
  } else {
    await expect(editor.getByRole("alert").first()).toContainText("Version history is unavailable");
  }

  // The fidelity review comes before anything is exported.
  await editor.getByRole("button", { name: "Export", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByTestId("fidelity-score")).toBeVisible();
  await expect(dialog.getByTestId("fidelity-score")).toContainText("%");
  await expect(dialog.locator('[data-testid^="count-"]').first()).toBeVisible();

  for (const [label, extension] of [
    ["Markdown", "md"],
    ["HTML", "html"],
  ] as const) {
    if (!(await dialog.isVisible())) {
      await editor.getByRole("button", { name: "Export", exact: true }).click();
    }
    await page.getByRole("dialog").getByLabel(label, { exact: true }).check();
    await page.getByRole("dialog").getByRole("button", { name: "Confirm export" }).click();
    const result = page.locator('[data-slot="editor-export"]');
    await expect(result).toContainText(`.${extension}`, ENGINE_BOOT);
    const downloading = page.waitForEvent("download");
    await result.getByRole("button", { name: /^Download / }).click();
    const download = await downloading;
    expect(download.suggestedFilename()).toMatch(new RegExp(`\\.${extension}$`));
    expect(await readFile(await download.path(), "utf8")).toContain("Notes-1 edited on device");
  }

  // Redaction purges the saved history, says so, and leaves nothing in OPFS.
  if (!historyStored) return;
  await editor.getByRole("button", { name: "Purge history after redaction" }).click();
  await expect(page.getByTestId("redaction-purge-banner")).toBeVisible();
  await expect(page.getByTestId("redaction-purge-banner")).toContainText(/purged/i);
  await expect(editor.locator("[data-version-id]")).toHaveCount(0);
  await expect.poll(() => savedVersionFiles(page)).toBe(0);
});

test("Page mode warns when Vietnamese needs a font the subset lacks", async ({ page }) => {
  await openWorkspace(page);
  await addPdf(page, "vi-diacritics-subset-font.pdf", await readFile(SUBSET_FIXTURE));
  await switchTo(page, "page");
  await expect(page.getByTestId("page-objects-estimated")).toBeVisible(ENGINE_BOOT);

  await page.getByRole("button", { name: /^Text: Khu vườn/ }).dblclick();
  const field = page.getByRole("textbox", { name: "Edit text in place" });
  await field.fill("Tiếng Việt: Nguyễn Văn A");

  const alert = page.locator('[data-slot="font-substitution-alert"]');
  await expect(alert).toBeVisible();
  await expect(alert).toContainText("Noto Sans (Vietnamese coverage)");
  await expect(alert).toContainText("Subset");
  // The text is committed as typed, in NFC, with no replacement characters.
  await field.press("Enter");
  await expect(page.getByRole("button", { name: "Text: Tiếng Việt: Nguyễn Văn A" })).toBeVisible();
  await expect(page.getByText("\uFFFD")).toHaveCount(0);
  await expect(alert).toBeVisible();

  // The edit belongs to the file, so it is still there after another mode and back.
  await switchTo(page, "view");
  await switchTo(page, "page");
  await expect(page.getByRole("button", { name: "Text: Tiếng Việt: Nguyễn Văn A" })).toBeVisible();
  await expect(page.locator('[data-slot="font-substitution-alert"]')).toBeVisible();
});

test("Annotate mode stamps a page and designs a form with a tab order", async ({ page }) => {
  await openWorkspace(page);
  await addPdf(page, "form.pdf", makePdf(1, "Form"));
  await switchTo(page, "annotate");
  const surface = page.locator('[data-slot="annotate-forms-surface"]');
  await expect(surface).toBeVisible(ENGINE_BOOT);

  // A stamp placed from the keyboard makes the apply action available, and applying it runs
  // `pdf.annotations` through the workspace's tool runner.
  await surface.getByRole("button", { name: /^Stamp/ }).click();
  await surface.getByRole("button", { name: "Place stamp at centre" }).click();
  const apply = surface.getByRole("button", { name: "Apply changes" });
  await expect(apply).toBeEnabled();
  await apply.click();
  await expect(page.locator('[data-slot="tool-result"]')).toBeVisible(ENGINE_BOOT);
  await expect(page.locator('[data-slot="tool-result"]')).toContainText("Annotations");

  // The form designer lists fields in tab order and can reorder them.
  await surface.getByRole("button", { name: "Design form" }).click();
  await surface.getByRole("button", { name: "Add text field" }).click();
  await surface.getByRole("button", { name: "Add checkbox field" }).click();
  const fields = surface.getByRole("list", { name: "Fields in tab order" }).getByRole("listitem");
  await expect(fields).toHaveCount(2);
  await expect(surface.getByLabel("Tab order 1")).toBeVisible();
  await expect(surface.getByLabel("Tab order 2")).toBeVisible();
  const before = await fields.first().textContent();
  await surface
    .getByRole("button", { name: /^Move .* up$/ })
    .last()
    .click();
  await expect.poll(async () => fields.first().textContent()).not.toBe(before);
});

test("AI panel is offline-first, gates the cloud behind consent and never runs a recipe", async ({
  page,
}) => {
  const documentRequests: string[] = [];
  page.on("request", (request) => {
    const body = request.postDataBuffer();
    if (request.method() !== "GET" && body && body.length > 0) {
      documentRequests.push(`${request.method()} ${request.url()}`);
    }
  });

  await openWorkspace(page);
  await addPdf(page, "secret.pdf", makePdf(2, "Secret"));

  await page.getByTestId("inspector-tab-ai").click();
  const panel = page.locator('[data-slot="ai-panel"]');
  await expect(panel).toBeVisible();
  await expect(panel.locator('[data-slot="ai-privacy-badge"]')).toContainText("0 bytes uploaded");
  // The browser has no local model, so the panel offers the Desktop command, not a Run button.
  await expect(panel.locator('[data-slot="ai-handoff"]')).toBeVisible();
  await expect(panel.getByRole("button", { name: "Run", exact: true })).toHaveCount(0);

  // Choosing Cloud asks first; cancelling keeps the panel local and sends nothing.
  await panel.getByRole("button", { name: /^Cloud/ }).click();
  const consent = page.getByRole("dialog");
  await expect(consent).toBeVisible();
  await consent.getByRole("button", { name: "Cancel" }).click();
  await expect(consent).toBeHidden();
  await expect(panel.locator('[data-slot="ai-privacy-badge"]')).toHaveAttribute(
    "data-provider",
    "local",
  );

  // A sentence becomes a draft in the Recipe Builder, behind its confirmation, and runs nothing.
  await panel.getByRole("tab", { name: "Recipe" }).click();
  await panel.getByLabel("Describe the steps").fill("merge, OCR, compress under 5 MB");
  await panel.getByRole("button", { name: "Draft recipe" }).click();
  await expect(panel.locator('[data-slot="ai-recipe"]')).toBeVisible();
  await panel.getByRole("button", { name: /Open in Recipe Builder/ }).click();
  await expect(page.getByTestId("tab-recipes")).toHaveAttribute("aria-selected", "true");
  const confirm = page.getByRole("dialog");
  await expect(confirm).toContainText("Load this recipe?");
  await expect(confirm).toContainText("pdf.merge");
  await expect(page.locator('[data-slot="recipe-step"]')).toHaveCount(0);
  await confirm.getByRole("button", { name: "Confirm & Load" }).click();
  await expect(page.locator('[data-slot="recipe-step"]')).toHaveCount(3);

  // Nothing carrying the document left the device.
  expect(documentRequests).toEqual([]);
});
