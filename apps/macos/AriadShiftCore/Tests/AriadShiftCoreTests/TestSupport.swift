import Foundation
import Synchronization
import XCTest

@testable import AriadShiftCore

/// Repository root, derived from this file's location:
/// apps/macos/AriadShiftCore/Tests/AriadShiftCoreTests/TestSupport.swift.
let repositoryRoot: URL = {
    var url = URL(fileURLWithPath: #filePath)
    for _ in 0..<6 { url.deleteLastPathComponent() }
    return url
}()

/// A test precondition that does not hold. Thrown after `XCTFail`, so a broken
/// setup fails the run instead of silently skipping tests.
struct TestSetupError: Error, CustomStringConvertible {
    let description: String
}

/// Absolute path of a committed fixture. A missing fixture is a failure, never a skip.
func fixturePath(_ relative: String) throws -> String {
    let url = repositoryRoot.appendingPathComponent("fixtures").appendingPathComponent(relative)
    guard FileManager.default.fileExists(atPath: url.path) else {
        let message = "missing fixture \(relative) at \(url.path)"
        XCTFail(message)
        throw TestSetupError(description: message)
    }
    return url.path
}

/// The real `ashift` helper: `ASHIFT_HELPER_PATH`, else `target/release/ashift`,
/// else `target/debug/ashift`. Fails loudly when none is an executable file.
func helperURL() throws -> URL {
    var candidates: [String] = []
    if let path = ProcessInfo.processInfo.environment["ASHIFT_HELPER_PATH"], !path.isEmpty {
        candidates.append(path)
    } else {
        candidates.append(repositoryRoot.appendingPathComponent("target/release/ashift").path)
        candidates.append(repositoryRoot.appendingPathComponent("target/debug/ashift").path)
    }
    if let found = candidates.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) {
        return URL(fileURLWithPath: found)
    }
    let message =
        "no ashift helper found. Set ASHIFT_HELPER_PATH or build target/release/ashift (tried \(candidates))"
    XCTFail(message)
    throw TestSetupError(description: message)
}

func makeClient() throws -> EngineClient {
    try EngineClient(helperURL: helperURL())
}

extension XCTestCase {
    /// A fresh scratch directory, removed when the test ends.
    func temporaryDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("ashift-core-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }
}

/// Lets a `@Sendable` progress callback cancel the task that is running the
/// operation, regardless of whether the callback or the task assignment runs first.
final class TaskCanceller<Success: Sendable>: Sendable {
    private struct State {
        var task: Task<Success, Error>?
        var cancelRequested = false
    }

    private let state = Mutex(State())

    func attach(_ task: Task<Success, Error>) {
        let cancelNow = state.withLock { state -> Bool in
            state.task = task
            return state.cancelRequested
        }
        if cancelNow { task.cancel() }
    }

    func cancel() {
        let task = state.withLock { state -> Task<Success, Error>? in
            state.cancelRequested = true
            return state.task
        }
        task?.cancel()
    }
}

/// Collects values from a `@Sendable` callback.
final class Collector<Element: Sendable>: Sendable {
    private let storage = Mutex<[Element]>([])

    func append(_ element: Element) { storage.withLock { $0.append(element) } }
    var values: [Element] { storage.withLock { $0 } }
}
