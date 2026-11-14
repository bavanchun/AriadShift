import XCTest
@testable import AriadDesignSystem

final class FuzzyMatcherTests: XCTestCase {
    func testEmptyQueryMatchesWithoutHighlights() {
        XCTAssertEqual(DSFuzzyMatcher.match("", in: "Compress PDF"), DSFuzzyMatch(score: 0, matchedOffsets: []))
        XCTAssertEqual(DSFuzzyMatcher.match("   ", in: "Compress PDF")?.matchedOffsets, [])
    }

    func testSubsequenceIsRequired() {
        XCTAssertNil(DSFuzzyMatcher.match("xyz", in: "Compress PDF"))
        XCTAssertNil(DSFuzzyMatcher.match("pdfs", in: "PDF"))
        XCTAssertNotNil(DSFuzzyMatcher.match("cpdf", in: "Compress PDF"))
    }

    func testCaseAndDiacriticsAreIgnored() {
        XCTAssertEqual(DSFuzzyMatcher.match("RESUME", in: "Résumé builder")?.matchedOffsets, [0, 1, 2, 3, 4, 5])
        XCTAssertEqual(DSFuzzyMatcher.match("tieng", in: "Tiếng Việt")?.matchedOffsets, [0, 1, 2, 3, 4])
    }

    func testWordStartsAndRunsWin() {
        // "sum" should highlight the start of "Summarize", not scattered letters.
        XCTAssertEqual(DSFuzzyMatcher.match("sum", in: "Summarize")?.matchedOffsets, [0, 1, 2])
        // Word starts beat letters inside words: "pd" lands on "PDF", not "Compress" + "D".
        XCTAssertEqual(DSFuzzyMatcher.match("pd", in: "Compress PDF")?.matchedOffsets, [9, 10])
        // camelCase boundaries count as word starts.
        XCTAssertEqual(DSFuzzyMatcher.match("tw", in: "pdfToWord")?.matchedOffsets, [3, 5])
    }

    func testRankingPrefersPrefixAndContiguousMatches() throws {
        let compress = try XCTUnwrap(DSFuzzyMatcher.match("cmp", in: "Compress PDF"))
        let crop = try XCTUnwrap(DSFuzzyMatcher.match("cmp", in: "Crop margins of PDF"))
        XCTAssertGreaterThan(compress.score, crop.score)
        let prefix = try XCTUnwrap(DSFuzzyMatcher.match("split", in: "Split PDF"))
        let inner = try XCTUnwrap(DSFuzzyMatcher.match("split", in: "Auto split by bookmarks"))
        XCTAssertGreaterThan(prefix.score, inner.score)
    }
}

final class PaletteFilterTests: XCTestCase {
    private let sections = DSCommandPalette.previewSections

    func testEmptyQueryKeepsOrderAndSections() {
        let result = DSPaletteFilter.filter(sections, query: "")
        XCTAssertEqual(result.map(\.id), ["recent", "suggested", "organize"])
        XCTAssertEqual(result[2].rows.map(\.item.id), ["merge", "split", "rotate", "compress"])
    }

    func testRowIDsAreUniqueAcrossSections() {
        let ids = DSPaletteFilter.filter(sections, query: "").flatMap(\.rows).map(\.id)
        XCTAssertEqual(ids.count, Set(ids).count)
        XCTAssertTrue(ids.contains("recent/compress"))
        XCTAssertTrue(ids.contains("organize/compress"))
    }

    func testFilteringDropsEmptySectionsAndRanksWithinSection() {
        let result = DSPaletteFilter.filter(sections, query: "pdf")
        XCTAssertEqual(result.map(\.id), ["recent", "organize"])
        XCTAssertEqual(result[1].rows.first?.item.id, "merge")
    }

    func testKeywordMatchesRankLastAndHighlightNothing() throws {
        let result = DSPaletteFilter.filter(sections, query: "shrink")
        let row = try XCTUnwrap(result.first?.rows.first)
        XCTAssertEqual(row.item.id, "compress")
        XCTAssertEqual(row.highlightedOffsets, [])
    }
}

final class PaletteLayoutTests: XCTestCase {
    func testFitsWithoutSnappingWhenShort() {
        XCTAssertEqual(DSPaletteLayout.visibleHeight(rowsPerSection: [1, 2]), 28 + 40 + 28 + 80)
    }

    func testSnapsToWholeRowsAndNeverEndsOnAHeader() {
        // 28 + 4*40 = 188, then header 216 and rows 256, 296, 336 > 320: the cut lands after the row ending at 296.
        XCTAssertEqual(DSPaletteLayout.visibleHeight(rowsPerSection: [4, 5]), 296)
        // The second header would end at 296 <= 320 with its first row at 336, so the cut stays after row six.
        XCTAssertEqual(DSPaletteLayout.visibleHeight(rowsPerSection: [6, 3]), 268)
    }

    func testEmptySectionsAddNothing() {
        XCTAssertEqual(DSPaletteLayout.visibleHeight(rowsPerSection: [0, 1, 0]), 68)
        XCTAssertEqual(DSPaletteLayout.visibleHeight(rowsPerSection: []), 0)
    }
}
