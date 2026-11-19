import AriadShiftCore
import XCTest

/// Runs inside the built app: the bundle must carry the CLI helper and Pandoc, and the
/// default `EngineClient` must reach the Rust engine with them.
final class BundleHelperTests: XCTestCase {
    func testAppBundleEmbedsExecutableHelper() {
        let helper = EngineClient.bundledHelperURL()
        XCTAssertTrue(
            FileManager.default.isExecutableFile(atPath: helper.path),
            "missing helper at \(helper.path)"
        )
    }

    func testDefaultClientConfiguresBundledHelper() throws {
        let client = try EngineClient()
        XCTAssertEqual(client.helperURL, EngineClient.bundledHelperURL())
        XCTAssertTrue(client.helperURL.path.hasSuffix(".app/Contents/Helpers/ashift"), client.helperURL.path)
    }

    func testDefaultClientListsTools() async throws {
        let client = try EngineClient()
        let tools = try await client.listTools()
        XCTAssertTrue(tools.contains { $0.id == "pdf.reorder" })
    }

    func testHelperRunsAndReportsVersion() throws {
        let process = Process()
        process.executableURL = EngineClient.bundledHelperURL()
        process.arguments = ["--version"]
        let output = Pipe()
        process.standardOutput = output
        try process.run()
        process.waitUntilExit()
        let text = String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        XCTAssertEqual(process.terminationStatus, 0)
        XCTAssertTrue(text.contains("ashift"), text)
    }

    // MARK: - Bundled Pandoc

    private var bundledPandoc: URL { EngineClient.bundledPandocURL(beside: EngineClient.bundledHelperURL()) }

    func testAppBundleEmbedsExecutablePandoc() {
        var isDirectory: ObjCBool = false
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: bundledPandoc.path, isDirectory: &isDirectory)
                && !isDirectory.boolValue,
            "missing Pandoc at \(bundledPandoc.path)"
        )
        XCTAssertTrue(FileManager.default.isExecutableFile(atPath: bundledPandoc.path), bundledPandoc.path)
        XCTAssertTrue(bundledPandoc.path.hasSuffix(".app/Contents/Helpers/pandoc"), bundledPandoc.path)
    }

    func testBundledPandocRunsAndIsTheSupportedVersion() throws {
        let process = Process()
        process.executableURL = bundledPandoc
        process.arguments = ["--version"]
        // Nothing from the developer's environment: the binary must start by itself.
        process.environment = ["PATH": "/usr/bin:/bin"]
        let output = Pipe()
        process.standardOutput = output
        try process.run()
        process.waitUntilExit()
        let text = String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        XCTAssertEqual(process.terminationStatus, 0, text)
        XCTAssertTrue(text.hasPrefix("pandoc 3.12"), text)
    }

    func testAppBundleShipsPandocLicenceNotices() throws {
        let directory = try XCTUnwrap(Bundle.main.resourceURL).appendingPathComponent("ThirdParty/pandoc")
        for name in ["COPYING.txt", "COPYRIGHT", "ACKNOWLEDGEMENTS.txt"] {
            let url = directory.appendingPathComponent(name)
            let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int) ?? 0
            XCTAssertGreaterThan(size, 0, "missing or empty \(url.path)")
        }
        let acknowledgements = try String(
            contentsOf: directory.appendingPathComponent("ACKNOWLEDGEMENTS.txt"), encoding: .utf8)
        XCTAssertTrue(acknowledgements.contains("3.12"))
        XCTAssertTrue(acknowledgements.contains("https://github.com/jgm/pandoc/tree/3.12"))
    }

    /// The product rule: Export as Word needs no installed Pandoc. The process gets a `PATH` that holds none and no
    /// `ASHIFT_PANDOC`, so the only Pandoc the engine can reach is the one `EngineClient` exports from the bundle.
    /// Markdown to Word, because it needs no other engine; the app's own PDF to Word route ends in the same Pandoc.
    func testExportsToWordWithNoPandocOnPath() async throws {
        let strippedPath = "/usr/bin:/bin"
        for directory in strippedPath.split(separator: ":") {
            XCTAssertFalse(
                FileManager.default.fileExists(atPath: "\(directory)/pandoc"),
                "a Pandoc in \(directory) would make this test prove nothing"
            )
        }
        let variables = ["PATH", EngineClient.pandocEnvironmentName]
        let originals = variables.map { name in getenv(name).map { String(cString: $0) } }
        addTeardownBlock {
            for (name, original) in zip(variables, originals) {
                if let original { setenv(name, original, 1) } else { unsetenv(name) }
            }
        }
        setenv("PATH", strippedPath, 1)
        unsetenv(EngineClient.pandocEnvironmentName)

        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("ashift-bundle-pandoc-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        let input = directory.appendingPathComponent("note.md")
        try "# Bundled Pandoc\n\nExported without a Pandoc on the PATH.\n".write(
            to: input, atomically: true, encoding: .utf8)
        let output = directory.appendingPathComponent("note.docx")

        // What AriadShiftApp.init does at launch; this test host builds the client itself and the PATH was changed above.
        EngineClient.exportBundledPandoc()
        let client = try EngineClient()
        XCTAssertEqual(
            getenv(EngineClient.pandocEnvironmentName).map { String(cString: $0) },
            bundledPandoc.path,
            "the launch export must name the bundled Pandoc"
        )
        let outcome = try await client.convert(
            ConvertRequest(input: input.path, output: output.path, targetFormat: "docx", overwrite: true)
        )

        XCTAssertEqual(outcome.outputPath, output.path)
        let docx = try Data(contentsOf: output)
        XCTAssertEqual(docx.prefix(4), Data([0x50, 0x4B, 0x03, 0x04]), "a .docx is a zip archive")
        XCTAssertNotNil(docx.range(of: Data("word/document.xml".utf8)), "the archive holds the Word document")
    }
}
