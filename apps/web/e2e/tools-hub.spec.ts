import { readFile } from "node:fs/promises";
import { expect, type Page, test } from "@playwright/test";

interface RegistryTool {
  id: string;
  runtimes: string[];
  tier: string;
}

const ENGINE_BOOT = { timeout: 60_000 };

async function registry(): Promise<RegistryTool[]> {
  const text = await readFile(new URL("../../../schemas/tools.v1.json", import.meta.url), "utf8");
  return (JSON.parse(text) as { tools: RegistryTool[] }).tools;
}

/** Where a tool is shown to run first: the same rule the hub's badges follow. */
const primaryRuntime = (tool: RegistryTool) =>
  tool.runtimes.includes("wasm")
    ? "browser"
    : tool.runtimes.includes("cloud")
      ? "cloud"
      : "desktop";

async function openHub(page: Page) {
  await page.goto("/en/app");
  await page.getByRole("tab", { name: "Tools" }).click();
  await expect(page.getByTestId("tool-hub")).toBeVisible(ENGINE_BOOT);
}

test("the hub lists every registered tool with its runtime and tier", async ({ page }) => {
  const tools = await registry();
  await openHub(page);
  const cards = page.locator('[data-testid^="group-"] [data-slot="tool-card"]');
  await expect(cards).toHaveCount(tools.length);

  const shown = await cards.evaluateAll((nodes) =>
    nodes.map((node) => ({
      id: node.getAttribute("data-tool-id"),
      runtime: node.getAttribute("data-runtime"),
      tier: node.getAttribute("data-tier"),
      runtimeBadge: node.querySelector('[data-testid="runtime-badge"]')?.textContent ?? "",
      tierBadge: node.querySelector('[data-testid="tier-badge"]')?.textContent ?? "",
    })),
  );
  expect(new Set(shown.map((card) => card.id)).size).toBe(tools.length);
  const labels = { browser: "Browser", desktop: "Desktop", cloud: "Cloud" } as const;
  for (const tool of tools) {
    const card = shown.find((entry) => entry.id === tool.id);
    const runtime = primaryRuntime(tool);
    expect(card, `${tool.id} is in the hub`).toBeTruthy();
    expect(card?.runtime, `${tool.id} runtime`).toBe(runtime);
    expect(card?.runtimeBadge, `${tool.id} runtime badge`).toContain(labels[runtime]);
    expect(card?.tierBadge, `${tool.id} tier badge`).toBe(tool.tier === "pro" ? "Pro" : "Free");
  }
  await expect(page.getByTestId("zero-upload-badge")).toContainText("0 bytes uploaded");
});

test("a category filter and the search box narrow the grid", async ({ page }) => {
  const tools = await registry();
  await openHub(page);
  const cards = page.locator('[data-testid^="group-"] [data-slot="tool-card"]');

  await page.getByTestId("runtime-desktop").click();
  const desktopOnly = tools.filter((tool) => primaryRuntime(tool) === "desktop").length;
  expect(desktopOnly).toBeGreaterThan(0);
  await expect(cards).toHaveCount(desktopOnly);
  await page.getByTestId("runtime-all").click();
  await expect(cards).toHaveCount(tools.length);

  await page.getByTestId("category-security").click();
  const security = await cards.count();
  expect(security).toBeGreaterThan(0);
  expect(security).toBeLessThan(tools.length);
  await expect(page.locator('[data-testid^="group-"]')).toHaveCount(1);
  await expect(page.getByTestId("group-security")).toBeVisible();
  await page.getByTestId("category-all").click();

  await page.getByRole("searchbox", { name: "Search tools…" }).fill("merge");
  await expect(cards.first()).toHaveAttribute("data-tool-id", "pdf.merge");
  expect(await cards.count()).toBeLessThan(15);

  await page.getByRole("searchbox", { name: "Search tools…" }).fill("zzzxqjk");
  await expect(page.getByTestId("empty-state")).toBeVisible();
  await page.getByRole("button", { name: "Reset search and filters" }).click();
  await expect(cards).toHaveCount(tools.length);
});

test("Ctrl+K opens the palette, and a plain sentence finds the rotate tool", async ({ page }) => {
  await openHub(page);
  await page.keyboard.press("Control+K");
  const input = page.getByRole("combobox", { name: /search tools/i });
  await expect(input).toBeVisible();
  await input.fill("rotate 90");
  const first = page.getByRole("option").first();
  await expect(first).toContainText("Rotate");
  await expect(first).toContainText("pdf.rotate");
  await page.keyboard.press("Enter");

  // The chosen tool opens in the PDF workspace.
  await expect(page.getByRole("tab", { name: "PDF workspace" })).toHaveAttribute(
    "aria-selected",
    "true",
    ENGINE_BOOT,
  );
  await expect(page.getByLabel("Choose a tool")).toHaveValue("pdf.rotate", ENGINE_BOOT);
  expect(new URL(page.url()).searchParams.get("tool")).toBe("pdf.rotate");
});

test("the palette also opens from the converter, before the hub was ever shown", async ({
  page,
}) => {
  await page.goto("/en/app");
  await expect(page.getByTestId("device-indicator")).toContainText(
    /browser storage|In memory/,
    ENGINE_BOOT,
  );
  await page.keyboard.press("Control+K");
  await expect(page.getByRole("combobox", { name: /search tools/i })).toBeVisible(ENGINE_BOOT);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("combobox", { name: /search tools/i })).toBeHidden();
});

test("a linked tool opens in the workspace", async ({ page }) => {
  await page.goto("/en/app?tool=pdf.compress");
  await expect(page.getByLabel("Choose a tool")).toHaveValue("pdf.compress", ENGINE_BOOT);
});

test("a desktop-only tool shows the honest 'Run on your computer' panel", async ({ page }) => {
  await openHub(page);
  const card = page.locator('[data-slot="tool-card"][data-tool-id="pdf.sign"]');
  await expect(card.getByTestId("runtime-badge")).toContainText("Desktop");
  await card.getByRole("button", { name: "Sign", exact: true }).click();

  await expect(page.getByLabel("Choose a tool")).toHaveValue("pdf.sign", ENGINE_BOOT);
  const panel = page.locator('[data-slot="handoff-panel"]');
  await expect(panel).toContainText("Run on your computer");
  const command = page.locator('[data-slot="handoff-panel"] code');
  await expect(command).toContainText("ashift tool 'pdf.sign'");
  await expect(page.getByRole("button", { name: "Run tool on this device" })).toHaveCount(0);
  await expect(page.getByRole("link", { name: "Get AriadShift" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Copy command" })).toBeVisible();
});

test("the copy button puts the command on the clipboard", async ({
  page,
  context,
  browserName,
}) => {
  test.skip(
    browserName !== "chromium",
    "Clipboard permissions are granted by name in Chromium only",
  );
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/en/app?tool=pdf.sign");
  const command = page.locator('[data-slot="handoff-panel"] code');
  await expect(command).toContainText("ashift tool 'pdf.sign'", ENGINE_BOOT);
  const expected = (await command.textContent()) ?? "";
  await page.getByRole("button", { name: "Copy command" }).click();
  await expect(page.getByText("Command copied")).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(expected);
});
