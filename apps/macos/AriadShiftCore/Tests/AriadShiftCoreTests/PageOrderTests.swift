import XCTest

@testable import AriadShiftCore

/// Page lists for the page tools, and the selection and zoom rules of the viewer.
final class PageOrderTests: XCTestCase {
    func testMovingOnePageForwardAndBackward() {
        XCTAssertEqual(PageOrder.moved([0], toInsertionPoint: 3, pageCount: 5), [1, 2, 0, 3, 4], "page 1 goes before page 4")
        XCTAssertEqual(PageOrder.moved([3], toInsertionPoint: 1, pageCount: 5), [0, 3, 1, 2, 4])
        XCTAssertEqual(PageOrder.moved([0], toInsertionPoint: 5, pageCount: 5), [1, 2, 3, 4, 0], "after the last page")
        XCTAssertEqual(PageOrder.moved([4], toInsertionPoint: 0, pageCount: 5), [4, 0, 1, 2, 3], "before the first page")
    }

    func testMovingSeveralPagesKeepsTheirRelativeOrder() {
        XCTAssertEqual(PageOrder.moved([1, 3], toInsertionPoint: 5, pageCount: 6), [0, 2, 4, 1, 3, 5], "before the last page")
        XCTAssertEqual(PageOrder.moved([4, 2], toInsertionPoint: 0, pageCount: 6), [2, 4, 0, 1, 3, 5])
        XCTAssertEqual(PageOrder.moved([1, 4], toInsertionPoint: 3, pageCount: 6), [0, 2, 1, 4, 3, 5], "pages before the point and after it")
    }

    func testAMoveThatChangesNothingIsNotARequest() {
        XCTAssertNil(PageOrder.moved([2], toInsertionPoint: 2, pageCount: 5), "dropped before itself")
        XCTAssertNil(PageOrder.moved([2], toInsertionPoint: 3, pageCount: 5), "dropped after itself")
        XCTAssertNil(PageOrder.moved([1, 2], toInsertionPoint: 1, pageCount: 5))
        XCTAssertNil(PageOrder.moved([], toInsertionPoint: 1, pageCount: 5))
        XCTAssertNil(PageOrder.moved([7], toInsertionPoint: 1, pageCount: 5), "page out of range")
        XCTAssertNil(PageOrder.moved([1], toInsertionPoint: 9, pageCount: 5), "point out of range")
        XCTAssertNil(PageOrder.moved([0], toInsertionPoint: 0, pageCount: 1))
    }

    func testEveryMoveIsAPermutation() {
        for count in 2...7 {
            for moving in [Set([0]), [count - 1], [0, count - 1], Set(0..<count / 2)] {
                for point in 0...count {
                    guard let order = PageOrder.moved(moving, toInsertionPoint: point, pageCount: count) else { continue }
                    XCTAssertEqual(order.sorted(), Array(0..<count), "\(moving) -> \(point) of \(count)")
                }
            }
        }
    }

    func testShiftingTheSelectionByOnePosition() {
        XCTAssertEqual(PageOrder.shifted([2], by: -1, pageCount: 5), [0, 2, 1, 3, 4])
        XCTAssertEqual(PageOrder.shifted([2], by: 1, pageCount: 5), [0, 1, 3, 2, 4])
        XCTAssertEqual(PageOrder.shifted([1, 2], by: 1, pageCount: 5), [0, 3, 1, 2, 4])
        XCTAssertNil(PageOrder.shifted([0], by: -1, pageCount: 5), "already first")
        XCTAssertNil(PageOrder.shifted([4], by: 1, pageCount: 5), "already last")
        XCTAssertNil(PageOrder.shifted([2], by: 2, pageCount: 5))
    }

    /// Separate pages each move by one position; they are never gathered into a block.
    func testShiftingANonContiguousSelectionMovesEveryRunByOne() {
        XCTAssertEqual(PageOrder.shifted([1, 5], by: -1, pageCount: 8), [1, 0, 2, 3, 5, 4, 6, 7])
        XCTAssertEqual(PageOrder.shifted([1, 5], by: 1, pageCount: 8), [0, 2, 1, 3, 4, 6, 5, 7])
        XCTAssertEqual(PageOrder.shifted([1, 2, 5], by: 1, pageCount: 8), [0, 3, 1, 2, 4, 6, 5, 7])
        // A run at the edge stays while the other run moves.
        XCTAssertEqual(PageOrder.shifted([0, 3], by: -1, pageCount: 5), [0, 1, 3, 2, 4])
        XCTAssertEqual(PageOrder.shifted([1, 4], by: 1, pageCount: 5), [0, 2, 1, 3, 4])
        XCTAssertNil(PageOrder.shifted([0, 1], by: -1, pageCount: 5))
        XCTAssertNil(PageOrder.shifted([], by: 1, pageCount: 5))
        XCTAssertNil(PageOrder.shifted([9], by: 1, pageCount: 5), "out of range")
        // Two runs with one page between them still use different neighbours.
        XCTAssertEqual(PageOrder.shifted([1, 3], by: -1, pageCount: 5), [1, 0, 3, 2, 4])
    }

    func testPageRangeTextCompressesAscendingRunsOnly() {
        XCTAssertEqual(PageOrder.pageRangeText([2, 0, 1, 3, 4, 5]), "3,1,2,4-6")
        XCTAssertEqual(PageOrder.pageRangeText([0, 1, 2]), "1-3")
        XCTAssertEqual(PageOrder.pageRangeText([3, 2, 1, 0]), "4,3,2,1", "a descending run stays listed page by page")
        XCTAssertEqual(PageOrder.pageRangeText([]), "")
        // One page moved in a 1,000-page document stays short.
        let order = PageOrder.moved([0], toInsertionPoint: 500, pageCount: 1000)
        XCTAssertEqual(PageOrder.pageRangeText(try XCTUnwrap(order)), "2-500,1,501-1000")
    }

    func testPageRangeTextExpandsBackToTheSameList() throws {
        let order = try XCTUnwrap(PageOrder.moved([3, 4, 9], toInsertionPoint: 1, pageCount: 12))
        let text = PageOrder.pageRangeText(order)
        let expanded = text.split(separator: ",").flatMap { token -> [Int] in
            let ends = token.split(separator: "-").compactMap { Int($0) }
            return ends.count == 2 ? Array((ends[0] - 1)...(ends[1] - 1)) : [ends[0] - 1]
        }
        XCTAssertEqual(expanded, order)
    }

    func testParameterJSON() {
        XCTAssertEqual(PageOrder.reorderParams([1, 0, 2]), #"{"order":"2,1,3"}"#)
        XCTAssertEqual(PageOrder.deleteParams([4, 1, 2, 3]), #"{"pages":"2-5"}"#)
        XCTAssertEqual(PageOrder.rotateParams([0, 2]), #"{"angle":90,"pages":"1,3"}"#)
        XCTAssertEqual(PageOrder.rotateParams([0], angle: -90), #"{"angle":-90,"pages":"1"}"#)
    }

    func testClickSelection() {
        var selection = PageSelection()
        selection.click(2, .plain)
        XCTAssertEqual(selection.pages, [2])
        selection.click(5, .toggle)
        XCTAssertEqual(selection.pages, [2, 5])
        selection.click(2, .toggle)
        XCTAssertEqual(selection.pages, [5])
        selection.click(8, .extend)
        XCTAssertEqual(selection.pages, Set(2...8), "extends from the page clicked last")
        selection.click(4, .extend)
        XCTAssertEqual(selection.pages, Set(2...4))
        selection.click(7, .plain)
        XCTAssertEqual(selection.pages, [7])
        selection.set([1, 4])
        XCTAssertEqual(selection.anchor, 1)
    }

    func testZoomStepsStayInsideTheLimits() {
        XCTAssertEqual(ZoomLevel.zoomedIn(from: 1), 1.25)
        XCTAssertEqual(ZoomLevel.zoomedOut(from: 1), 0.75)
        XCTAssertEqual(ZoomLevel.zoomedIn(from: 1.1), 1.25, "between stops goes to the next one")
        XCTAssertEqual(ZoomLevel.zoomedOut(from: 1.1), 1)
        XCTAssertEqual(ZoomLevel.zoomedIn(from: 8), 8)
        XCTAssertEqual(ZoomLevel.zoomedOut(from: 0.1), 0.1)
        XCTAssertEqual(ZoomLevel.zoomedIn(from: 100), 8, "an out-of-range scale is clamped first")
        XCTAssertEqual(ZoomLevel.zoomedOut(from: 0), 0.1)
        XCTAssertEqual(ZoomLevel.clamped(.nan), 1)
        XCTAssertEqual(ZoomLevel.clamped(-3), ZoomLevel.minimum)
        // Repeated zooming reaches both ends and stops.
        var scale = 1.0
        for _ in 0..<40 { scale = ZoomLevel.zoomedIn(from: scale) }
        XCTAssertEqual(scale, ZoomLevel.maximum)
        for _ in 0..<40 { scale = ZoomLevel.zoomedOut(from: scale) }
        XCTAssertEqual(scale, ZoomLevel.minimum)
    }

    func testEditsBecomeRegistryToolRequests() {
        XCTAssertEqual(
            PageEdit.move([0], before: 3).request(pageCount: 5),
            PageEditRequest(toolID: "pdf.reorder", paramsJSON: #"{"order":"2,3,1,4,5"}"#)
        )
        XCTAssertEqual(
            PageEdit.shift([2], by: -1).request(pageCount: 5),
            PageEditRequest(toolID: "pdf.reorder", paramsJSON: #"{"order":"1,3,2,4,5"}"#)
        )
        XCTAssertEqual(
            PageEdit.rotate([1, 2, 3], angle: 90).request(pageCount: 5),
            PageEditRequest(toolID: "pdf.rotate", paramsJSON: #"{"angle":90,"pages":"2-4"}"#)
        )
        XCTAssertEqual(
            PageEdit.delete([0, 4]).request(pageCount: 5),
            PageEditRequest(toolID: "pdf.delete_pages", paramsJSON: #"{"pages":"1,5"}"#)
        )
    }

    func testEditsThatChangeNothingOrCannotRunMakeNoRequest() {
        XCTAssertNil(PageEdit.move([2], before: 2).request(pageCount: 5))
        XCTAssertNil(PageEdit.shift([0], by: -1).request(pageCount: 5))
        XCTAssertNil(PageEdit.rotate([], angle: 90).request(pageCount: 5))
        XCTAssertNil(PageEdit.rotate([1], angle: 45).request(pageCount: 5))
        XCTAssertNil(PageEdit.rotate([9], angle: 90).request(pageCount: 5))
        XCTAssertNil(PageEdit.delete([]).request(pageCount: 5))
        XCTAssertNil(PageEdit.delete(Set(0..<5)).request(pageCount: 5), "a document keeps at least one page")
        XCTAssertNil(PageEdit.delete([7]).request(pageCount: 5))
    }

    func testPageDragPayloadRoundTripsAndIgnoresForeignText() {
        let payload = PageDrag.payload([4, 1, 2])
        XCTAssertEqual(PageDrag.pages(from: payload), [1, 2, 4])
        XCTAssertNil(PageDrag.pages(from: "hello"))
        XCTAssertNil(PageDrag.pages(from: "ashift-pages:"))
        XCTAssertNil(PageDrag.pages(from: "ashift-pages:1,x"))
        XCTAssertNil(PageDrag.pages(from: "ashift-pages:-1"))
    }
}
