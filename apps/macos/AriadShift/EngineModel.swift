import AriadShiftCore
import Combine
import Foundation

/// App-wide engine state: the client and the Tools menu catalogue. Every engine call runs
/// off the main actor; this model only publishes the results.
///
/// This is an `ObservableObject` rather than an `@Observable`: menu bar `Commands` redraw
/// from `objectWillChange` but do not observe `@Observable` properties.
@MainActor
final class EngineModel: ObservableObject {
    enum State: Equatable {
        case loading
        case ready
        case failed(String)
    }

    @Published private(set) var state = State.loading
    @Published private(set) var groups: [ToolGroup] = []
    @Published private(set) var client: EngineClient?
    /// The registry facts (arity, secret parameters) could not be read. The engine itself is
    /// ready, but every tool that needs those facts stays disabled.
    @Published private(set) var runtimeInfoFailed = false
    private var loadTask: Task<Void, Never>?
    private let helper: URL?

    /// - Parameter helper: the `ashift` helper; the app bundle's by default. Tests point it at a
    ///   real file, or at a missing one to see a broken bundle.
    init(helper: URL? = nil) {
        self.helper = helper
    }

    /// Starts loading once; later calls do nothing.
    func start() {
        guard loadTask == nil else { return }
        loadTask = Task { await load() }
    }

    /// Completes when loading ended, in `.ready` or `.failed`.
    func waitUntilLoaded() async {
        start()
        await loadTask?.value
    }

    private func load() async {
        do {
            // The initialiser checks the helper and calls into Rust, so keep it off the main actor.
            let helper = helper
            let client = try await Task.detached(priority: .userInitiated) { try EngineClient(helperURL: helper) }.value
            async let tools = client.listTools()
            // Arity and secret parameters decide how a tool is edited. Without them nothing is
            // guessed: the catalogue disables every tool that needs them.
            let runtimeInfo = try? await client.toolRuntimeInfo()
            runtimeInfoFailed = runtimeInfo == nil
            groups = ToolCatalog.build(
                tools: try await tools, runtimeInfo: runtimeInfo ?? [:], plan: client.displayPlan)
            self.client = client
            state = .ready
        } catch {
            state = .failed(error.localizedDescription)
        }
    }
}
