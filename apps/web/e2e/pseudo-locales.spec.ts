import { expect, test } from "@playwright/test";

// The pseudo-locales (docs/i18n.md) are routable only in a build made with
// ARIADSHIFT_I18N_PSEUDO=1, which the default end-to-end run does not use:
//   ARIADSHIFT_I18N_PSEUDO=1 pnpm --filter @ariadshift/web exec next build
//   ARIADSHIFT_I18N_PSEUDO=1 pnpm --filter @ariadshift/web e2e pseudo-locales
const PSEUDO = process.env.ARIADSHIFT_I18N_PSEUDO === "1";
const WIDTHS = [320, 768, 1280];

test.describe("pseudo-locales", () => {
  test.skip(!PSEUDO, "needs a build made with ARIADSHIFT_I18N_PSEUDO=1");

  for (const [locale, dir] of [
    ["en-XA", "ltr"],
    ["ar-XB", "rtl"],
  ] as const) {
    test(`${locale} converter has dir=${dir} and does not overflow horizontally`, async ({
      page,
    }) => {
      for (const width of WIDTHS) {
        await page.setViewportSize({ width, height: 800 });
        await page.goto(`/${locale}/app`);
        await expect(page.locator("html")).toHaveAttribute("dir", dir);
        await expect(page.locator("html")).toHaveAttribute("lang", locale);
        const overflow = await page.evaluate(
          () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
        );
        expect(overflow, `${locale} at ${width}px overflows by ${overflow}px`).toBeLessThanOrEqual(
          0,
        );
        // Every control stays inside the viewport horizontally.
        const clipped = await page.evaluate(() =>
          [...document.querySelectorAll("button, a, input, select, [role=combobox]")]
            .filter((element) => {
              const box = element.getBoundingClientRect();
              return box.width > 0 && (box.left < -1 || box.right > window.innerWidth + 1);
            })
            .map((element) => element.textContent?.trim().slice(0, 40) ?? element.tagName),
        );
        expect(clipped, `${locale} at ${width}px clips controls`).toEqual([]);
      }
    });
  }

  test("en-XA shows accented, bracketed text and no unaccented English headline", async ({
    page,
  }) => {
    await page.goto("/en-XA");
    const heading = await page.getByRole("heading", { level: 1 }).textContent();
    expect(heading).toMatch(/^\[.+\]$/);
    expect(heading).not.toContain("Convert documents");
  });
});
