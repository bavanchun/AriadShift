// Builds small, valid PDFs for the tool end-to-end tests, so no binary fixtures are committed.
// Every page shows one marker string in an uncompressed content stream, which lets a test read
// back the page order of a tool's output.

/** A PDF whose page `n` (1-based) shows `${label}-${n}`. */
export function makePdf(pageCount: number, label = "P"): Buffer {
  const objects: string[] = [];
  // Object ids: 1 catalog, 2 page tree, 3 font, then a page and its content per page.
  const pageId = (index: number) => 4 + index * 2;
  const contentId = (index: number) => 5 + index * 2;
  objects[1] = "<< /Type /Catalog /Pages 2 0 R >>";
  objects[2] = `<< /Type /Pages /Count ${pageCount} /Kids [${Array.from(
    { length: pageCount },
    (_, index) => `${pageId(index)} 0 R`,
  ).join(" ")}] >>`;
  objects[3] = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";
  for (let index = 0; index < pageCount; index += 1) {
    const text = `(${label}-${index + 1}) Tj`;
    const stream = `BT /F1 24 Tf 72 700 Td ${text} ET`;
    objects[pageId(index)] =
      `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents ${contentId(index)} 0 R /Resources << /Font << /F1 3 0 R >> >> >>`;
    objects[contentId(index)] = `<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`;
  }
  let body = "%PDF-1.7\n";
  const offsets: number[] = [];
  for (let id = 1; id < objects.length; id += 1) {
    offsets[id] = body.length;
    body += `${id} 0 obj\n${objects[id]}\nendobj\n`;
  }
  const xref = body.length;
  body += `xref\n0 ${objects.length}\n0000000000 65535 f \n`;
  for (let id = 1; id < objects.length; id += 1) {
    body += `${String(offsets[id]).padStart(10, "0")} 00000 n \n`;
  }
  body += `trailer\n<< /Size ${objects.length} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(body, "latin1");
}

/** The body of the first object of a PDF for which `accept` is true; tolerant of either spacing style. */
function findObject(text: string, accept: (body: string) => boolean): string {
  for (const match of text.matchAll(/(?:^|[\s>])\d+ 0 obj([\s\S]*?)endobj/g)) {
    if (accept(match[1] as string)) return match[1] as string;
  }
  return "";
}

const pagesRoot = (body: string) => /\/Type\s*\/Pages\b/.test(body);

/** Page markers of a PDF in page order, for a PDF that stores pages and content uncompressed. */
export function pageMarkers(pdf: Buffer, label = "P"): string[] {
  const text = pdf.toString("latin1");
  const root = findObject(text, pagesRoot);
  const kids = [...(/\/Kids\s*\[([^\]]*)\]/.exec(root)?.[1] ?? "").matchAll(/(\d+)\s+0\s+R/g)].map(
    (match) => Number(match[1]),
  );
  const marker = new RegExp(`\\((${label}-\\d+)\\)`);
  return kids.map((id) => {
    const page = objectById(text, id);
    const contents = /\/Contents\s*(\d+)\s+0\s+R/.exec(page);
    return marker.exec(objectById(text, Number(contents?.[1])))?.[1] ?? "?";
  });
}

function objectById(text: string, id: number): string {
  const match = new RegExp(`(?:^|[\\s>])${id} 0 obj([\\s\\S]*?)endobj`).exec(text);
  return match?.[1] ?? "";
}

/** The page count a PDF declares in its page tree root. */
export function declaredPageCount(pdf: Buffer): number {
  const root = findObject(pdf.toString("latin1"), pagesRoot);
  const count = /\/Count\s*(\d+)/.exec(root)?.[1];
  return count ? Number.parseInt(count, 10) : -1;
}

/** The /Rotate value of every page in page order (0 when a page has none). */
export function pageRotations(pdf: Buffer): number[] {
  const text = pdf.toString("latin1");
  const root = findObject(text, pagesRoot);
  const kids = [...(/\/Kids\s*\[([^\]]*)\]/.exec(root)?.[1] ?? "").matchAll(/(\d+)\s+0\s+R/g)].map(
    (match) => Number(match[1]),
  );
  return kids.map((id) => Number(/\/Rotate\s*(-?\d+)/.exec(objectById(text, id))?.[1] ?? 0));
}
