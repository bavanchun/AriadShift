import Foundation

/// Access to the generated String Catalog (`Localizable.xcstrings`, built from `locales/*.json`
/// by `scripts/i18n-xcstrings.mjs`). Keys are the dotted JSON paths. Plain and plural messages
/// are read with `String(localized:)` at the call site; this type covers `select` messages,
/// which the catalogue stores as one key per branch.
enum L10n {
    /// One branch of a select message, for example `branch("mac.inspector.field", "size")`.
    /// An unknown branch falls back to the message's `other` branch.
    static func branch(_ key: String, _ name: String, _ arguments: CVarArg..., bundle: Bundle = .main) -> String {
        let specific = "\(key).\(name)"
        var template = bundle.localizedString(forKey: specific, value: specific, table: nil)
        if template == specific {
            template = bundle.localizedString(forKey: "\(key).other", value: nil, table: nil)
        }
        return arguments.isEmpty ? template : String(format: template, arguments: arguments)
    }
}
