import CoreGraphics

/// One command in the palette.
public struct DSPaletteItem: Identifiable, Hashable, Sendable {
    public let id: String
    public let title: String
    /// The one-line outcome under the title ("Make the file smaller, keep text sharp.").
    public let outcome: String
    public let systemImage: String
    public let tint: DSIconWell.Tint
    /// A display string such as "⌘⇧C", or nil.
    public let shortcut: String?
    /// Extra words that match without being highlighted (for example "shrink" for Compress).
    public let keywords: [String]
    /// A short badge such as "Pro" for a tool that cannot run yet, or nil.
    public let badge: String?

    public init(
        id: String,
        title: String,
        outcome: String,
        systemImage: String,
        tint: DSIconWell.Tint = .accent,
        shortcut: String? = nil,
        keywords: [String] = [],
        badge: String? = nil
    ) {
        self.id = id
        self.title = title
        self.outcome = outcome
        self.systemImage = systemImage
        self.tint = tint
        self.shortcut = shortcut
        self.keywords = keywords
        self.badge = badge
    }
}

/// A titled group of palette items: Recent, Suggested for this file, then each tool group.
public struct DSPaletteSection: Identifiable, Hashable, Sendable {
    public let id: String
    public let title: String
    public let items: [DSPaletteItem]

    public init(id: String, title: String, items: [DSPaletteItem]) {
        self.id = id
        self.title = title
        self.items = items
    }
}

/// A row of the filtered palette: the item, its section and the title characters to highlight.
public struct DSPaletteRowModel: Identifiable, Hashable, Sendable {
    /// Unique across sections: the same tool can be both Recent and in its group.
    public let id: String
    public let item: DSPaletteItem
    public let highlightedOffsets: [Int]
    let score: Int
}

/// A section after filtering, with its surviving rows in rank order.
public struct DSPaletteResultSection: Identifiable, Hashable, Sendable {
    public let id: String
    public let title: String
    public let rows: [DSPaletteRowModel]
}

public enum DSPaletteFilter {
    /// Filters and ranks every section by `query`. An empty query keeps the given order. Sections left without
    /// rows are dropped. Items that match only a keyword rank below every title match and highlight nothing.
    public static func filter(_ sections: [DSPaletteSection], query: String) -> [DSPaletteResultSection] {
        sections.compactMap { section in
            let rows = section.items.enumerated().compactMap { index, item -> (DSPaletteRowModel, Int)? in
                let rowID = "\(section.id)/\(item.id)"
                if let match = DSFuzzyMatcher.match(query, in: item.title) {
                    return (DSPaletteRowModel(id: rowID, item: item, highlightedOffsets: match.matchedOffsets, score: match.score), index)
                }
                let keywordScore = item.keywords.compactMap { DSFuzzyMatcher.match(query, in: $0)?.score }.max()
                guard let keywordScore else { return nil }
                return (DSPaletteRowModel(id: rowID, item: item, highlightedOffsets: [], score: keywordScore - keywordPenalty), index)
            }
            guard !rows.isEmpty else { return nil }
            let ranked = rows
                .sorted { lhs, rhs in lhs.0.score != rhs.0.score ? lhs.0.score > rhs.0.score : lhs.1 < rhs.1 }
                .map(\.0)
            return DSPaletteResultSection(id: section.id, title: section.title, rows: ranked)
        }
    }

    static let keywordPenalty = 1000
}

/// Layout rules of the palette results list.
public enum DSPaletteLayout {
    /// The visible list height: the full content when it fits under `maxHeight`, otherwise the tallest height
    /// that ends on a whole row. A cut never lands mid-row or directly below a group header.
    public static func visibleHeight(
        rowsPerSection: [Int],
        maxHeight: CGFloat = DSSize.paletteListMaxHeight,
        headerHeight: CGFloat = DSControlHeight.paletteHeader,
        rowHeight: CGFloat = DSControlHeight.paletteRow
    ) -> CGFloat {
        var total: CGFloat = 0
        var lastFittingRowEnd: CGFloat = 0
        for rows in rowsPerSection where rows > 0 {
            total += headerHeight
            for _ in 0..<rows {
                total += rowHeight
                if total <= maxHeight { lastFittingRowEnd = total }
            }
        }
        return total <= maxHeight ? total : lastFittingRowEnd
    }
}
