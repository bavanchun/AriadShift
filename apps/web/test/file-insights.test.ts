import { readFileSync } from "node:fs";
import { inspectPdfInsights } from "@/converter/insights";
import { toolCatalog } from "@/converter/tool-catalog";
import { loadCore } from "./wasm";

const budget = 16 * 1024 * 1024;
const fixture = (path: string) =>
  new Uint8Array(readFileSync(new URL(`../../../fixtures/${path}`, import.meta.url)));
function inspect(bytes: Uint8Array) {
  const core = loadCore();
  return inspectPdfInsights(core, bytes, "document.pdf", budget, toolCatalog(core.listTools?.()));
}

/** Small, valid PDF built from test-owned content, including exact xref offsets. */
function pdf(objects: string[], trailer = "") {
  let text = "%PDF-1.4\n";
  const offsets = [0];
  for (const [index, body] of objects.entries()) {
    offsets.push(text.length);
    text += `${index + 1} 0 obj\n${body}\nendobj\n`;
  }
  const xref = text.length;
  text += `xref\n0 ${offsets.length}\n0000000000 65535 f \n`;
  text += offsets
    .slice(1)
    .map((offset) => `${String(offset).padStart(10, "0")} 00000 n \n`)
    .join("");
  text += `trailer\n<< /Size ${offsets.length} /Root 1 0 R ${trailer} >>\nstartxref\n${xref}\n%%EOF\n`;
  return new TextEncoder().encode(text);
}

describe("local PDF file insights", () => {
  it("reports scanned pages, image DPI estimates and real next-step IDs", () => {
    const result = inspect(fixture("pdf/vi-garden-image-only.pdf"));
    expect(result.pageCount).toBeGreaterThan(0);
    expect(result.scannedPageCount).toBeGreaterThan(0);
    expect(result.scannedPages).toContain(1);
    expect(result.suggestedTools).toContain("pdf.ocr");
    expect(result.estimatedImageDpi.min).toBeGreaterThan(0);
  });

  it("follows page-tree order and inherited resources instead of assigning every image to every page", () => {
    const result = inspect(
      pdf(
        [
          "<< /Type /Catalog /Pages 2 0 R >>",
          "<< /Type /Pages /Kids [4 0 R 3 0 R] /Count 2 /MediaBox [0 0 612 792] >>",
          "<< /Type /Page /Parent 2 0 R /Contents 6 0 R /Resources << /XObject << /Im1 7 0 R >> >> >>",
          "<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources << >> >>",
          "<< /Length 0 >>\nstream\n\nendstream",
          "<< /Length 7 >>\nstream\n/Im1 Do\nendstream",
          "<< /Type /XObject /Subtype /Image /Width 612 /Height 792 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\nx\nendstream",
          "<< /Subject (Private draft) /Keywords (notes) /CreationDate (D:20260101) /ModDate (D:20260201) >>",
        ],
        "/Info 8 0 R",
      ),
    );
    expect(result.pageCount).toBe(2);
    expect(result.blankPages).toEqual([1]);
    expect(result.scannedPages).toEqual([2]);
    expect(result.estimatedImageDpi).toEqual({ min: 72, max: 72, lowDpiPages: [2] });
    expect(result.metadataKeys).toEqual(
      expect.arrayContaining(["subject", "keywords", "creation_date", "mod_date"]),
    );
    expect(result.hasEditHistory).toBe(true);
    expect(result.suggestedTools).toEqual(
      expect.arrayContaining(["pdf.delete_pages", "pdf.sanitize"]),
    );
  });

  it("counts required empty fields and signature fields using the real form listing contract", () => {
    const core = loadCore();
    const source = fixture("pdf-forms/forms-all-types.pdf");
    const created = core.runTool?.(
      [source],
      ["form.pdf"],
      "pdf.forms_create",
      JSON.stringify({
        fields: [
          {
            type: "text",
            name: "required_empty",
            page: 1,
            rect: [20, 20, 160, 45],
            required: true,
          },
        ],
      }),
      budget,
      false,
    );
    expect(created).toBeDefined();
    const result = inspect(created?.bytes as Uint8Array);
    expect(result.hasForms).toBe(true);
    expect(result.emptyRequiredFormFields.names).toContain("required_empty");
    expect(result.emptyRequiredFormFields.count).toBe(result.emptyRequiredFormFields.names.length);
    expect(result.signatureFieldCount).toBeGreaterThan(0);
    expect(result.suggestedTools).toContain("pdf.forms_fill");
    expect(result.suggestedTools).toContain("pdf.sign_verify");
  });

  it("returns null DPI for vector/text-only PDFs and detects incremental update trailers", () => {
    const source = pdf([
      "<< /Type /Catalog /Pages 2 0 R >>",
      "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
      "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>",
    ]);
    const result = inspect(source);
    expect(result.estimatedImageDpi).toEqual({ min: null, max: null, lowDpiPages: [] });
    expect(result.hasEditHistory).toBe(false);
    const appended = new Uint8Array([...source, ...new TextEncoder().encode("\n%%EOF\n")]);
    expect(inspect(appended).hasEditHistory).toBe(true);
  });
});
