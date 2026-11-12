import Foundation
import XCTest

/// Repository root, derived from this file's location: apps/macos/AriadShiftTests/TestSupport.swift.
let repositoryRoot: URL = {
    var url = URL(fileURLWithPath: #filePath)
    for _ in 0..<4 { url.deleteLastPathComponent() }
    return url
}()

/// A committed fixture. A missing fixture fails the test instead of skipping it.
func fixtureURL(_ relative: String, file: StaticString = #filePath, line: UInt = #line) throws -> URL {
    let url = repositoryRoot.appendingPathComponent("fixtures").appendingPathComponent(relative)
    guard FileManager.default.fileExists(atPath: url.path) else {
        XCTFail("missing fixture \(relative) at \(url.path)", file: file, line: line)
        throw CocoaError(.fileNoSuchFile)
    }
    return url
}

/// The compiled localisation of the app for one language, as the system would load it.
func localizedBundle(_ language: String, file: StaticString = #filePath, line: UInt = #line) throws -> Bundle {
    guard let path = Bundle.main.path(forResource: language, ofType: "lproj"), let bundle = Bundle(path: path) else {
        XCTFail("the app bundle has no \(language).lproj", file: file, line: line)
        throw CocoaError(.fileNoSuchFile)
    }
    return bundle
}
