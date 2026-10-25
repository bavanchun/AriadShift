import { engineLabel } from "@/components/route-summary";
import { offlineUrls } from "@/converter/offline";
import { formatBytes, formatDuration, formatScore } from "@/lib/format-bytes";
import { ACCEPT, desktopLink, FORMATS, formatLabel, pairSlug, parsePairSlug } from "@/lib/formats";
import capabilities from "../../../crates/ariad-core/data/capabilities.json";
import { routeHeaders } from "../route-headers.mjs";

describe("format pairs", () => {
  it("round-trips every slug", () => {
    for (const from of FORMATS) {
      for (const to of FORMATS) {
        if (from === to) {
          expect(parsePairSlug(pairSlug(from.id, to.id))).toBeUndefined();
        } else {
          expect(parsePairSlug(pairSlug(from.id, to.id))).toEqual({ from, to });
        }
      }
    }
  });

  it("rejects malformed slugs", () => {
    expect(parsePairSlug("markdown")).toBeUndefined();
    expect(parsePairSlug("markdown-to-html-to-docx")).toBeUndefined();
    expect(parsePairSlug("unknown-to-html")).toBeUndefined();
  });

  it("labels unknown ids with the id and builds the picker filter", () => {
    expect(formatLabel("ariad-ir+json")).toBe("AriadShift IR (JSON)");
    expect(formatLabel("pdf")).toBe("PDF");
    expect(formatLabel("unknown")).toBe("unknown");
    for (const id of new Set(capabilities.edges.flatMap((edge) => [edge.from, edge.to]))) {
      expect(formatLabel(id)).not.toBe(id);
    }
    for (const id of new Set(capabilities.edges.map((edge) => edge.engine))) {
      expect(engineLabel(id)).not.toBe(id);
    }
    expect(engineLabel("typst")).toBe("Typst");
    expect(engineLabel("libreoffice")).toBe("LibreOffice");
    expect(engineLabel("docling")).toBe("Docling");
    expect(engineLabel("pdf")).toBe("PDF text reader");
    expect(ACCEPT.split(",")).toEqual(
      expect.arrayContaining([".md", ".markdown", ".html", ".htm", ".json", ".docx", ".epub"]),
    );
  });

  it("encodes public formats as compatible Desktop route names", () => {
    for (const format of FORMATS.filter((format) => !format.internal)) {
      const expected =
        format.id === "ariad-ir+json" ? "json" : format.id === "markdown" ? "md" : format.id;
      const target = format.id === "html" ? "docx" : "html";
      const url = new URL(desktopLink(format.id, target, "private"));
      expect(url.searchParams.get("route")).toBe(`${expected}-${target}`);
      expect(url.searchParams.get("profile")).toBe("private");
    }
    expect(desktopLink("ariad-ir+json", "markdown", "editable")).toBe(
      "ariadshift://open?route=json-md&profile=editable",
    );
    expect(desktopLink("markdown", "ariad-ir+json", "fast")).toBe(
      "ariadshift://open?route=md-json&profile=fast",
    );
  });
});

describe("number formatting", () => {
  it("formats sizes, scores and durations", () => {
    expect(formatBytes(1)).toBe("1 byte");
    expect(formatBytes(512)).toBe("512 bytes");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(64 * 1024 * 1024)).toBe("64 MB");
    expect(formatScore(0.986)).toBe("99%");
    expect(formatScore(null)).toBe("not measured");
    expect(formatDuration(0)).toBe("under 1 ms");
    expect(formatDuration(80)).toBe("80 ms");
    expect(formatDuration(2500)).toBe("2.5 s");
  });
});

describe("offline URL list", () => {
  it("keeps same-origin assets and drops the rest", () => {
    expect(
      offlineUrls(
        "https://site.test/app?to=html#top",
        [
          "https://site.test/_next/static/chunks/a.js",
          "https://cdn.test/other.js",
          "https://site.test/sw.js",
          "https://site.test/formats?_rsc=abc",
        ],
        ["https://site.test/_next/static/chunks/turbopack-worker-1.js#params=x"],
      ),
    ).toEqual([
      "https://site.test/_next/static/chunks/a.js",
      "https://site.test/_next/static/chunks/turbopack-worker-1.js",
      "https://site.test/app",
    ]);
  });
});

describe("route headers", () => {
  it("isolates the converter only", () => {
    const opener = routeHeaders.filter(({ headers }) =>
      headers.some(({ key }) => key === "Cross-Origin-Opener-Policy"),
    );
    expect(opener.map(({ source }) => source)).toEqual(["/app", "/app/:path*"]);
  });
});
