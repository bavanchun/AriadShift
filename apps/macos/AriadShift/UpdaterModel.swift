import Foundation
import Combine
import Sparkle

/// What the build says about updates: the appcast URL and the EdDSA public key that signs
/// releases. Both are Info.plist values; the key is a build setting that stays empty in dev builds.
struct UpdaterConfiguration: Equatable {
    let feedURL: URL?
    let publicKey: String

    static func current(bundle: Bundle = .main) -> UpdaterConfiguration {
        let feed = (bundle.object(forInfoDictionaryKey: "SUFeedURL") as? String).flatMap(URL.init(string:))
        let key = (bundle.object(forInfoDictionaryKey: "SUPublicEDKey") as? String) ?? ""
        return UpdaterConfiguration(feedURL: feed, publicKey: key.trimmingCharacters(in: .whitespacesAndNewlines))
    }

    /// Updates run only when a feed and a signing key are both configured.
    var isEnabled: Bool { feedURL != nil && !publicKey.isEmpty }
}

/// Sparkle's standard updater, started only for builds that carry a public key. Automatic
/// checks stay off until the release owner turns them on (`SUEnableAutomaticChecks`) or the
/// user does in Settings.
///
/// An `ObservableObject`, because menu bar `Commands` do not observe `@Observable` properties.
@MainActor
final class UpdaterModel: ObservableObject {
    let configuration: UpdaterConfiguration
    @Published private(set) var canCheckForUpdates = false
    private let controller: SPUStandardUpdaterController?
    private var observation: NSKeyValueObservation?

    init(configuration: UpdaterConfiguration = .current()) {
        self.configuration = configuration
        guard configuration.isEnabled else {
            controller = nil
            return
        }
        let controller = SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: nil, userDriverDelegate: nil)
        self.controller = controller
        canCheckForUpdates = controller.updater.canCheckForUpdates
        observation = controller.updater.observe(\.canCheckForUpdates, options: [.new]) { [weak self] _, change in
            let value = change.newValue ?? false
            Task { @MainActor in self?.canCheckForUpdates = value }
        }
    }

    var automaticallyChecks: Bool {
        get { controller?.updater.automaticallyChecksForUpdates ?? false }
        set { controller?.updater.automaticallyChecksForUpdates = newValue }
    }

    func checkForUpdates() {
        controller?.checkForUpdates(nil)
    }
}
