import AriadDesignSystem
import AriadShiftCore
import Foundation

/// What the welcome window plays and shows when it appears.
///
/// The full opening plays on the first launch and after an update, the short one on every other appearance, and a
/// crossfade replaces both under Reduce Motion. The onboarding sheet follows the opening until onboarding is done
/// (or when Help > Show Welcome asks for it); What's New follows the first opening after an update.
struct OpeningPlan: Equatable {
    enum Motion: Equatable {
        case play(DSOpeningTimeline.Variant)
        /// Reduce Motion: the final state crossfades in.
        case fade
    }

    enum Sheet: String, Identifiable, Equatable {
        case onboarding
        case whatsNew

        var id: String { rawValue }
    }

    let motion: Motion
    /// The sheet shown over the welcome window once the opening ends, if any.
    let sheet: Sheet?

    /// - Parameters:
    ///   - lastSeenVersion: the app version the welcome window last appeared in; nil before this version recorded one.
    ///   - firstAppearance: the welcome window appears for the first time since the app launched.
    ///   - onboardingRequested: Help > Show Welcome asked for the onboarding sheet.
    static func resolve(
        lastSeenVersion: String?,
        currentVersion: String,
        needsOnboarding: Bool,
        firstAppearance: Bool,
        onboardingRequested: Bool,
        reduceMotion: Bool
    ) -> OpeningPlan {
        // A version that never recorded one (an older release) counts as an update too.
        let updated = !needsOnboarding && lastSeenVersion != currentVersion
        let full = firstAppearance && (needsOnboarding || updated)
        let motion: Motion = reduceMotion ? .fade : .play(full ? .full : .short)
        let sheet: Sheet? = if needsOnboarding || onboardingRequested {
            .onboarding
        } else if firstAppearance && updated {
            .whatsNew
        } else {
            nil
        }
        return OpeningPlan(motion: motion, sheet: sheet)
    }
}

/// The version the welcome window last appeared in, kept in the app's defaults next to the onboarding flag.
struct OpeningRecord {
    static let lastSeenVersionKey = "opening.lastSeenVersion"

    let defaults: UserDefaults

    init(defaults: UserDefaults = AppSettings.launchDefaults) {
        self.defaults = defaults
    }

    var lastSeenVersion: String? { defaults.string(forKey: Self.lastSeenVersionKey) }

    func record(_ version: String) {
        defaults.set(version, forKey: Self.lastSeenVersionKey)
    }

    /// The running app's version (`CFBundleShortVersionString`).
    static var currentVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
    }
}
