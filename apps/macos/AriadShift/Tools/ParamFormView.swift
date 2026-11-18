import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The generic parameter form of a tool, generated from its registry schema, as an inspector section: each setting is
/// a row with its label in the trailing-aligned label column. Field titles and help text come from the schema, so
/// they are not part of the String Catalog.
struct ParamFormView: View {
    let form: ParamForm
    @Binding var values: [String: ParamValue]
    let error: (kind: String, field: String)?

    var body: some View {
        DSInspectorSection(Text("mac.run.parameters")) {
            if form.fields.isEmpty {
                Text("mac.run.no_parameters")
                    .font(DSFont.callout)
                    .foregroundStyle(.secondary)
            } else {
                ParamFieldsView(fields: form.fields, values: $values)
            }
            if let error {
                Text(L10n.branch("mac.run.field_error", error.kind, error.field))
                    .font(DSFont.callout)
                    .foregroundStyle(DSColor.danger)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isStaticText)
                    .accessibilityIdentifier("run.field_error")
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(String(localized: "mac.run.parameters"))
    }
}

/// The editors of one object level. Groups and list rows nest it, each over its own value dictionary.
struct ParamFieldsView: View {
    let fields: [ParamField]
    @Binding var values: [String: ParamValue]

    var body: some View {
        ForEach(fields) { field in
            row(for: field)
        }
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

    private func group(_ field: ParamField) -> Binding<[String: ParamValue]> {
        Binding(
            get: { if case let .group(value)? = values[field.name] { value } else { [:] } },
            set: { values[field.name] = .group($0) }
        )
    }

    private func rows(_ field: ParamField) -> Binding<[ParamRow]> {
        Binding(
            get: { if case let .rows(value)? = values[field.name] { value } else { [] } },
            set: { values[field.name] = .rows($0) }
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
    private func label(_ field: ParamField) -> Text {
        Text(verbatim: field.isRequired ? field.title + " *" : field.title)
    }

    private func requiredHint(_ field: ParamField) -> String {
        field.isRequired ? L10n.branch("mac.run.field_error", "missing", field.title) : ""
    }

    @ViewBuilder
    private func row(for field: ParamField) -> some View {
        switch field.kind {
        case .toggle:
            // The switch leads and its title follows, as in the mockup's inspector rows.
            DSInspectorRow(nil) {
                HStack(spacing: DSSpacing.s) {
                    Toggle(isOn: flag(field)) { Text(verbatim: field.title) }
                        .toggleStyle(.switch)
                        .controlSize(.small)
                        .labelsHidden()
                    Text(verbatim: field.title)
                        .font(DSFont.body)
                        .accessibilityHidden(true)
                }
                .help(field.help ?? "")
            }
        case let .choice(options):
            labelled(field) {
                Picker(selection: index(field)) {
                    // A selection without a tag is an invalid Picker configuration whose result SwiftUI leaves
                    // undefined, so `nil` always has one: an optional field shows a dash, a required field that has
                    // no value yet shows an empty title, and the entry is gone once a value is chosen.
                    if !field.isRequired {
                        Text(verbatim: "–").tag(Int?.none)
                    } else if index(field).wrappedValue == nil {
                        Text(verbatim: "").tag(Int?.none)
                    }
                    ForEach(options.indices, id: \.self) { position in
                        Text(verbatim: options[position].displayText).tag(Int?.some(position))
                    }
                } label: {
                    Text(verbatim: field.title)
                }
                .labelsHidden()
            }
        case .secret:
            labelled(field) {
                SecureField(text: text(field), prompt: Text(verbatim: "")) { Text(verbatim: field.title) }
                    .textFieldStyle(.roundedBorder)
            }
        case let .presets(presets):
            labelled(field) {
                HStack(spacing: DSSpacing.xs) {
                    TextField(text: text(field), prompt: Text(verbatim: "")) { Text(verbatim: field.title) }
                        .textFieldStyle(.roundedBorder)
                    Menu {
                        ForEach(presets, id: \.self) { preset in
                            Button(preset) { values[field.name] = .text(preset) }
                        }
                    } label: {
                        Image(systemName: "list.bullet")
                    }
                    .menuStyle(.borderlessButton)
                    .menuIndicator(.hidden)
                    .fixedSize()
                    .help(String(localized: "mac.run.presets"))
                    .accessibilityLabel(String(localized: "mac.run.presets"))
                }
            }
        case let .group(children):
            VStack(alignment: .leading, spacing: DSSpacing.m) {
                Text(verbatim: field.title)
                    .font(DSFont.headline)
                    .accessibilityAddTraits(.isHeader)
                ParamFieldsView(fields: children, values: group(field))
            }
            .help(field.help ?? "")
            .accessibilityElement(children: .contain)
        case let .rows(shape):
            ParamRowsEditor(field: field, shape: shape, rows: rows(field))
        case .text, .integer, .number, .list:
            labelled(field) {
                TextField(text: text(field), prompt: Text(verbatim: "")) { Text(verbatim: field.title) }
                    .textFieldStyle(.roundedBorder)
            }
        }
    }

    /// A row with the field title in the label column and the schema help as its tooltip.
    private func labelled(_ field: ParamField, @ViewBuilder control: () -> some View) -> some View {
        DSInspectorRow(label(field)) {
            control()
                .help(field.help ?? field.title)
                .accessibilityLabel(field.title)
                .accessibilityHint(requiredHint(field))
        }
    }
}
