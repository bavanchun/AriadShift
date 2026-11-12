import { expect, test } from "@playwright/test";

const ENGINE_BOOT = { timeout: 60_000 };

test("the app speaks Vietnamese on /vi/app and English on /en/app", async ({ page }) => {
  await page.goto("/en/app");
  await expect(page.locator("html")).toHaveAttribute("lang", "en");
  await expect(page.getByRole("tab", { name: "Tools" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Convert" })).toBeVisible();

  await page.goto("/vi/app");
  await expect(page.locator("html")).toHaveAttribute("lang", "vi");
  await expect(page.locator("html")).toHaveAttribute("dir", "ltr");
  await expect(page.getByRole("tab", { name: "Công cụ" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Chuyển đổi" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Không gian làm việc PDF" })).toBeVisible();

  await page.getByRole("tab", { name: "Công cụ" }).click();
  await expect(page.getByTestId("tool-hub")).toBeVisible(ENGINE_BOOT);
  await expect(page.getByPlaceholder("Tìm công cụ…")).toBeVisible();
  const merge = page.locator('[data-slot="tool-card"][data-tool-id="pdf.merge"]');
  await expect(merge.getByRole("button", { name: "Gộp", exact: true })).toBeVisible();
  await expect(merge).toContainText("Gộp hai hoặc nhiều tài liệu PDF");
  await expect(merge.getByTestId("runtime-badge")).toContainText("Trình duyệt");
  await expect(page.getByTestId("result-count")).toContainText("công cụ");
});

test("the workspace is Vietnamese too", async ({ page }) => {
  await page.goto("/vi/app?tool=pdf.rotate");
  await expect(page.getByLabel("Chọn công cụ")).toHaveValue("pdf.rotate", ENGINE_BOOT);
  await expect(
    page.getByText("Thêm một tệp PDF để xem trước và sắp xếp các trang của nó."),
  ).toBeVisible();
  await expect(page.locator('[data-slot="card-title"]', { hasText: "Xoay" })).toBeVisible();
});

test("the language switcher keeps the app route", async ({ page }) => {
  await page.goto("/en/app");
  await page.locator("summary[aria-label^='Language']").click();
  await page.getByRole("link", { name: "Tiếng Việt" }).click();
  await expect(page).toHaveURL(/\/vi\/app$/);
  await expect(page.getByRole("tab", { name: "Công cụ" })).toBeVisible();
});

test("Arabic lays the app out right to left", async ({ page }) => {
  await page.goto("/ar/app");
  const html = page.locator("html");
  await expect(html).toHaveAttribute("lang", "ar");
  await expect(html).toHaveAttribute("dir", "rtl");
  // The tab list starts at the right edge: the first tab sits right of the last.
  const tabs = page.getByRole("tablist").first().getByRole("tab");
  await expect(tabs).toHaveCount(4);
  const first = await tabs.first().boundingBox();
  const last = await tabs.last().boundingBox();
  expect(first && last && first.x > last.x).toBe(true);
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
});

test("the app route is cross-origin isolated in Chromium and Firefox", async ({
  page,
  browserName,
}) => {
  test.skip(
    browserName === "webkit",
    "WebKit does not implement Cross-Origin-Embedder-Policy: credentialless",
  );
  await page.goto("/en/app");
  await expect.poll(() => page.evaluate(() => self.crossOriginIsolated)).toBe(true);
  await page.goto("/vi/app");
  await expect.poll(() => page.evaluate(() => self.crossOriginIsolated)).toBe(true);
});

test("tools run without cross-origin isolation, so WebKit needs no SharedArrayBuffer", async ({
  page,
  browserName,
}) => {
  test.skip(browserName !== "webkit", "Isolation is available in the other browsers");
  await page.goto("/en/app?tool=pdf.rotate");
  expect(await page.evaluate(() => self.crossOriginIsolated)).toBe(false);
  await expect(page.getByLabel("Choose a tool")).toHaveValue("pdf.rotate", { timeout: 90_000 });
});
