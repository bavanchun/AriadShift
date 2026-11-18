import AriadShiftCore
import Foundation

/// A small, fixed tool catalogue for model and snapshot tests, so they do not change when the registry grows.
/// It goes through the real `ToolCatalog.build`, so availability, titles and forms are computed as in the app.
enum ToolFixtures {
    static let compressSchema = """
    {"type":"object","properties":{
      "preset":{"type":"string","enum":["balanced","smallest","high"],"default":"balanced","description":"How hard to compress."},
      "grayscale":{"type":"boolean","default":false,"title":"Grayscale"}
    }}
    """

    static let rotateSchema = """
    {"type":"object","required":["angle"],"properties":{
      "angle":{"type":"integer","enum":[90,180,270],"description":"Clockwise turn."},
      "pages":{"type":"string","description":"Pages to turn, for example 1-3,7."}
    }}
    """

    static let emptySchema = #"{"type":"object","properties":{}}"#

    static func info(
        _ id: String, group: String, summary: String, schema: String = emptySchema, tier: String = "free"
    ) -> ToolInfo {
        ToolInfo(
            id: id, version: "1", group: group, summary: summary, inputFormats: ["pdf"], outputFormat: "pdf",
            outputFormats: ["pdf"], tier: tier, engine: "builtin", paramsSchemaJson: schema
        )
    }

    static let tools: [ToolInfo] = [
        info("pdf.merge", group: "organize", summary: "Combine files into one document."),
        info("pdf.rotate", group: "organize", summary: "Turn pages a quarter at a time.", schema: rotateSchema),
        info("pdf.extract_pages", group: "organize", summary: "Copy chosen pages into a new file."),
        info("pdf.metadata", group: "organize", summary: "Edit the title, author and keywords."),
        info("pdf.watermark", group: "overlay", summary: "Stamp text across every page."),
        info("pdf.annotations", group: "annotate", summary: "List the comments and highlights."),
        info("pdf.compress", group: "optimize", summary: "Make the file smaller, keep text sharp.", schema: compressSchema),
        info("pdf.to_pdfa", group: "convert", summary: "Make an archival PDF/A copy."),
        info("pdf.encrypt", group: "security", summary: "Lock the file with a password."),
        info("pdf.validate_pdfa", group: "compare", summary: "Check the file against PDF/A.", tier: "pro"),
        info("pdf.ocr", group: "ocr", summary: "Make scanned pages searchable."),
        info("ai.summarize", group: "ai", summary: "A short summary of the whole document."),
    ]

    /// Every fixture tool takes the open document only; merge takes more.
    static let runtimeInfo: [String: ToolRuntimeInfo] = Dictionary(uniqueKeysWithValues: tools.map { tool in
        (tool.id, ToolRuntimeInfo(arity: tool.id == "pdf.merge" ? InputArity(min: 2, max: 10) : .one))
    })

    static let groups = ToolCatalog.build(tools: tools, runtimeInfo: runtimeInfo)

    static func entry(_ id: String) -> ToolEntry {
        guard let entry = groups.flatMap(\.tools).first(where: { $0.id == id }) else {
            preconditionFailure("no fixture tool \(id)")
        }
        return entry
    }

    static func report(bytes: UInt64 = 1_200_000, pages: UInt64? = 12, words: UInt64? = 3_400, title: String? = "Board report")
        -> InspectReport
    {
        InspectReport(
            format: "pdf", bytes: bytes, pageCount: pages, wordCount: words, title: title, author: nil,
            reachableFormats: [], warnings: []
        )
    }
}
