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
    /// Names a `UserDefaults` suite to use instead of the standard defaults: a suite name, or the absolute path of a
    /// preferences file. Only a Debug build of the app reads it (see `AriadShiftApp.init`): UI tests set it so every
    /// run starts from a fresh preferences file and never touches the user's own settings.
    public static let defaultsSuiteVariable = "ASHIFT_DEFAULTS_SUITE"

    /// The suite named by `defaultsSuiteVariable` in `environment`, or nil when it is absent or empty.
    public static func suiteDefaults(in environment: [String: String]) -> UserDefaults? {
        guard let suite = environment[defaultsSuiteVariable], !suite.isEmpty else { return nil }
        return UserDefaults(suiteName: suite)
    }

    /// The defaults the app reads and writes: the standard ones, unless the app replaces them once at startup,
    /// before anything reads them. The replacement is made in the app target under `#if DEBUG` because a Swift
    /// package built by Xcode does not see the app's `DEBUG` condition, so a gate here would never open.
    /// The test process seeds the suite before the app starts, which works across processes only because the
    /// app is not sandboxed (`ENABLE_APP_SANDBOX: NO`): a sandboxed app could not read it.
    public nonisolated(unsafe) static var launchDefaults: UserDefaults = .standard

    // UserDefaults is documented thread-safe but not declared Sendable in the SDK; removing this
    // fails with "stored property 'defaults' ... has non-Sendable type" (checked on the macOS 27.2 SDK).
    private nonisolated(unsafe) let defaults: UserDefaults

    public init(defaults: UserDefaults = AppSettings.launchDefaults) {
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
