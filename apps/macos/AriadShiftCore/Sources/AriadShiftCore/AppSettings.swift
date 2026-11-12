import Foundation

/// UserDefaults keys shared by `AppSettings` and the SwiftUI `@AppStorage` bindings.
public enum SettingsKey {
    /// Folder for tool and import results; empty means next to the input.
    public static let outputFolder = "settings.outputFolder"
    public static let openResultAfterRun = "settings.openResultAfterRun"
    public static let onboardingCompleted = "onboarding.completed"
}

/// The settings that change engine behaviour. Values persist in `UserDefaults`.
public struct AppSettings: Sendable {
    // UserDefaults is documented thread-safe but not declared Sendable in the SDK; removing this
    // fails with "stored property 'defaults' ... has non-Sendable type" (checked on the macOS 27.2 SDK).
    private nonisolated(unsafe) let defaults: UserDefaults

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    /// Folder chosen for results, or nil for "next to the input".
    public var outputFolder: URL? {
        get {
            guard let path = defaults.string(forKey: SettingsKey.outputFolder), !path.isEmpty else { return nil }
            return URL(fileURLWithPath: path, isDirectory: true)
        }
        nonmutating set { defaults.set(newValue?.path ?? "", forKey: SettingsKey.outputFolder) }
    }

    /// Open a single PDF result in a new window once a tool finishes. On by default.
    public var openResultAfterRun: Bool {
        get { defaults.object(forKey: SettingsKey.openResultAfterRun) as? Bool ?? true }
        nonmutating set { defaults.set(newValue, forKey: SettingsKey.openResultAfterRun) }
    }

    /// The first-launch welcome is shown until it has been completed once.
    public var needsOnboarding: Bool {
        !defaults.bool(forKey: SettingsKey.onboardingCompleted)
    }

    public func completeOnboarding() {
        defaults.set(true, forKey: SettingsKey.onboardingCompleted)
    }
}
