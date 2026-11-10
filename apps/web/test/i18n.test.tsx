// @vitest-environment jsdom
import path from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import DownloadPage from "@/app/[locale]/download/page";
import FormatsPage from "@/app/[locale]/formats/page";
import HomePage from "@/app/[locale]/page";
import PrivacyPage from "@/app/[locale]/privacy/page";
import QualityPage from "@/app/[locale]/quality/page";
import sitemap from "@/app/sitemap";
import { ConverterApp } from "@/components/converter/converter-app";
import { ConverterClient } from "@/converter/client";
import { allLocales, findLocale, isLocale, publishedLocales } from "@/i18n/locales";
import { loadMessages, scopeMessages } from "@/i18n/messages";
import { pageAlternates } from "@/i18n/metadata";
import { localePath, splitLocalePath } from "@/i18n/paths";
import { FORMATS, formatMessageKey } from "@/lib/formats";
import { renderIntl } from "./intl";

// The pseudo-locales are routable only in a run that asks for them (docs/i18n.md).
vi.hoisted(() => {
  process.env.ARIADSHIFT_I18N_PSEUDO = "1";
});

const params = (locale: string) => ({
  params: Promise.resolve({ locale }),
  searchParams: Promise.resolve({}),
});

describe("locale registry", () => {
  it("lists the 42 locales of docs/i18n.md plus the two test pseudo-locales", () => {
    const tags = allLocales().map((locale) => locale.tag);
    expect(tags.filter((tag) => tag !== "en-XA" && tag !== "ar-XB")).toHaveLength(42);
    expect(tags).toEqual(
      expect.arrayContaining(["en", "vi", "pt-BR", "sr-Latn", "zh-Hans", "ar", "fa"]),
    );
    expect(isLocale("en-XA")).toBe(true);
    expect(isLocale("xx")).toBe(false);
  });

  it("takes direction and status from locales.toml", () => {
    expect(findLocale("ar")?.dir).toBe("rtl");
    expect(findLocale("fa")?.dir).toBe("rtl");
    expect(findLocale("ar-XB")?.dir).toBe("rtl");
    expect(findLocale("vi")).toMatchObject({ status: "reviewed", community: false });
    expect(findLocale("de")).toMatchObject({ status: "community", community: true });
  });

  it("publishes reviewed and complete community locales, never pseudo or unchecked RTL ones", () => {
    const published = publishedLocales().map((locale) => locale.tag);
    expect(published).toEqual(expect.arrayContaining(["en", "vi", "de", "ja"]));
    expect(published).not.toContain("en-XA");
    expect(published).not.toContain("ar-XB");
    expect(published).not.toContain("ar");
    expect(published).not.toContain("fa");
  });

  it("lists every published locale in the sitemap and the hreflang alternates", () => {
    const urls = sitemap().map(({ url }) => url);
    for (const { tag } of publishedLocales()) {
      expect(urls).toContain(`https://ariadshift.ariadnev.com/${tag}`);
    }
    const alternates = pageAlternates("vi", "/formats");
    expect(alternates?.canonical).toBe("/vi/formats");
    expect(alternates?.languages).toMatchObject({
      en: "/en/formats",
      vi: "/vi/formats",
      "x-default": "/en/formats",
    });
    expect(alternates?.languages).not.toHaveProperty("ar");
  });
});

describe("messages", () => {
  it("layers a locale over English", () => {
    const en = loadMessages("en") as { common: { profile: { fast: { label: string } } } };
    expect(en.common.profile.fast.label).toBe("Fast");
    const german = loadMessages("de") as { common: { profile: { fast: { label: string } } } };
    expect(german.common.profile.fast.label).toEqual(expect.any(String));
  });

  it("drops translator notes and passes only the requested namespaces", () => {
    const scoped = scopeMessages(loadMessages("en"), ["common", "web.route"]);
    expect(Object.keys(scoped).sort()).toEqual(["common", "web"]);
    expect(Object.keys(scoped.web as object)).toEqual(["route"]);
    expect(JSON.stringify(loadMessages("en"))).not.toContain("@note");
  });

  it("describes every public format", () => {
    const common = loadMessages("en").common as Record<string, Record<string, unknown>>;
    for (const format of FORMATS.filter((entry) => !entry.internal)) {
      expect(common.format?.[formatMessageKey(format)], format.id).toMatchObject({
        noun: expect.any(String),
        description: expect.any(String),
      });
    }
  });

  it("composes locale-prefixed paths", () => {
    expect(localePath("vi", "/")).toBe("/vi");
    expect(localePath("vi", "/app")).toBe("/vi/app");
    expect(splitLocalePath("/vi/convert/a-to-b")).toEqual({
      locale: "vi",
      path: "/convert/a-to-b",
    });
    expect(splitLocalePath("/vi")).toEqual({ locale: "vi", path: "/" });
  });
});

/** Product, format and engine names that stay untranslated (the shared glossary). */
const NAMES = new Set([
  ...FORMATS.flatMap((format) => format.label.split(/\W+/)),
  "AriadShift",
  "Markdown",
  "HTML",
  "DOCX",
  "EPUB",
  "PDF",
  "Word",
  "Pandoc",
  "Typst",
  "LibreOffice",
  "Docling",
  "JSON",
  "MCP",
  "ashift",
  "PowerPoint",
  "Excel",
  "OpenDocument",
  "Rich",
  "Text",
  "Spreadsheet",
  "Presentation",
  "ODT",
  "RTF",
  "PPTX",
  "XLSX",
  "DOC",
  "XLS",
  "PPT",
  "ODS",
  "ODP",
  "PNG",
  "JPEG",
  "WebP",
  "TIFF",
  "IR",
  "MIT",
  "BSD",
  "GPL",
  "LGPL",
  "MPL",
  "OFL",
  "ISC",
  "Apache",
]);

/** Words of three or more plain letters outside names and code: hard-coded English. */
function unaccentedEnglish(html: string): string[] {
  const text = html
    .replace(/<(pre|code|script|style)[\s\S]*?<\/\1>/g, " ")
    .replace(/(?:aria-label|title|alt)="([^"]*)"/g, ">$1<")
    .replace(/<[^>]+>/g, " ")
    .replace(/&[a-z#0-9]+;/g, " ")
    .replace(/\.[a-z][a-z.]*/g, " ");
  return text
    .split(/[\s·,;:()[\]“”"'–→/.!?|{}~]+/)
    .filter((word) => /^[A-Za-z]{3,}$/.test(word) && !NAMES.has(word));
}

describe("en-XA pseudo-locale", () => {
  const previousDirectory = process.cwd();
  beforeAll(() => process.chdir(path.resolve(import.meta.dirname, "..")));
  afterAll(() => process.chdir(previousDirectory));

  it("accents, pads and brackets every server-rendered screen", async () => {
    for (const [name, page] of [
      ["home", HomePage],
      ["formats", FormatsPage],
      ["quality", QualityPage],
      ["download", DownloadPage],
      ["privacy", PrivacyPage],
    ] as const) {
      const html = renderToStaticMarkup(await page(params("en-XA")));
      expect(html, name).toContain("[");
      expect(html, name).toContain("~");
      expect(unaccentedEnglish(html), `${name} shows hard-coded English`).toEqual([]);
    }
  });

  it("renders the converter without hard-coded English", () => {
    const client = new ConverterClient(
      () =>
        Object.assign(new EventTarget(), {
          postMessage() {},
          terminate() {},
        }) as unknown as Worker,
    );
    const { container } = renderIntl(<ConverterApp client={client} />, "en-XA");
    expect(container.textContent).toContain("[");
    expect(unaccentedEnglish(container.innerHTML)).toEqual([]);
  });

  it("keeps the source locale readable", async () => {
    const html = renderToStaticMarkup(await HomePage(params("en")));
    expect(html).toContain("Convert documents on your own device");
    expect(html).toContain("/en/app");
  });
});
