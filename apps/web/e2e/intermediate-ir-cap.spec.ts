import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { crc32 } from "node:zlib";
import { expect, type Page, test } from "@playwright/test";

test.setTimeout(120_000);

const ENGINE_BOOT = { timeout: 60_000 };
const JPEG_BYTES = 12.5 * 1024 * 1024;

/**
 * 1x1 white baseline JPEG (204 bytes), produced with:
 * `ffmpeg -f lavfi -i color=c=white:s=1x1:d=1 -frames:v 1 -bitexact -f image2 -`
 * Contains SOI (0xFFD8), APP0 JFIF (0xFFE0), DQT (0xFFDB), DHT (0xFFC4),
 * SOF0 baseline frame (0xFFC0), SOS (0xFFDA at byte offset 185), scan data, and EOI (0xFFD9).
 */
const BASELINE_1X1_JPEG_HEX =
  "ffd8ffe000104a46494600010200000100010000" +
  "ffdb0043000804040404040505050505050606060606060606060606060607070708080807070706060707080808080909090808080809090a0a0a0c0c0b0b0e0e0e111114" +
  "ffc4004b00010100000000000000000000000000000007010100000000000000000000000000000000100100000000000000000000000000000000110100000000000000000000000000000000" +
  "ffc00011080001000103012200021100031100" +
  "ffda000c03010002110311003f00bf800fffd9";

function buildDecodableJpeg(targetBytes: number): Buffer {
  const base = Buffer.from(BASELINE_1X1_JPEG_HEX, "hex");
  const sosOffset = base.indexOf(Buffer.from([0xff, 0xda]));
  if (sosOffset < 0) {
    throw new Error("missing SOS marker in baseline JPEG");
  }
  const out = Buffer.alloc(targetBytes, 0x41);
  base.copy(out, 0, 0, sosOffset);
  let cursor = sosOffset;
  let remaining = targetBytes - base.length;
  // Pad with valid COM (0xFFFE) segments before the SOS marker so the JPEG stays decodable.
  while (remaining > 0) {
    let segmentTotal = Math.min(remaining, 65_500);
    if (remaining - segmentTotal > 0 && remaining - segmentTotal < 4) {
      segmentTotal = remaining - 4;
    }
    const segmentLength = segmentTotal - 2;
    out[cursor] = 0xff;
    out[cursor + 1] = 0xfe;
    out.writeUInt16BE(segmentLength, cursor + 2);
    cursor += segmentTotal;
    remaining -= segmentTotal;
  }
  base.copy(out, cursor, sosOffset);
  return out;
}

function storedZip(entries: Array<{ name: string; data: Buffer }>): Buffer {
  const localParts: Buffer[] = [];
  const centralParts: Buffer[] = [];
  let offset = 0;

  for (const entry of entries) {
    const nameBytes = Buffer.from(entry.name, "utf8");
    const checksum = crc32(entry.data) >>> 0;

    const local = Buffer.alloc(30 + nameBytes.length);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0, 6);
    local.writeUInt16LE(0, 8);
    local.writeUInt16LE(0, 10);
    local.writeUInt16LE(0x0021, 12);
    local.writeUInt32LE(checksum, 14);
    local.writeUInt32LE(entry.data.length, 18);
    local.writeUInt32LE(entry.data.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    local.writeUInt16LE(0, 28);
    nameBytes.copy(local, 30);

    const central = Buffer.alloc(46 + nameBytes.length);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0, 8);
    central.writeUInt16LE(0, 10);
    central.writeUInt16LE(0, 12);
    central.writeUInt16LE(0x0021, 14);
    central.writeUInt32LE(checksum, 16);
    central.writeUInt32LE(entry.data.length, 20);
    central.writeUInt32LE(entry.data.length, 24);
    central.writeUInt16LE(nameBytes.length, 28);
    central.writeUInt16LE(0, 30);
    central.writeUInt16LE(0, 32);
    central.writeUInt16LE(0, 34);
    central.writeUInt16LE(0, 36);
    central.writeUInt32LE(0, 38);
    central.writeUInt32LE(offset, 42);
    nameBytes.copy(central, 46);

    localParts.push(local, entry.data);
    centralParts.push(central);
    offset += local.length + entry.data.length;
  }

  const centralDir = Buffer.concat(centralParts);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(0, 4);
  eocd.writeUInt16LE(0, 6);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(centralDir.length, 12);
  eocd.writeUInt32LE(offset, 16);
  eocd.writeUInt16LE(0, 20);

  return Buffer.concat([...localParts, centralDir, eocd]);
}

function buildJpegDocx(jpeg: Buffer): Buffer {
  const contentTypes = `<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="xml" ContentType="application/xml" />
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" />
  <Default Extension="jpg" ContentType="image/jpeg" />
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml" />
</Types>`;

  const rootRels = `<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml" />
</Relationships>`;

  const docRels = `<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/photo.jpg" />
</Relationships>`;

  const documentXml = `<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
            xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
            xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
            xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">
  <w:body>
    <w:p>
      <w:r>
        <w:t>Large JPEG document</w:t>
      </w:r>
    </w:p>
    <w:p>
      <w:r>
        <w:drawing>
          <wp:inline>
            <wp:extent cx="914400" cy="914400" />
            <wp:docPr id="1" name="Picture" descr="Embedded photo" />
            <a:graphic>
              <a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">
                <pic:pic>
                  <pic:nvPicPr>
                    <pic:cNvPr id="1" name="photo.jpg" />
                    <pic:cNvPicPr />
                  </pic:nvPicPr>
                  <pic:blipFill>
                    <a:blip r:embed="rId1" />
                  </pic:blipFill>
                  <pic:spPr>
                    <a:xfrm>
                      <a:off x="0" y="0" />
                      <a:ext cx="914400" cy="914400" />
                    </a:xfrm>
                    <a:prstGeom prst="rect">
                      <a:avLst />
                    </a:prstGeom>
                  </pic:spPr>
                </pic:pic>
              </a:graphicData>
            </a:graphic>
          </wp:inline>
        </w:drawing>
      </w:r>
    </w:p>
  </w:body>
</w:document>`;

  return storedZip([
    { name: "[Content_Types].xml", data: Buffer.from(contentTypes, "utf8") },
    { name: "_rels/.rels", data: Buffer.from(rootRels, "utf8") },
    { name: "word/_rels/document.xml.rels", data: Buffer.from(docRels, "utf8") },
    { name: "word/document.xml", data: Buffer.from(documentXml, "utf8") },
    { name: "word/media/photo.jpg", data: jpeg },
  ]);
}

async function pick(page: Page, name: string, buffer: Buffer) {
  await expect(page.getByTestId("device-indicator")).toContainText(
    /browser storage|In memory/,
    ENGINE_BOOT,
  );
  await page.locator("input[type=file]").setInputFiles({
    name,
    buffer,
    mimeType: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  });
  await expect(page.getByTestId("input-name")).toHaveText(name);
}

test("converts a 12.5 MiB JPEG DOCX to Markdown when intermediate IR exceeds 16 MiB", async ({
  page,
}) => {
  const jpeg = buildDecodableJpeg(JPEG_BYTES);
  const docx = buildJpegDocx(jpeg);
  expect(docx.byteLength).toBeGreaterThan(JPEG_BYTES);
  expect(docx.byteLength).toBeLessThan(16 * 1024 * 1024);

  await page.goto("/app");
  await pick(page, "large-jpeg.docx", docx);

  await page.getByTestId("target-select").click();
  await page.getByRole("option", { name: "Markdown", exact: true }).click();
  await expect(page.getByTestId("route")).toBeVisible();
  await page.getByRole("button", { name: "Convert", exact: true }).click();
  await expect(page.getByTestId("result")).toBeVisible({ timeout: 120_000 });
  await expect(page.getByTestId("converter-error")).toHaveCount(0);

  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: /^Download / }).click();
  const output = await readFile(await (await download).path(), "utf8");
  expect(output).toContain("Large JPEG document");
  const match = output.match(/data:image\/jpeg;base64,([A-Za-z0-9+/=]+)/);
  expect(match).not.toBeNull();
  const imageBytes = Buffer.from(match?.[1] ?? "", "base64");
  expect([...imageBytes.subarray(0, 3)]).toEqual([0xff, 0xd8, 0xff]);
  expect(imageBytes.byteLength).toBe(JPEG_BYTES);
  expect(imageBytes.equals(jpeg)).toBe(true);
  expect(createHash("sha256").update(imageBytes).digest("hex")).toBe(
    createHash("sha256").update(jpeg).digest("hex"),
  );
});
