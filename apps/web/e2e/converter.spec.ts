import { readFile } from "node:fs/promises";
import { expect, type Page, test } from "@playwright/test";

const MARKDOWN = `# Field notes

A paragraph with *emphasis* and a [link](https://example.test).

- first
- second

| Name | Value |
| ---- | ----- |
| a    | 1     |
`;

const HTML = `<!doctype html>
<html lang="en"><head><title>Sample page</title></head>
<body><h1>Sample page</h1><p>Hello <strong>world</strong>.</p><ul><li>one</li><li>two</li></ul></body></html>`;

async function openConverter(page: Page, query = "") {
  await page.goto(`/app${query}`);
  await expect(page.getByRole("heading", { name: "Convert a document" })).toBeVisible();
}

async function chooseFile(page: Page, name: string, mimeType: string, content: string | Buffer) {
  const chooser = page.waitForEvent("filechooser");
  await page.getByRole("button", { name: "Choose a file" }).click();
  await (await chooser).setFiles({ name, mimeType, buffer: Buffer.from(content) });
  await expect(page.getByTestId("input-name")).toHaveText(name);
}

async function selectTarget(page: Page, label: string) {
  await page.getByTestId("target-select").click();
  await page.getByRole("option", { name: label }).click();
  await expect(page.getByTestId("target-select")).toContainText(label);
}

async function convertAndDownload(page: Page) {
  await expect(page.getByTestId("route")).toBeVisible();
  await page.getByRole("button", { name: "Convert", exact: true }).click();
  await expect(page.getByTestId("result")).toBeVisible();
  const outputName = (await page.getByTestId("output-name").textContent()) ?? "";
  const downloading = page.waitForEvent("download");
  await page.getByRole("button", { name: `Download ${outputName}` }).click();
  const download = await downloading;
  expect(download.suggestedFilename()).toBe(outputName);
  const path = await download.path();
  return { name: outputName, text: await readFile(path, "utf8") };
}

test("converts Markdown to HTML on the device", async ({ page }) => {
  await openConverter(page);
  await expect(page.getByTestId("device-indicator")).toContainText("Processed on this device");
  await chooseFile(page, "notes.md", "text/markdown", MARKDOWN);
  await expect(page.getByTestId("detected-format")).toHaveText("Markdown");
  await expect(page.getByTestId("counts")).toContainText("Tables");

  await selectTarget(page, "HTML");
  await expect(page.getByTestId("route")).toContainText("AriadShift core");
  const output = await convertAndDownload(page);
  expect(output.name).toBe("notes.html");
  expect(output.text).toContain("<h1>Field notes</h1>");
  expect(output.text).toContain("<em>emphasis</em>");
  expect(output.text).toContain("<table>");
});

test("converts files with backslashes in name and retains the download name", async ({ page }) => {
  await openConverter(page);
  await chooseFile(page, "folder\\doc.md", "text/markdown", MARKDOWN);
  await selectTarget(page, "HTML");
  await expect(page.getByTestId("route")).toBeVisible();
  await page.getByRole("button", { name: "Convert", exact: true }).click();
  await expect(page.getByTestId("result")).toBeVisible();
  const outputName = (await page.getByTestId("output-name").textContent()) ?? "";
  expect(outputName).toBe("folder\\doc.html");
  const downloading = page.waitForEvent("download");
  await page.getByRole("button", { name: `Download ${outputName}` }).click();
  const download = await downloading;
  expect(download.suggestedFilename()).toBe("folder_doc.html");
  const path = await download.path();
  expect(await readFile(path, "utf8")).toContain("<h1>Field notes</h1>");
});

test("converts HTML to Markdown", async ({ page }) => {
  await openConverter(page);
  await chooseFile(page, "page.html", "text/html", HTML);
  await expect(page.getByTestId("detected-format")).toHaveText("HTML");
  await selectTarget(page, "Markdown");
  const output = await convertAndDownload(page);
  expect(output.name).toBe("page.md");
  expect(output.text).toContain("# Sample page");
  expect(output.text).toContain("**world**");
});

test("round-trips Markdown through IR JSON", async ({ page }) => {
  await openConverter(page);
  await chooseFile(page, "notes.md", "text/markdown", MARKDOWN);
  await selectTarget(page, "AriadShift IR (JSON)");
  const ir = await convertAndDownload(page);
  expect(ir.name).toBe("notes.ir.json");
  const document = JSON.parse(ir.text);
  expect(document.version).toBe("ariad-ir/1");

  await page.getByRole("button", { name: "Start over and delete files" }).click();
  await chooseFile(page, "notes.ir.json", "application/json", ir.text);
  await expect(page.getByTestId("detected-format")).toHaveText("AriadShift IR (JSON)");
  await selectTarget(page, "Markdown");
  const markdown = await convertAndDownload(page);
  expect(markdown.name).toBe("notes.md");
  expect(markdown.text).toContain("# Field notes");
  expect(markdown.text).toContain("| Name | Value |");
});

test("preselects the target from a format-pair link", async ({ page }) => {
  await openConverter(page, "?from=html&to=markdown");
  await chooseFile(page, "page.html", "text/html", HTML);
  await expect(page.getByTestId("target-select")).toContainText("Markdown");
});

test("offers the desktop app for legacy Office input", async ({ page }) => {
  await openConverter(page, "?to=markdown&profile=faithful");
  await chooseFile(page, "page.rtf", "application/rtf", "{\\rtf1 Hello}");
  await expect(page.getByTestId("detected-format")).toHaveText("Rich Text (RTF)");
  await expect(page.getByRole("link", { name: "Open in Desktop" })).toHaveAttribute(
    "href",
    "ariadshift://open?route=rtf-md&profile=faithful",
  );
  await expect(page.getByText("Process in Cloud")).toHaveCount(0);
});

test("file drops outside the picker preserve the current document", async ({ page }) => {
  await openConverter(page);
  await expect(page.getByTestId("device-indicator")).toContainText(/storage|In memory/);
  const prevented = () =>
    page.evaluate(() => {
      const files = new DataTransfer();
      files.items.add(new File(["# Another document"], "another.md"));
      const text = new DataTransfer();
      text.setData("text/plain", "ordinary text");
      const dispatch = (type: string, dataTransfer: DataTransfer) => {
        const event = new DragEvent(type, { bubbles: true, cancelable: true, dataTransfer });
        document.body.dispatchEvent(event);
        return event.defaultPrevented;
      };
      return [
        dispatch("dragover", files),
        dispatch("drop", files),
        dispatch("dragover", text),
        dispatch("drop", text),
      ];
    });

  expect(await prevented()).toEqual([true, true, false, false]);
  await chooseFile(page, "notes.md", "text/markdown", MARKDOWN);
  expect(await prevented()).toEqual([true, true, false, false]);
  await expect(page.getByTestId("input-name")).toHaveText("notes.md");
});

/** Names of the per-tab folders under the converter's OPFS directory. */
async function sessionFolders(page: Page): Promise<string[]> {
  return page.evaluate(async () => {
    const root = await navigator.storage.getDirectory();
    let base: FileSystemDirectoryHandle;
    try {
      base = await root.getDirectoryHandle("ariadshift");
    } catch {
      return [];
    }
    const names: string[] = [];
    for await (const name of (base as unknown as { keys(): AsyncIterable<string> }).keys()) {
      names.push(name);
    }
    return names;
  });
}

async function usesOpfs(page: Page): Promise<boolean> {
  await expect(page.getByTestId("device-indicator")).toContainText(/storage|In memory/);
  return (
    (await page.getByTestId("device-indicator").textContent())?.includes("browser storage") ?? false
  );
}

test("reset deletes the stored files", async ({ page, browserName }) => {
  await openConverter(page);
  await chooseFile(page, "notes.md", "text/markdown", MARKDOWN);
  await selectTarget(page, "HTML");
  await convertAndDownload(page);
  const opfs = await usesOpfs(page);

  await page.getByRole("button", { name: "Start over and delete files" }).click();
  await expect(page.getByRole("button", { name: "Choose a file" })).toBeVisible();
  await expect(page.getByRole("status")).toHaveText("Files were removed from this device.");

  test.skip(!opfs, `${browserName} kept files in memory`);
  expect(await sessionFolders(page)).toEqual([]);
});

test("two tabs keep their files apart", async ({ context, browserName }) => {
  const first = await context.newPage();
  const second = await context.newPage();
  await openConverter(first);
  await openConverter(second);
  await chooseFile(first, "first.md", "text/markdown", "# First tab\n");
  await chooseFile(second, "second.html", "text/html", "<h1>Second tab</h1>");

  await selectTarget(first, "HTML");
  const fromFirst = await convertAndDownload(first);
  expect(fromFirst.text).toContain("<h1>First tab</h1>");

  await selectTarget(second, "Markdown");
  const fromSecond = await convertAndDownload(second);
  expect(fromSecond.text).toContain("# Second tab");

  await second.getByRole("button", { name: "Start over and delete files" }).click();
  await expect(second.getByRole("status")).toHaveText("Files were removed from this device.");
  const again = await convertAndDownload(first);
  expect(again.text).toContain("<h1>First tab</h1>");

  test.skip(!(await usesOpfs(first)), `${browserName} kept files in memory`);
  expect(await sessionFolders(first)).toHaveLength(1);
});

test("files of a closed tab are deleted when the converter starts again", async ({
  context,
  browserName,
}) => {
  const closing = await context.newPage();
  await openConverter(closing);
  await chooseFile(closing, "left-behind.md", "text/markdown", "# Left behind\n");
  test.skip(!(await usesOpfs(closing)), `${browserName} kept files in memory`);
  await closing.close();

  const next = await context.newPage();
  await openConverter(next);
  await expect(next.getByTestId("device-indicator")).toContainText("browser storage");
  await expect.poll(() => sessionFolders(next)).toHaveLength(1);
});

test("refuses files above the browser limit", async ({ page }) => {
  await openConverter(page);
  const chooser = page.waitForEvent("filechooser");
  await page.getByRole("button", { name: "Choose a file" }).click();
  await (await chooser).setFiles({
    name: "huge.md",
    mimeType: "text/markdown",
    buffer: Buffer.alloc(17 * 1024 * 1024, "a"),
  });
  await expect(page.getByTestId("converter-error")).toContainText("up to 16 MB");
  await expect(page.getByRole("link", { name: "Open in Desktop" })).toHaveAttribute(
    "href",
    "ariadshift://open?route=md-html&profile=editable",
  );
  await expect(page.getByRole("button", { name: "Choose a file" })).toBeVisible();
});

test("offers a compatible Desktop route for oversized IR JSON", async ({ page }) => {
  await page.goto("/app?to=docx&profile=private");
  await page.locator("input[type=file]").setInputFiles({
    name: "huge.ir.json",
    mimeType: "application/json",
    buffer: Buffer.alloc(17 * 1024 * 1024, " "),
  });
  await expect(page.getByTestId("converter-error")).toContainText("up to 16 MB");
  await expect(page.getByRole("link", { name: "Open in Desktop" })).toHaveAttribute(
    "href",
    "ariadshift://open?route=json-docx&profile=private",
  );
});

test("the file picker is keyboard operable", async ({ page }) => {
  await openConverter(page);
  const picker = page.getByRole("button", { name: "Choose a file" });
  await picker.focus();
  await expect(picker).toBeFocused();
  const chooser = page.waitForEvent("filechooser");
  await page.keyboard.press("Enter");
  await chooser;
});

test("the target and profile selects are keyboard operable", async ({ page }) => {
  await openConverter(page);
  await chooseFile(page, "notes.md", "text/markdown", MARKDOWN);
  const target = page.getByTestId("target-select");
  await expect(target).toContainText("HTML");
  await target.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("option", { name: "HTML" })).toBeFocused();
  await page.keyboard.press("Home");
  await page.keyboard.press("Enter");
  await expect(target).toContainText("AriadShift IR (JSON)");
  await expect(page.getByRole("listbox")).toBeHidden();

  const profile = page.getByTestId("profile-select");
  await profile.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("option", { name: "Editable" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("option", { name: "Faithful" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(profile).toContainText("Faithful");
  await expect(page.getByTestId("route")).toBeVisible();
});
