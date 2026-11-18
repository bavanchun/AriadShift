import Foundation

/// Builds the page lists the page tools take (`pdf.reorder`, `pdf.delete_pages`, `pdf.rotate`).
///
/// This is request building, not document logic: it turns what the user did in the page grid
/// (move these pages there, pick these pages) into the registry's page-range text and nothing
/// more. The engine validates the list and performs the edit.
public enum PageOrder {
    /// The page order after moving `moving` so the moved pages sit together just before the page
    /// that is now at `insertionPoint` (`pageCount` means after the last page). Pages are 0-based and
    /// keep their relative order. The result lists, for every new position, the old page index.
    ///
    /// Returns nil when nothing would change (including an empty or out-of-range move).
    public static func moved(_ moving: Set<Int>, toInsertionPoint insertionPoint: Int, pageCount: Int) -> [Int]? {
        guard pageCount > 1, !moving.isEmpty, moving.allSatisfy({ (0..<pageCount).contains($0) }),
            (0...pageCount).contains(insertionPoint)
        else { return nil }
        let moved = moving.sorted()
        let staying = (0..<pageCount).filter { !moving.contains($0) }
        // Pages that stay in front of the insertion point keep their place before the moved block.
        let before = insertionPoint - moved.filter { $0 < insertionPoint }.count
        let order = Array(staying[..<before]) + moved + Array(staying[before...])
        return order == Array(0..<pageCount) ? nil : order
    }

    /// The order after shifting the selected pages one position toward the start (`-1`) or end (`1`).
    ///
    /// Every maximal run of neighbouring selected pages swaps places with the one page next to it, so
    /// a selection of separate pages (`{1, 5}`) moves each page by one and never gathers them. A run
    /// already at the edge stays; nil when no page would move.
    public static func shifted(_ selection: Set<Int>, by step: Int, pageCount: Int) -> [Int]? {
        guard pageCount > 1, step == -1 || step == 1, !selection.isEmpty,
            selection.allSatisfy({ (0..<pageCount).contains($0) })
        else { return nil }
        var order = Array(0..<pageCount)
        for run in runs(of: selection.sorted()) {
            if step < 0, run.lowerBound > 0 {
                order.replaceSubrange((run.lowerBound - 1)...run.upperBound, with: Array(run) + [run.lowerBound - 1])
            } else if step > 0, run.upperBound < pageCount - 1 {
                order.replaceSubrange(run.lowerBound...(run.upperBound + 1), with: [run.upperBound + 1] + Array(run))
            }
        }
        return order == Array(0..<pageCount) ? nil : order
    }

    /// The maximal runs of consecutive numbers in an ascending list.
    private static func runs(of sorted: [Int]) -> [ClosedRange<Int>] {
        var result: [ClosedRange<Int>] = []
        for value in sorted {
            if let last = result.last, last.upperBound + 1 == value {
                result[result.count - 1] = last.lowerBound...value
            } else {
                result.append(value...value)
            }
        }
        return result
    }

    /// Page-range text for `pages` (0-based, order kept): 1-based numbers where an ascending run
    /// of three or more pages becomes `a-b`, so `[2, 0, 1, 3, 4, 5]` gives `3,1,2,4-6`. A 1,000-page
    /// document with one page moved stays a short list.
    public static func pageRangeText(_ pages: [Int]) -> String {
        var tokens: [String] = []
        var index = 0
        while index < pages.count {
            var end = index
            while end + 1 < pages.count, pages[end + 1] == pages[end] + 1 { end += 1 }
            if end - index >= 2 {
                tokens.append("\(pages[index] + 1)-\(pages[end] + 1)")
                index = end + 1
            } else {
                tokens.append("\(pages[index] + 1)")
                index += 1
            }
        }
        return tokens.joined(separator: ",")
    }

    /// The `order` parameter of `pdf.reorder` for a full page order, as JSON.
    public static func reorderParams(_ order: [Int]) -> String {
        "{\"order\":\"\(pageRangeText(order))\"}"
    }

    /// The `pages` parameter of `pdf.delete_pages` for the selected pages, as JSON.
    public static func deleteParams(_ selection: Set<Int>) -> String {
        "{\"pages\":\"\(pageRangeText(selection.sorted()))\"}"
    }

    /// The parameters of `pdf.rotate` for the selected pages, a clockwise quarter turn by default.
    public static func rotateParams(_ selection: Set<Int>, angle: Int = 90) -> String {
        "{\"angle\":\(angle),\"pages\":\"\(pageRangeText(selection.sorted()))\"}"
    }
}

/// Click selection in a grid of pages: plain click, command-click toggle and shift-click range.
public struct PageSelection: Equatable, Sendable {
    public private(set) var pages: Set<Int>
    /// The page clicked last: a shift-click extends the selection from it.
    public private(set) var anchor: Int?

    public init(pages: Set<Int> = [], anchor: Int? = nil) {
        self.pages = pages
        self.anchor = anchor
    }

    public enum Click: Sendable { case plain, toggle, extend }

    public mutating func click(_ page: Int, _ kind: Click) {
        switch kind {
        case .plain:
            pages = [page]
            anchor = page
        case .toggle:
            if pages.contains(page) { pages.remove(page) } else { pages.insert(page) }
            anchor = page
        case .extend:
            let from = anchor ?? page
            pages = Set(min(from, page)...max(from, page))
        }
    }

    /// Replaces the selection (for example with the page the viewer scrolled to).
    public mutating func set(_ pages: Set<Int>) {
        self.pages = pages
        anchor = pages.min()
    }
}

/// A page edit the user asked for in the page grid, as the registry tool that performs it.
public enum PageEdit: Equatable, Sendable {
    /// Move pages so they sit before the page at this index (`pageCount` means the end).
    case move(Set<Int>, before: Int)
    /// Move the pages one position toward the start (-1) or the end (1).
    case shift(Set<Int>, by: Int)
    case rotate(Set<Int>, angle: Int)
    case delete(Set<Int>)

    /// The tool and parameters for this edit on a document of `pageCount` pages, or nil when the
    /// edit changes nothing or cannot be asked for (no pages, a page out of range, deleting every page).
    public func request(pageCount: Int) -> PageEditRequest? {
        switch self {
        case let .move(pages, before):
            PageOrder.moved(pages, toInsertionPoint: before, pageCount: pageCount).map {
                PageEditRequest(toolID: "pdf.reorder", paramsJSON: PageOrder.reorderParams($0))
            }
        case let .shift(pages, step):
            PageOrder.shifted(pages, by: step, pageCount: pageCount).map {
                PageEditRequest(toolID: "pdf.reorder", paramsJSON: PageOrder.reorderParams($0))
            }
        case let .rotate(pages, angle):
            valid(pages, pageCount) && [90, 180, 270, -90].contains(angle)
                ? PageEditRequest(toolID: "pdf.rotate", paramsJSON: PageOrder.rotateParams(pages, angle: angle)) : nil
        case let .delete(pages):
            valid(pages, pageCount) && pages.count < pageCount
                ? PageEditRequest(toolID: "pdf.delete_pages", paramsJSON: PageOrder.deleteParams(pages)) : nil
        }
    }

    private func valid(_ pages: Set<Int>, _ pageCount: Int) -> Bool {
        !pages.isEmpty && pages.allSatisfy { (0..<pageCount).contains($0) }
    }
}

public struct PageEditRequest: Equatable, Sendable {
    public let toolID: String
    public let paramsJSON: String
}

/// What a page drag carries: the dragged pages as plain text, so no custom type has to be declared.
/// Text from anywhere else is not a page drag and is ignored.
public enum PageDrag {
    private static let prefix = "ashift-pages:"

    public static func payload(_ pages: Set<Int>) -> String {
        prefix + pages.sorted().map(String.init).joined(separator: ",")
    }

    public static func pages(from payload: String) -> Set<Int>? {
        guard payload.hasPrefix(prefix) else { return nil }
        let numbers = payload.dropFirst(prefix.count).split(separator: ",").map { Int($0) }
        guard !numbers.isEmpty, !numbers.contains(nil) else { return nil }
        let pages = Set(numbers.compactMap { $0 })
        return pages.allSatisfy { $0 >= 0 } ? pages : nil
    }
}
