import AriadShiftCore
import AppKit
import Observation
import UniformTypeIdentifiers

/// File > Import: converts other formats to PDF through the engine and opens the result.
@MainActor
@Observable
final class ImportModel {
    @MainActor
    @Observable
    final class Job: Identifiable {
        enum State {
            case running
            case failed(String)
        }

        let id = UUID()
        let name: String
        fileprivate(set) var state = State.running
        fileprivate var task: Task<Void, Never>?

        init(name: String) {
            self.name = name
        }

        func cancel() { task?.cancel() }
    }

    private(set) var jobs: [Job] = []
    /// Shared by every job so that two files with the same name stem get different outputs.
    @ObservationIgnored private let reservations = NameReservations()

    /// Asks for files, then imports each. Returns whether any job was started.
    func chooseAndImport(client: EngineClient?) -> Bool {
        guard let client else { return false }
        let panel = NSOpenPanel()
        panel.message = String(localized: "mac.import.message")
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = ImportSource.extensions.keys.compactMap { UTType(filenameExtension: $0) }
        guard panel.runModal() == .OK, !panel.urls.isEmpty else { return false }
        for url in panel.urls {
            begin(url, client: client)
        }
        return true
    }

    func begin(_ url: URL, client: EngineClient) {
        let job = Job(name: url.lastPathComponent)
        jobs.append(job)
        let destination = OutputPlacer.destinationDirectory(for: url, customFolder: AppSettings().outputFolder)
        job.task = Task {
            do {
                let output = try await DocumentImporter.importDocument(
                    at: url, destination: destination, client: client, reservations: reservations
                )
                jobs.removeAll { $0.id == job.id }
                await ToolRunModel.open(output)
            } catch EngineError.cancelled {
                jobs.removeAll { $0.id == job.id }
            } catch {
                job.state = .failed(error.localizedDescription)
            }
        }
    }

    func dismiss(_ job: Job) {
        job.cancel()
        jobs.removeAll { $0.id == job.id }
    }
}
