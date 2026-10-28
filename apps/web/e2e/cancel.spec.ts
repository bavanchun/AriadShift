import { expect, test } from "@playwright/test";

test("cancels a conversion and converts another file with a fresh worker", async ({ page }) => {
  test.setTimeout(120_000);
  await page.addInitScript(() => {
    const OriginalWorker = window.Worker;
    let count = 0;
    window.Worker = class extends OriginalWorker {
      constructor(...args: ConstructorParameters<typeof Worker>) {
        super(...args);
        document.documentElement.dataset.converterWorkers = String(++count);
      }
    };
  });
  await page.goto("/app");
  await expect(page.getByTestId("device-indicator")).toContainText(/browser storage|In memory/, {
    timeout: 60_000,
  });
  await page.locator("input[type=file]").setInputFiles({
    name: "large.md",
    mimeType: "text/markdown",
    buffer: Buffer.from(
      `# Large document\n\n${"A long paragraph of ordinary text. ".repeat(100_000)}\n`,
    ),
  });
  await expect(page.getByTestId("input-name")).toHaveText("large.md");
  await page.getByTestId("target-select").click();
  await page.getByRole("option", { name: "DOCX" }).click();
  const convert = page.getByRole("button", { name: "Convert", exact: true });
  await expect(convert).toBeEnabled();
  const before = await page.locator("html").getAttribute("data-converter-workers");
  await convert.click();
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(page.getByRole("button", { name: "Cancel", exact: true })).toHaveCount(0);
  await expect(page.getByTestId("converter-error")).toHaveCount(0);
  await page.locator("input[type=file]").setInputFiles({
    name: "next.md",
    mimeType: "text/markdown",
    buffer: Buffer.from("# Fresh worker\n"),
  });
  await expect(page.getByTestId("input-name")).toHaveText("next.md");
  await expect
    .poll(async () => Number(await page.locator("html").getAttribute("data-converter-workers")))
    .toBeGreaterThan(Number(before));
  await expect(convert).toBeEnabled();
  await convert.click();
  await expect(page.getByTestId("output-name")).toHaveText("next.html");
});
