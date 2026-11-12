import AriadShiftCore
import SwiftUI

/// The generic parameter form of a tool, generated from its registry schema. Field titles and
/// help text come from the schema, so they are not part of the String Catalog.
struct ParamFormView: View {
    let form: ParamForm
    @Binding var values: [String: ParamValue]
    let error: (kind: String, field: String)?

    var body: some View {
        Form {
            ForEach(form.fields) { field in
                row(for: field)
            }
            if let error {
                Text(L10n.branch("mac.run.field_error", error.kind, error.field))
                    .foregroundStyle(.red)
                    .accessibilityAddTraits(.isStaticText)
            }
        }
        .formStyle(.grouped)
    }

    private func text(_ field: ParamField) -> Binding<String> {
        Binding(
            get: { if case let .text(value)? = values[field.name] { value } else { "" } },
            set: { values[field.name] = .text($0) }
        )
    }

    private func flag(_ field: ParamField) -> Binding<Bool> {
        Binding(
            get: { if case let .flag(value)? = values[field.name] { value } else { false } },
            set: { values[field.name] = .flag($0) }
        )
    }

    private func index(_ field: ParamField) -> Binding<Int?> {
        Binding(
            get: { if case let .index(value)? = values[field.name] { value } else { nil } },
            set: { values[field.name] = .index($0) }
        )
    }

    /// A required field is marked with an asterisk on screen only: VoiceOver would read the
    /// symbol aloud, so it hears the title and the "is required" hint instead.
    private func title(_ field: ParamField) -> String {
        field.isRequired ? field.title + " *" : field.title
    }

    private func requiredHint(_ field: ParamField) -> String {
        field.isRequired ? L10n.branch("mac.run.field_error", "missing", field.title) : ""
    }

    @ViewBuilder
    private func row(for field: ParamField) -> some View {
        switch field.kind {
        case .toggle:
            Toggle(field.title, isOn: flag(field)).help(field.help ?? "")
        case let .choice(options):
            Picker(title(field), selection: index(field)) {
                if !field.isRequired {
                    Text(verbatim: "–").tag(Int?.none)
                }
                ForEach(options.indices, id: \.self) { position in
                    Text(verbatim: options[position].displayText).tag(Int?.some(position))
                }
            }
            .help(field.help ?? "")
            .announced(as: field, requiredHint: requiredHint(field))
        case .secret:
            SecureField(title(field), text: text(field)).help(field.help ?? "")
                .announced(as: field, requiredHint: requiredHint(field))
        case let .presets(presets):
            HStack {
                TextField(title(field), text: text(field)).help(field.help ?? "")
                    .announced(as: field, requiredHint: requiredHint(field))
                Menu(String(localized: "mac.run.presets")) {
                    ForEach(presets, id: \.self) { preset in
                        Button(preset) { values[field.name] = .text(preset) }
                    }
                }
                .fixedSize()
            }
        case .text, .integer, .number, .list:
            TextField(title(field), text: text(field)).help(field.help ?? "")
                .announced(as: field, requiredHint: requiredHint(field))
        }
    }
}

private extension View {
    /// What VoiceOver says: the plain title (no asterisk) and, for a required field, that it is required.
    func announced(as field: ParamField, requiredHint: String) -> some View {
        accessibilityLabel(field.title).accessibilityHint(requiredHint)
    }
}
