import AriadShiftCore
import Foundation

/// The tools the user ran most recently, newest first, shared by every window and kept across launches.
struct ToolRecents: Sendable {
    static let key = "tools.recent"
    /// The palette's Recent section shows this many tools.
    static let limit = 3

    // UserDefaults is documented thread-safe but not declared Sendable in the SDK.
    private nonisolated(unsafe) let defaults: UserDefaults

    /// Uses the app's launch defaults, which UI tests replace with a fresh suite.
    init(defaults: UserDefaults = AppSettings.launchDefaults) {
        self.defaults = defaults
    }

    var ids: [String] {
        Array((defaults.stringArray(forKey: Self.key) ?? []).prefix(Self.limit))
    }

    /// Moves `id` to the front, keeping at most `limit` tools.
    func record(_ id: String) {
        let updated = [id] + ids.filter { $0 != id }
        defaults.set(Array(updated.prefix(Self.limit)), forKey: Self.key)
    }
}
