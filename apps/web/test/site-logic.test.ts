import { offlineUrls } from "@/converter/offline";
import { formatBytes, formatDuration, formatScore } from "@/lib/format-bytes";
import { ACCEPT, FORMATS, formatLabel, pairSlug, parsePairSlug } from "@/lib/formats";
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
    expect(parsePairSlug("pdf-to-html")).toBeUndefined();
  });

  it("labels unknown ids with the id and builds the picker filter", () => {
    expect(formatLabel("ariad-ir+json")).toBe("AriadShift IR (JSON)");
    expect(formatLabel("pdf")).toBe("pdf");
    expect(ACCEPT.split(",")).toEqual(
      expect.arrayContaining([".md", ".markdown", ".html", ".htm", ".json", ".docx", ".epub"]),
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
