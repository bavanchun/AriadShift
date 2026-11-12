import AriadShiftCore
import SwiftUI

/// Placeholder window that proves the Swift app reaches the Rust engine through
/// `EngineClient`. The document shell replaces it.
struct FoundationStatusView: View {
    private enum Status {
        case loading
        case ready(toolCount: Int)
        case failed(String)
    }

    @State private var status = Status.loading

    private var appVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
    }

    var body: some View {
        VStack(spacing: 8) {
            Text("AriadShift")
                .font(.largeTitle.bold())
            Text("Version \(appVersion)")
                .foregroundStyle(.secondary)
            switch status {
            case .loading:
                ProgressView()
            case let .ready(toolCount):
                Text("Engine ready: \(toolCount) tools")
            case let .failed(message):
                Text(message)
                    .foregroundStyle(.red)
            }
        }
        .padding(40)
        .frame(minWidth: 360)
        .task {
            do {
                let client = try EngineClient()
                status = .ready(toolCount: try await client.listTools().count)
            } catch {
                status = .failed(error.localizedDescription)
            }
        }
    }
}
