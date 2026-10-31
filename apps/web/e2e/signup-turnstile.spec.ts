import { randomBytes } from "node:crypto";
import { expect, test } from "@playwright/test";

// Integration proof uses the real web build, real auth/PostgreSQL and the real
// Cloudflare widget/Siteverify endpoint. No route or widget stubs are installed.
// Automated runs must explicitly use Cloudflare's documented test key pair;
// they prove integration, not resistance to automated challenges in production.
const AUTH = process.env.NEXT_PUBLIC_ARIAD_AUTH_URL?.replace(/\/+$/, "");
const SITE_KEY = process.env.NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY;
const DUMMY_SITE_KEY =
  SITE_KEY === "1x00000000000000000000AA" || SITE_KEY === "1x00000000000000000000BB";
test.skip(!AUTH || !SITE_KEY, "needs a cloud build, real auth and a configured Turnstile site key");
test.use({ serviceWorkers: "block" });

test("verifies live signup proofs and rejects a direct bypass", async ({ page, request }) => {
  test.setTimeout(90_000);
  const suffix = randomBytes(8).toString("hex");
  const email = `browser-${suffix}@example.com`;
  const refused = await request.post(`${AUTH}/api/auth/sign-up/email`, {
    data: {
      name: "Bypass",
      email: `bypass-${suffix}@example.com`,
      password: "correct horse battery staple",
    },
  });
  expect(refused.status()).toBe(400);
  expect((await refused.json()).code).toBe("MISSING_CAPTCHA_RESPONSE");
  await page.goto("/sign-in?next=/sign-in");
  await page.getByRole("button", { name: "Create one" }).click();
  await page.getByLabel("Name", { exact: true }).fill("Browser signup");
  await page.getByLabel("Email", { exact: true }).fill(email);
  await page.getByLabel("Password", { exact: true }).fill("correct horse battery staple");
  await page.getByLabel("Confirm password", { exact: true }).fill("correct horse battery staple");
  const submit = page.getByRole("button", { name: "Create account", exact: true });
  await expect(submit).toBeEnabled({ timeout: 60_000 });
  const response = page.waitForResponse(
    (res) => res.url() === `${AUTH}/api/auth/sign-up/email` && res.request().method() === "POST",
  );
  await submit.click();
  const created = await response;
  expect(created.request().headers()["x-captcha-response"]).toBeTruthy();
  if (DUMMY_SITE_KEY) {
    // Cloudflare documents action "test" for dummy validation. It cannot
    // demonstrate a successful production "signup" action. Preserve refusal.
    expect(created.status()).toBe(403);
    expect((await created.json()).code).toBe("CAPTCHA_VERIFICATION_FAILED");
    await expect(page.getByText(/signup bot check was not accepted/)).toBeVisible();
    return;
  }
  expect(created.status()).toBe(200);
  await expect(page.getByText(email, { exact: true })).toBeVisible();
  const replay = await request.post(`${AUTH}/api/auth/sign-up/email`, {
    headers: { "x-captcha-response": created.request().headers()["x-captcha-response"] as string },
    data: {
      name: "Replay",
      email: `replay-${suffix}@example.com`,
      password: "correct horse battery staple",
    },
  });
  expect(replay.status()).toBe(403);
});

test("loads the live upload widget on the credentialless converter route", async ({ page }) => {
  test.setTimeout(90_000);
  const document = await page.goto("/app?to=docx");
  expect(document?.headers()["cross-origin-embedder-policy"]).toBe("credentialless");
  await page.locator("input[type=file]").setInputFiles({
    name: "challenge.md",
    mimeType: "text/markdown",
    buffer: Buffer.from("# Challenge\n"),
  });
  await page.getByRole("radio", { name: /^On / }).check();
  await expect(page.getByTestId("turnstile")).toBeVisible();
  // Read the real widget's hidden input; no window.turnstile replacement.
  await expect(page.locator('input[name="cf-turnstile-response"]')).not.toHaveValue("", {
    timeout: 60_000,
  });
  await expect(
    page.getByRole("alert").filter({ hasText: /bot check could not|check could not run/ }),
  ).toHaveCount(0);
});
