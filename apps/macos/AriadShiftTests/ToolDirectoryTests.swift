import AppKit
import AriadDesignSystem
import XCTest

@testable import AriadShift
@testable import AriadShiftCore

/// The discovery model behind the palette, the Tools tab and the inspector cards.
@MainActor
final class ToolDirectoryTests: XCTestCase {
    private let directory = ToolDirectory(groups: ToolFixtures.groups)

    func testRegistryGroupsGatherIntoJobsInJobOrder() {
        XCTAssertEqual(directory.jobs.map(\.job), [.organize, .edit, .convert, .secure, .ocr, .ai])
        let ids = Dictionary(uniqueKeysWithValues: directory.jobs.map { ($0.job, $0.tools.map(\.id)) })
        XCTAssertEqual(ids[.edit], ["pdf.watermark", "pdf.annotations"], "overlay comes before annotate in the registry order")
        XCTAssertEqual(ids[.convert], ["pdf.to_pdfa", "pdf.compress"], "convert comes before optimize")
        XCTAssertEqual(ids[.secure], ["pdf.encrypt", "pdf.validate_pdfa"])
        XCTAssertEqual(ToolJob.of(registryGroup: "something_new"), .other)
        XCTAssertEqual(directory.jobs.flatMap(\.tools).count, ToolFixtures.tools.count, "every tool appears once")
    }

    func testEmptyQueryShowsRecentThenSuggestedThenEachToolOnce() {
        let sections = directory.paletteSections(recent: ["pdf.compress", "pdf.gone"], suggested: ["pdf.compress", "pdf.ocr"])
        XCTAssertEqual(sections.map(\.id), ["recent", "suggested", "organize", "edit", "convert", "secure", "ai"])
        XCTAssertEqual(sections[0].items.map(\.id), ["pdf.compress"], "a tool no longer in the catalogue is skipped")
        XCTAssertEqual(sections[1].items.map(\.id), ["pdf.ocr"], "a recent tool is not suggested again")
        let ids = sections.flatMap(\.items).map(\.id)
        XCTAssertEqual(ids.count, Set(ids).count, "no tool shows twice")
        XCTAssertFalse(sections.contains { $0.id == "ocr" }, "a job whose only tool is above is left out")
    }

    func testTypedQuerySearchesEveryJobByTitleAndKeyword() {
        XCTAssertEqual(ids(DSPaletteFilter.filter(directory.searchSections(), query: "cmp")).first, "pdf.compress")
        XCTAssertEqual(
            ids(DSPaletteFilter.filter(directory.searchSections(), query: "optimize")), ["pdf.compress"],
            "the registry group is a keyword"
        )
        XCTAssertEqual(directory.searchSections().flatMap(\.items).count, ToolFixtures.tools.count)
    }

    private func ids(_ sections: [DSPaletteResultSection]) -> [String] {
        sections.flatMap(\.rows).map(\.item.id)
    }

    func testBrowserFiltersByJobAndKeepsOrderWithoutAQuery() {
        XCTAssertEqual(directory.browserSections(query: "").map(\.job), directory.jobs.map(\.job))
        XCTAssertEqual(directory.browserSections(query: "  ").count, directory.jobs.count, "blank space is no query")
        let rotate = directory.browserSections(query: "rotat")
        XCTAssertEqual(rotate.map(\.job), [.organize])
        XCTAssertEqual(rotate.first?.tools.map(\.id), ["pdf.rotate"])
        XCTAssertTrue(directory.browserSections(query: "zzzz").isEmpty)
    }

    func testUnavailableAndProToolsCarryABadgeAndAIUsesTheThreadTint() {
        let validate = ToolFixtures.entry("pdf.validate_pdfa")
        XCTAssertTrue(validate.isAvailable, "a Pro tool opens; the host refuses the run on the free plan")
        XCTAssertTrue(validate.isProLocked)
        XCTAssertEqual(ToolDirectory.badge(for: validate), "Pro")
        XCTAssertEqual(ToolDirectory.paletteItem(validate).badge, "Pro")
        XCTAssertNil(ToolDirectory.badge(for: ToolFixtures.entry("pdf.compress")))
        XCTAssertEqual(ToolDirectory.badge(for: ToolFixtures.entry("ai.summarize")), "Later")
        XCTAssertEqual(ToolDirectory.tint(for: ToolFixtures.entry("ai.summarize")), .thread)
        XCTAssertEqual(ToolDirectory.tint(for: ToolFixtures.entry("pdf.ocr")), .accent)
    }

    func testSuggestionsSkipProToolsThePlanDoesNotGrant() {
        let tools = ToolFixtures.tools.map { tool in
            tool.id == "pdf.ocr" ? ToolFixtures.info(tool.id, group: tool.group, summary: tool.summary, tier: "pro") : tool
        }
        let scan = ToolFixtures.report(words: 0)
        let free = ToolDirectory(groups: ToolCatalog.build(tools: tools, runtimeInfo: ToolFixtures.runtimeInfo))
        XCTAssertFalse(free.suggestions(for: scan).contains("pdf.ocr"))
        let pro = ToolDirectory(groups: ToolCatalog.build(tools: tools, runtimeInfo: ToolFixtures.runtimeInfo, plan: .pro))
        XCTAssertEqual(pro.suggestions(for: scan).first, "pdf.ocr")
    }

    func testPaletteItemsUseTheRegistrySummaryAsTheOutcome() {
        let item = ToolDirectory.paletteItem(ToolFixtures.entry("pdf.compress"))
        XCTAssertEqual(item.title, "Compress")
        XCTAssertEqual(item.outcome, "Make the file smaller, keep text sharp.")
        XCTAssertEqual(item.systemImage, "arrow.down.right.and.arrow.up.left")
        XCTAssertTrue(item.keywords.contains("pdf compress"))
    }

    func testEverySymbolExistsOnThisSystem() {
        let names = Set(ToolDirectory.symbols.values).union(ToolJob.allCases.map(\.systemImage))
        for name in names.sorted() {
            XCTAssertNotNil(NSImage(systemSymbolName: name, accessibilityDescription: nil), "\(name) is not an SF Symbol")
        }
    }

    func testSuggestionsComeFromTheDocumentFacts() {
        XCTAssertEqual(directory.suggestions(for: nil), [], "nothing before the facts arrive")
        XCTAssertEqual(directory.suggestions(for: ToolFixtures.report()), [])
        XCTAssertEqual(directory.suggestions(for: ToolFixtures.report(words: 0)), ["pdf.ocr"], "no text layer")
        XCTAssertEqual(directory.suggestions(for: ToolFixtures.report(pages: 0, words: 0)), [], "an empty document is no scan")
        XCTAssertEqual(directory.suggestions(for: ToolFixtures.report(bytes: 24_000_000)), ["pdf.compress"])
        XCTAssertEqual(directory.suggestions(for: ToolFixtures.report(title: " ")), ["pdf.metadata"])
        XCTAssertEqual(
            directory.suggestions(for: ToolFixtures.report(bytes: 24_000_000, words: 0, title: nil)),
            ["pdf.ocr", "pdf.compress", "pdf.metadata"]
        )
    }

    func testRecentsKeepTheNewestThreeWithoutRepeats() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "ashift-recents-\(UUID().uuidString)"))
        let recents = ToolRecents(defaults: defaults)
        XCTAssertEqual(recents.ids, [])
        for id in ["a", "b", "c", "a", "d"] { recents.record(id) }
        XCTAssertEqual(recents.ids, ["d", "a", "c"])
    }

    func testToastMessageNamesTheSizesOrTheFileCount() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("ashift-toast-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        let original = directory.appendingPathComponent("doc.pdf")
        let result = directory.appendingPathComponent("doc-compressed.pdf")
        try Data(count: 4_200_000).write(to: original)
        try Data(count: 1_100_000).write(to: result)

        XCTAssertEqual(ToolPresentation.message(for: [result], tool: "Compress", document: original), "Compress · 4.2 MB → 1.1 MB")
        XCTAssertEqual(ToolPresentation.message(for: [result, original], tool: "Split", document: original), "Split · 2 files created")
    }
}
