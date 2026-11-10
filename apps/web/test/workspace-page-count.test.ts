import { countPdfPages } from "@/components/workspace/page-count";
import { makePdf } from "../e2e/pdf-fixtures";

const blob = (bytes: Uint8Array | string) => new Blob([bytes as BlobPart]);

describe("countPdfPages", () => {
  it("reads the count from the page tree root", async () => {
    expect(await countPdfPages(blob(new Uint8Array(makePdf(7))))).toBe(7);
    expect(await countPdfPages(blob(new Uint8Array(makePdf(1))))).toBe(1);
  });

  it("uses the root of a nested page tree", async () => {
    const nested =
      "%PDF-1.7\n1 0 obj\n<</Type/Catalog/Pages 2 0 R>>\nendobj\n" +
      "3 0 obj\n<</Type/Pages/Parent 2 0 R/Count 2/Kids[]>>\nendobj\n" +
      "2 0 obj\n<</Count 5/Kids[3 0 R]/Type/Pages>>\nendobj\n";
    expect(await countPdfPages(blob(nested))).toBe(5);
  });

  it("answers null for what it cannot read", async () => {
    expect(await countPdfPages(blob(""))).toBeNull();
    expect(await countPdfPages(blob("not a pdf"))).toBeNull();
    expect(await countPdfPages(blob(new Uint8Array(8)))).toBeNull();
    expect(await countPdfPages(blob("%PDF-1.7\n1 0 obj\n<</Type/Catalog>>\nendobj\n"))).toBeNull();
  });
});
