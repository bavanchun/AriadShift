import AriadDesignSystem
import AriadShiftCore
import Foundation

/// The job a tool does for the user. The registry has fourteen technical groups; people look for tools by job, so
/// the palette and the Tools tab show these six (plus Other for a registry group the app does not know yet).
enum ToolJob: String, CaseIterable, Sendable {
    case organize, edit, convert, secure, ocr, ai, other

    /// The registry groups gathered under this job, in the registry's menu order.
    var registryGroups: [String] {
        switch self {
        case .organize: ["organize"]
        case .edit: ["overlay", "annotate", "forms", "redact", "edit"]
        case .convert: ["convert", "optimize"]
        case .secure: ["security", "sign", "compare"]
        case .ocr: ["ocr"]
        case .ai: ["ai", "automation"]
        case .other: []
        }
    }

    /// The job of a registry group; groups the app does not know yet fall under Other.
    static func of(registryGroup group: String) -> ToolJob {
        allCases.first { $0.registryGroups.contains(group) } ?? .other
    }

    var title: String { L10n.branch("mac.tools.job", rawValue) }

    /// The symbol of a tool of this job that has no symbol of its own.
    var systemImage: String {
        switch self {
        case .organize: "square.grid.2x2"
        case .edit: "pencil"
        case .convert: "arrow.triangle.2.circlepath"
        case .secure: "lock"
        case .ocr: "text.viewfinder"
        case .ai: "sparkles"
        case .other: "wrench.and.screwdriver"
        }
    }
}

/// The tools of the open catalogue arranged for discovery: by job for the Tools tab and the palette, with Recent and
/// "Suggested for this file" sections on top of the palette.
struct ToolDirectory: Sendable {
    /// Tools grouped by job, in job order; jobs without tools are left out.
    let jobs: [(job: ToolJob, tools: [ToolEntry])]

    init(groups: [ToolGroup]) {
        var byJob: [ToolJob: [ToolEntry]] = [:]
        for group in groups {
            byJob[ToolJob.of(registryGroup: group.id), default: []].append(contentsOf: group.tools)
        }
        jobs = ToolJob.allCases.compactMap { job in
            guard let tools = byJob[job], !tools.isEmpty else { return nil }
            // Registry groups arrive in menu order and each is sorted by title; keep that order inside a job.
            return (job, tools)
        }
    }

    var isEmpty: Bool { jobs.isEmpty }

    func entry(id: String) -> ToolEntry? {
        for (_, tools) in jobs {
            if let tool = tools.first(where: { $0.id == id }) { return tool }
        }
        return nil
    }

    // MARK: Palette

    /// The palette sections: Recent, Suggested for this file, then every job. With an empty query a tool shows once,
    /// in the first section that holds it; a query searches every job, so Recent and Suggested only add rank order.
    func paletteSections(recent: [String], suggested: [String]) -> [DSPaletteSection] {
        let recentTools = recent.compactMap(entry(id:))
        let suggestedTools = suggested.compactMap(entry(id:)).filter { tool in !recentTools.contains { $0.id == tool.id } }
        let shown = Set((recentTools + suggestedTools).map(\.id))
        var sections: [DSPaletteSection] = []
        if !recentTools.isEmpty {
            sections.append(DSPaletteSection(
                id: "recent", title: String(localized: "mac.palette.recent"), items: recentTools.map(Self.paletteItem)
            ))
        }
        if !suggestedTools.isEmpty {
            sections.append(DSPaletteSection(
                id: "suggested", title: String(localized: "mac.palette.suggested"), items: suggestedTools.map(Self.paletteItem)
            ))
        }
        for (job, tools) in jobs {
            let rest = tools.filter { !shown.contains($0.id) }
            guard !rest.isEmpty else { continue }
            sections.append(DSPaletteSection(id: job.rawValue, title: job.title, items: rest.map(Self.paletteItem)))
        }
        return sections
    }

    /// The palette sections for a typed query: every job with every tool. Recent and Suggested are left out so a
    /// tool appears once, ranked inside its job.
    func searchSections() -> [DSPaletteSection] {
        jobs.map { DSPaletteSection(id: $0.job.rawValue, title: $0.job.title, items: $0.tools.map(Self.paletteItem)) }
    }

    // MARK: Tools tab

    /// The Tools tab: every job in order, filtered by `query` with the palette's fuzzy match over titles and keywords.
    /// An empty query keeps every tool in catalogue order; a query ranks the matches inside each job.
    func browserSections(query: String) -> [(job: ToolJob, tools: [ToolEntry])] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return jobs }
        let results = DSPaletteFilter.filter(searchSections(), query: trimmed)
        return results.compactMap { section in
            guard let job = ToolJob(rawValue: section.id) else { return nil }
            let tools = section.rows.compactMap { entry(id: $0.item.id) }
            return tools.isEmpty ? nil : (job, tools)
        }
    }

    // MARK: Presentation of one tool

    static func paletteItem(_ tool: ToolEntry) -> DSPaletteItem {
        DSPaletteItem(
            id: tool.id,
            title: tool.title,
            outcome: tool.info.summary,
            systemImage: systemImage(for: tool),
            tint: tint(for: tool),
            keywords: keywords(for: tool),
            badge: badge(for: tool)
        )
    }

    /// AI and automation results are transformations, so their icon wells use the thread gold.
    static func tint(for tool: ToolEntry) -> DSIconWell.Tint {
        ToolJob.of(registryGroup: tool.info.group) == .ai ? .thread : .accent
    }

    /// The badge of a tool that cannot run here, or nil when it can.
    static func badge(for tool: ToolEntry) -> String? {
        guard case let .unavailable(reason) = tool.availability else { return nil }
        return L10n.branch("mac.tools.badge", reason.tooltipBranch)
    }

    /// Words that find a tool without being part of its title: the words of its id and its job.
    static func keywords(for tool: ToolEntry) -> [String] {
        let words = tool.id.split(whereSeparator: { $0 == "." || $0 == "_" }).map(String.init)
        return [words.joined(separator: " "), tool.info.group]
    }

    static func systemImage(for tool: ToolEntry) -> String {
        symbols[tool.id] ?? ToolJob.of(registryGroup: tool.info.group).systemImage
    }

    /// SF Symbols per tool id. Every name is checked against the system in the unit tests.
    static let symbols: [String: String] = [
        // Organize
        "pdf.merge": "square.stack.3d.down.right",
        "pdf.split": "scissors",
        "pdf.reorder": "arrow.up.arrow.down",
        "pdf.extract_pages": "doc.on.doc",
        "pdf.insert_blank": "doc.badge.plus",
        "pdf.rotate": "rotate.right",
        "pdf.rotate_content": "rotate.left",
        "pdf.delete_pages": "trash",
        "pdf.crop": "crop",
        "pdf.scale": "arrow.up.left.and.arrow.down.right",
        "pdf.booklet": "book",
        "pdf.nup": "square.grid.2x2",
        "pdf.divide_pages": "rectangle.split.2x1",
        "pdf.posterize": "rectangle.split.3x3",
        "pdf.single_page": "rectangle.portrait",
        "pdf.overlay": "square.on.square",
        "pdf.alternate_mix": "arrow.triangle.merge",
        "pdf.info": "info.circle",
        "pdf.metadata": "tag",
        "pdf.outline": "list.bullet.indent",
        "pdf.attachments": "paperclip",
        "pdf.layers": "square.3.layers.3d",
        "pdf.page_labels": "number",
        "pdf.viewer_prefs": "eye",
        "pdf.portfolio": "folder",
        // Edit
        "pdf.watermark": "drop",
        "pdf.watermark_detect": "magnifyingglass",
        "pdf.watermark_remove": "eraser",
        "pdf.watermark_remove_raster": "eraser",
        "pdf.page_numbers": "number.square",
        "pdf.bates": "number.square",
        "pdf.header_footer": "rectangle.topthird.inset.filled",
        "pdf.stamp": "seal",
        "pdf.add_text": "textformat",
        "pdf.add_image": "photo",
        "pdf.qr": "qrcode",
        "pdf.background": "rectangle.fill",
        "pdf.toc_page": "list.number",
        "pdf.invert_colors": "circle.righthalf.filled",
        "pdf.adjust_colors": "slider.horizontal.3",
        "pdf.replace_color": "paintpalette",
        "pdf.annotations": "pencil.tip.crop.circle",
        "pdf.annotations_export": "square.and.arrow.up",
        "pdf.annotations_import": "square.and.arrow.down",
        "pdf.forms_fill": "character.cursor.ibeam",
        "pdf.forms_flatten": "square.stack",
        "pdf.redact": "eye.slash",
        "pdf.redact_verify": "checkmark.shield",
        // Convert
        "pdf.compress": "arrow.down.right.and.arrow.up.left",
        "pdf.decompress": "arrow.up.left.and.arrow.down.right",
        "pdf.grayscale": "circle.lefthalf.filled",
        "pdf.linearize": "bolt",
        "pdf.repair": "wrench.and.screwdriver",
        "pdf.to_images": "photo",
        "pdf.render": "photo",
        "pdf.extract_images": "photo.on.rectangle",
        "pdf.thumbnails": "square.grid.3x3",
        "pdf.to_pdfa": "checkmark.seal",
        "pdf.to_pdfua": "accessibility",
        "pdf.to_cbz": "books.vertical",
        "pdf.tables_to_csv": "tablecells",
        "pdf.text_layout": "text.alignleft",
        "image.to_pdf": "photo",
        "web.to_pdf": "globe",
        // Secure
        "pdf.encrypt": "lock",
        "pdf.decrypt": "lock.open",
        "pdf.unlock_forms": "lock.open",
        "pdf.permissions": "hand.raised",
        "pdf.sanitize": "shield.lefthalf.filled",
        "pdf.sign": "signature",
        "pdf.sign_visual": "signature",
        "pdf.sign_verify": "checkmark.seal",
        "pdf.sign_remove": "xmark.seal",
        "pdf.sign_token": "key",
        "pdf.timestamp": "clock",
        "pdf.compare": "rectangle.split.2x1",
        "pdf.validate_pdfa": "checkmark.seal",
        "pdf.validate_pdfua": "accessibility",
        "pdf.accessibility_report": "accessibility",
        // OCR and scans
        "pdf.ocr": "text.viewfinder",
        "image.ocr": "text.viewfinder",
        "pdf.deskew": "skew",
        "pdf.auto_rotate": "rotate.right",
        "pdf.scan_split": "doc.viewfinder",
        "pdf.scanner_effect": "scanner",
        "pdf.detect_scanned": "doc.text.magnifyingglass",
        "pdf.remove_blank": "doc",
        "pdf.crop_whitespace": "crop",
        // AI
        "ai.summarize": "sparkles",
        "ai.ask": "bubble.left.and.text.bubble.right",
        "ai.translate": "translate",
        "ai.extract_tables": "tablecells",
        "ai.recipe_from_text": "wand.and.stars",
    ]

    // MARK: Suggestions

    /// A document of this size or larger gets Compress suggested.
    static let largeFileBytes: UInt64 = 10_000_000

    /// Tools that suit the open document, from facts the engine read from it: a document without a text layer gets
    /// OCR, a large one Compress, and one without a title Metadata. Only tools that can run are suggested, at most
    /// three, and nothing is suggested before the facts arrive.
    func suggestions(for report: InspectReport?) -> [String] {
        guard let report else { return [] }
        var ids: [String] = []
        if let words = report.wordCount, words == 0, (report.pageCount ?? 0) > 0 { ids.append("pdf.ocr") }
        if report.bytes >= Self.largeFileBytes { ids.append("pdf.compress") }
        if (report.title ?? "").trimmingCharacters(in: .whitespaces).isEmpty { ids.append("pdf.metadata") }
        return Array(ids.filter { entry(id: $0)?.isAvailable == true }.prefix(3))
    }
}
