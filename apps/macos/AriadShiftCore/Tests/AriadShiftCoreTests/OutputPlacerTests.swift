import Foundation
import Synchronization
import XCTest

@testable import AriadShiftCore

final class OutputPlacerTests: XCTestCase {
    func testUniqueURLKeepsFreeNamesAndNumbersTakenOnes() throws {
        let directory = try temporaryDirectory()
        let wanted = directory.appendingPathComponent("report.pdf")
        XCTAssertEqual(OutputPlacer.uniqueURL(for: wanted), wanted)
        try Data("a".utf8).write(to: wanted)
        let second = OutputPlacer.uniqueURL(for: wanted)
        XCTAssertEqual(second.lastPathComponent, "report 2.pdf")
        try Data("b".utf8).write(to: second)
        XCTAssertEqual(OutputPlacer.uniqueURL(for: wanted).lastPathComponent, "report 3.pdf")
    }

    func testPlaceMovesFilesWithoutReplacingAnything() throws {
        let staging = try temporaryDirectory()
        let destination = try temporaryDirectory()
        let existing = destination.appendingPathComponent("out.pdf")
        try Data("original".utf8).write(to: existing)
        let produced = staging.appendingPathComponent("out.pdf")
        try Data("new".utf8).write(to: produced)

        let placed = try OutputPlacer.place([produced], into: destination)

        XCTAssertEqual(placed.map(\.lastPathComponent), ["out 2.pdf"])
        XCTAssertEqual(try String(contentsOf: existing, encoding: .utf8), "original")
        XCTAssertEqual(try String(contentsOf: placed[0], encoding: .utf8), "new")
        XCTAssertFalse(FileManager.default.fileExists(atPath: produced.path), "the staged file was moved")
    }

    func testDestinationIsCustomFolderWhenItExistsElseTheInputFolder() throws {
        let inputFolder = try temporaryDirectory()
        let custom = try temporaryDirectory()
        let input = inputFolder.appendingPathComponent("a.pdf")
        XCTAssertEqual(OutputPlacer.destinationDirectory(for: input, customFolder: nil), inputFolder)
        XCTAssertEqual(OutputPlacer.destinationDirectory(for: input, customFolder: custom), custom)
        let missing = custom.appendingPathComponent("gone")
        XCTAssertEqual(OutputPlacer.destinationDirectory(for: input, customFolder: missing), inputFolder)
    }

    // MARK: - Names that are taken

    func testDanglingSymbolicLinkCountsAsTaken() throws {
        let directory = try temporaryDirectory()
        let link = directory.appendingPathComponent("out.pdf")
        try FileManager.default.createSymbolicLink(atPath: link.path, withDestinationPath: "/nonexistent/ashift-target")
        XCTAssertFalse(FileManager.default.fileExists(atPath: link.path), "the link dangles: fileExists follows it")
        XCTAssertEqual(OutputPlacer.uniqueURL(for: link).lastPathComponent, "out 2.pdf")
    }

    func testPlaceLeavesADanglingSymbolicLinkAlone() throws {
        let staging = try temporaryDirectory()
        let destination = try temporaryDirectory()
        let link = destination.appendingPathComponent("out.pdf")
        try FileManager.default.createSymbolicLink(atPath: link.path, withDestinationPath: "/nonexistent/ashift-target")
        let produced = staging.appendingPathComponent("out.pdf")
        try Data("new".utf8).write(to: produced)

        let placed = try OutputPlacer.place([produced], into: destination)

        XCTAssertEqual(placed.map(\.lastPathComponent), ["out 2.pdf"])
        XCTAssertEqual(try FileManager.default.destinationOfSymbolicLink(atPath: link.path), "/nonexistent/ashift-target")
    }

    func testReservedNamesAreSkippedLikeFilesOnDisk() throws {
        let directory = try temporaryDirectory()
        let wanted = directory.appendingPathComponent("x.pdf")
        let reservations = NameReservations()
        XCTAssertEqual(reservations.reserve(wanted).lastPathComponent, "x.pdf")
        XCTAssertEqual(reservations.reserve(wanted).lastPathComponent, "x 2.pdf")
        XCTAssertEqual(reservations.reserve(directory.appendingPathComponent("X.PDF")).lastPathComponent, "X 3.PDF", "case-insensitive")
        reservations.release(directory.appendingPathComponent("x 2.pdf"))
        XCTAssertEqual(reservations.reserve(wanted).lastPathComponent, "x 2.pdf", "a released name is free again")
    }

    // MARK: - Concurrent writers

    func testConcurrentPlacementsKeepEveryFile() throws {
        let staging = try temporaryDirectory()
        let destination = try temporaryDirectory()
        let count = 64
        let sources: [URL] = try (0..<count).map { index in
            let folder = staging.appendingPathComponent("s\(index)", isDirectory: true)
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let file = folder.appendingPathComponent("out.pdf")
            try Data("file-\(index)".utf8).write(to: file)
            return file
        }
        let results = Mutex<[Result<URL, Error>]>([])
        DispatchQueue.concurrentPerform(iterations: count) { index in
            let outcome = Result { try OutputPlacer.place([sources[index]], into: destination)[0] }
            results.withLock { $0.append(outcome) }
        }
        let placed = try results.withLock { $0 }.map { try $0.get() }
        XCTAssertEqual(Set(placed.map(\.path)).count, count, "every file got its own name")
        let contents = try placed.map { try String(contentsOf: $0, encoding: .utf8) }
        XCTAssertEqual(Set(contents), Set((0..<count).map { "file-\($0)" }), "no file was replaced")
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: destination.path).count, count)
    }

    /// The cross-volume path copies first and renames exclusively; it must not replace either.
    func testCopyExclusivelyRefusesAnExistingTargetAndKeepsBothFiles() throws {
        let staging = try temporaryDirectory()
        let destination = try temporaryDirectory()
        let source = staging.appendingPathComponent("out.pdf")
        try Data("new".utf8).write(to: source)
        let target = destination.appendingPathComponent("out.pdf")
        try Data("original".utf8).write(to: target)

        XCTAssertThrowsError(try OutputPlacer.copyExclusively(source, to: target)) { error in
            XCTAssertEqual((error as? CocoaError)?.code, .fileWriteFileExists)
        }
        XCTAssertEqual(try String(contentsOf: target, encoding: .utf8), "original")
        XCTAssertEqual(try String(contentsOf: source, encoding: .utf8), "new", "the source stays when nothing was placed")
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: destination.path), ["out.pdf"], "no scratch file is left")

        let free = destination.appendingPathComponent("free.pdf")
        try OutputPlacer.copyExclusively(source, to: free)
        XCTAssertEqual(try String(contentsOf: free, encoding: .utf8), "new")
        XCTAssertFalse(FileManager.default.fileExists(atPath: source.path), "the source is removed after the copy")
    }

    func testConcurrentExclusiveCopiesOfOneNameYieldExactlyOneWinner() throws {
        let staging = try temporaryDirectory()
        let destination = try temporaryDirectory()
        let count = 16
        let sources: [URL] = try (0..<count).map { index in
            let file = staging.appendingPathComponent("s\(index).pdf")
            try Data("file-\(index)".utf8).write(to: file)
            return file
        }
        let target = destination.appendingPathComponent("out.pdf")
        let wins = Mutex(0)
        DispatchQueue.concurrentPerform(iterations: count) { index in
            do {
                try OutputPlacer.copyExclusively(sources[index], to: target)
                wins.withLock { $0 += 1 }
            } catch {
                XCTAssertEqual((error as? CocoaError)?.code, .fileWriteFileExists)
            }
        }
        XCTAssertEqual(wins.withLock { $0 }, 1)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: destination.path), ["out.pdf"])
    }

    // MARK: - Volumes without an exclusive rename

    /// A volume that answers `RENAME_EXCL` with `ENOTSUP`, as the real syscall would.
    private let withoutExclusiveRename = ExclusiveRename { _, _ in ENOTSUP }

    /// A source on another volume (`EXDEV`) whose destination also lacks exclusive rename.
    private let crossVolumeWithoutExclusiveRename = ExclusiveRename { source, _ in
        URL(fileURLWithPath: source).lastPathComponent.hasPrefix(".ashift-") ? ENOTSUP : EXDEV
    }

    private func stage(_ count: Int, in staging: URL) throws -> [URL] {
        try (0..<count).map { index in
            let folder = staging.appendingPathComponent("s\(index)", isDirectory: true)
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let file = folder.appendingPathComponent("out.pdf")
            try Data("file-\(index)".utf8).write(to: file)
            return file
        }
    }

    private func assertEveryFileKept(_ renamer: ExclusiveRename, reservations: NameReservations? = nil) throws {
        let staging = try temporaryDirectory()
        let destination = try temporaryDirectory()
        let count = 64
        let sources = try stage(count, in: staging)
        let results = Mutex<[Result<URL, Error>]>([])
        DispatchQueue.concurrentPerform(iterations: count) { index in
            let outcome = Result {
                try OutputPlacer.place(
                    [sources[index]], into: destination, names: nil, reservations: reservations,
                    fileManager: .default, renamer: renamer
                )[0]
            }
            results.withLock { $0.append(outcome) }
        }
        let placed = try results.withLock { $0 }.map { try $0.get() }
        XCTAssertEqual(Set(placed.map(\.path)).count, count, "every file got its own name")
        let contents = try placed.map { try String(contentsOf: $0, encoding: .utf8) }
        XCTAssertEqual(Set(contents), Set((0..<count).map { "file-\($0)" }), "no file was replaced")
        XCTAssertEqual(
            try FileManager.default.contentsOfDirectory(atPath: destination.path).count, count,
            "no placeholder or scratch file is left"
        )
        for source in sources { XCTAssertFalse(FileManager.default.fileExists(atPath: source.path)) }
    }

    func testConcurrentPlacementsKeepEveryFileWithoutExclusiveRename() throws {
        try assertEveryFileKept(withoutExclusiveRename)
    }

    func testConcurrentPlacementsKeepEveryFileAcrossVolumesWithoutExclusiveRename() throws {
        try assertEveryFileKept(crossVolumeWithoutExclusiveRename)
    }

    func testConcurrentPlacementsWithReservationsKeepEveryFileWithoutExclusiveRename() throws {
        try assertEveryFileKept(withoutExclusiveRename, reservations: NameReservations())
    }

    func testFallbackRefusesAnExistingTargetAndKeepsBothFiles() throws {
        let staging = try temporaryDirectory()
        let destination = try temporaryDirectory()
        let source = staging.appendingPathComponent("out.pdf")
        try Data("new".utf8).write(to: source)
        let target = destination.appendingPathComponent("out.pdf")
        try Data("original".utf8).write(to: target)

        for renamer in [withoutExclusiveRename, crossVolumeWithoutExclusiveRename] {
            XCTAssertThrowsError(try OutputPlacer.moveExclusively(source, to: target, renamer: renamer)) { error in
                XCTAssertEqual((error as? CocoaError)?.code, .fileWriteFileExists)
            }
            XCTAssertEqual(try String(contentsOf: target, encoding: .utf8), "original")
            XCTAssertEqual(try String(contentsOf: source, encoding: .utf8), "new", "the source stays")
            XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: destination.path), ["out.pdf"])
        }
    }

    func testFallbackRefusesADanglingSymbolicLink() throws {
        let staging = try temporaryDirectory()
        let destination = try temporaryDirectory()
        let source = staging.appendingPathComponent("out.pdf")
        try Data("new".utf8).write(to: source)
        let link = destination.appendingPathComponent("out.pdf")
        try FileManager.default.createSymbolicLink(atPath: link.path, withDestinationPath: "/nonexistent/ashift-target")

        XCTAssertThrowsError(try OutputPlacer.moveExclusively(source, to: link, renamer: withoutExclusiveRename)) { error in
            XCTAssertEqual((error as? CocoaError)?.code, .fileWriteFileExists)
        }
        XCTAssertEqual(try FileManager.default.destinationOfSymbolicLink(atPath: link.path), "/nonexistent/ashift-target")
        XCTAssertFalse(FileManager.default.fileExists(atPath: "/nonexistent/ashift-target"), "nothing was created through the link")
    }

    func testFallbackRemovesItsPlaceholderWhenTheRenameFails() throws {
        let destination = try temporaryDirectory()
        let missing = destination.appendingPathComponent("not-there.pdf")
        let target = destination.appendingPathComponent("out.pdf")

        XCTAssertThrowsError(try OutputPlacer.moveExclusively(missing, to: target, renamer: withoutExclusiveRename))
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: destination.path), [], "no empty file is left behind")
    }

    func testFailedCopyLeavesNoScratchFile() throws {
        let destination = try temporaryDirectory()
        let missing = destination.appendingPathComponent("not-there.pdf")
        XCTAssertThrowsError(try OutputPlacer.copyExclusively(missing, to: destination.appendingPathComponent("out.pdf")))
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: destination.path), [])
    }

    func testAVolumeWithoutAnyExclusivePrimitiveGetsATypedError() {
        XCTAssertEqual(OutputPlacer.placementError(forCreateErrno: ENOTSUP) as? PlacementError, .exclusivePlacementUnsupported)
        XCTAssertEqual(OutputPlacer.placementError(forCreateErrno: EOPNOTSUPP) as? PlacementError, .exclusivePlacementUnsupported)
        XCTAssertEqual((OutputPlacer.placementError(forCreateErrno: EEXIST) as? CocoaError)?.code, .fileWriteFileExists)
        XCTAssertEqual((OutputPlacer.placementError(forCreateErrno: EACCES) as? CocoaError)?.code, .fileWriteUnknown)
    }

    // MARK: - Only the staging folder is moved

    func testStagedFilesAcceptsFilesInsideStaging() throws {
        let staging = try temporaryDirectory()
        let nested = staging.appendingPathComponent("sub", isDirectory: true)
        try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)
        let a = staging.appendingPathComponent("a.pdf")
        let b = nested.appendingPathComponent("b.pdf")
        try Data("a".utf8).write(to: a)
        try Data("b".utf8).write(to: b)
        XCTAssertEqual(try OutputPlacer.stagedFiles([a.path, b.path], in: staging).map(\.lastPathComponent), ["a.pdf", "b.pdf"])
        // The engine may report the resolved form of a temporary folder (/private/var/...).
        let resolved = a.resolvingSymlinksInPath()
        XCTAssertEqual(try OutputPlacer.stagedFiles([resolved.path], in: staging).count, 1)
    }

    func testStagedFilesRejectsAnInputOutsideStaging() throws {
        let staging = try temporaryDirectory()
        let elsewhere = try temporaryDirectory()
        let input = elsewhere.appendingPathComponent("input.pdf")
        try Data("input".utf8).write(to: input)
        let inside = staging.appendingPathComponent("ok.pdf")
        try Data("ok".utf8).write(to: inside)

        XCTAssertThrowsError(try OutputPlacer.stagedFiles([inside.path, input.path], in: staging)) { error in
            guard case .internal? = error as? EngineError else { return XCTFail("unexpected error \(error)") }
        }
        // A sibling folder with the same name prefix is not inside staging either.
        let sibling = URL(fileURLWithPath: staging.path + "-other")
        try FileManager.default.createDirectory(at: sibling, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: sibling) }
        let sneaky = sibling.appendingPathComponent("x.pdf")
        try Data("x".utf8).write(to: sneaky)
        XCTAssertThrowsError(try OutputPlacer.stagedFiles([sneaky.path], in: staging))
        XCTAssertEqual(try Data(contentsOf: input), Data("input".utf8), "the input stays where it was")
    }

    func testStagedFilesRejectsTraversalAndSymbolicLinksOutOfStaging() throws {
        let staging = try temporaryDirectory()
        let elsewhere = try temporaryDirectory()
        let input = elsewhere.appendingPathComponent("input.pdf")
        try Data("input".utf8).write(to: input)

        let traversal = staging.path + "/../" + elsewhere.lastPathComponent + "/input.pdf"
        XCTAssertThrowsError(try OutputPlacer.stagedFiles([traversal], in: staging))

        let link = staging.appendingPathComponent("link.pdf")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: input)
        XCTAssertThrowsError(try OutputPlacer.stagedFiles([link.path], in: staging), "a link to an input is not a product")
    }
}
