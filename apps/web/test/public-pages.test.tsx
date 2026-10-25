import path from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import PairPage from "@/app/convert/[pair]/page";
import FormatsPage from "@/app/formats/page";
import sitemap from "@/app/sitemap";
import { loadPairs } from "@/lib/catalog";

describe("public format pages", () => {
  // Match Next's working directory while loading the real WASM package.
  const previousDirectory = process.cwd();
  beforeAll(() => process.chdir(path.resolve(import.meta.dirname, "..")));
  afterAll(() => process.chdir(previousDirectory));

  it("renders CLI examples for PDF and Office routes", async () => {
    for (const [pair, command] of [
      ["pdf-to-markdown", "ashift convert input.pdf --to md"],
      ["markdown-to-pdf", "ashift convert input.md --to pdf"],
      ["odt-to-docx", "ashift convert input.odt --to docx"],
      ["ir-json-to-markdown", "ashift convert input.ir.json --to md"],
      ["markdown-to-ir-json", "ashift convert input.md --to json"],
    ] as const) {
      const html = renderToStaticMarkup(
        await PairPage({ params: Promise.resolve({ pair }), searchParams: Promise.resolve({}) }),
      );
      expect(html).toContain(command);
      expect(html).not.toContain("does not offer IR JSON");
    }
  });

  it("shows readable engine names and only public formats", () => {
    const html = renderToStaticMarkup(<FormatsPage />);
    expect(html).toContain("Read by PDF text reader");
    expect(html).toContain("written by Typst");
    expect(html).toContain("LibreOffice");
    expect(html).toContain("Docling");
    expect(html).not.toMatch(/(?:Read by|written by)[^<]*\b(?:pdf|typst|libreoffice|docling)\b/);
    expect(html).not.toContain("DoclingDocument (JSON)");
    expect(html).not.toContain("Pandoc AST (JSON)");
    expect(html).not.toMatch(/Extensions:\s*<\/p>/);
  });

  it("excludes internal formats from pair pages and the sitemap", () => {
    expect(loadPairs().length).toBeGreaterThan(0);
    for (const pair of loadPairs()) {
      expect([pair.from.id, pair.to.id]).not.toContain("docling+json");
      expect([pair.from.id, pair.to.id]).not.toContain("pandoc+json");
    }
    expect(
      sitemap()
        .map(({ url }) => url)
        .join("\n"),
    ).not.toMatch(/docling-json|pandoc-json/);
  });
});
