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
  expect(document.version).toBe("ariad-ir/0");

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

test("explains formats that need the CLI", async ({ page }) => {
  await openConverter(page);
  await chooseFile(page, "notes.md", "text/markdown", MARKDOWN);
  await expect(page.getByText(/Word \(DOCX\), EPUB need the AriadShift CLI/)).toBeVisible();
});

test("reset deletes the stored files", async ({ page, browserName }) => {
  await openConverter(page);
  await chooseFile(page, "notes.md", "text/markdown", MARKDOWN);
  await selectTarget(page, "HTML");
  await convertAndDownload(page);
  const storage = await page.getByTestId("device-indicator").textContent();

  await page.getByRole("button", { name: "Start over and delete files" }).click();
  await expect(page.getByRole("button", { name: "Choose a file" })).toBeVisible();
  await expect(page.getByRole("status")).toHaveText("Files were removed from this device.");

  test.skip(!storage?.includes("Private browser storage"), `${browserName} kept files in memory`);
  const remaining = await page.evaluate(async () => {
    const root = await navigator.storage.getDirectory();
    const names: string[] = [];
    for await (const name of (root as unknown as { keys(): AsyncIterable<string> }).keys()) {
      names.push(name);
    }
    return names;
  });
  expect(remaining).not.toContain("ariadshift");
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
