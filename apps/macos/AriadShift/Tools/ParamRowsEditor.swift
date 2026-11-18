import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// A list of objects: each row is its own nested form on a surface card, and rows can be added, removed and moved.
/// Rows that come in several kinds (a redaction target is exact text, a pattern, a page...) get a kind picker; the
/// kind's constant is written by `ParamForm`, so the user never types it.
struct ParamRowsEditor: View {
    let field: ParamField
    let shape: ParamRows
    @Binding var rows: [ParamRow]

    var body: some View {
        VStack(alignment: .leading, spacing: DSSpacing.s) {
            Text(verbatim: field.isRequired ? field.title + " *" : field.title)
                .font(DSFont.headline)
                .accessibilityAddTraits(.isHeader)
            ForEach(rows) { row in
                rowView(row.id)
            }
            addControl
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(field.title)
        .help(field.help ?? "")
    }

    private var card: some Shape {
        RoundedRectangle(cornerRadius: DSRadius.control, style: .continuous)
    }

    private func rowView(_ id: ParamRow.ID) -> some View {
        VStack(alignment: .leading, spacing: DSSpacing.m) {
            HStack(spacing: DSSpacing.xs) {
                if shape.variants.count > 1 {
                    Picker(selection: variant(id)) {
                        ForEach(shape.variants.indices, id: \.self) { index in
                            Text(verbatim: shape.variants[index].title).tag(index)
                        }
                    } label: {
                        Text(verbatim: shape.discriminator.map(Self.title) ?? "")
                    }
                    .labelsHidden()
                    .fixedSize()
                }
                Spacer(minLength: 0)
                Button(String(localized: "mac.run.move_up"), systemImage: "arrow.up") { move(id, by: -1) }
                    .disabled(rows.first?.id == id)
                Button(String(localized: "mac.run.move_down"), systemImage: "arrow.down") { move(id, by: 1) }
                    .disabled(rows.last?.id == id)
                Button(String(localized: "mac.run.remove"), systemImage: "minus.circle") { rows.removeAll { $0.id == id } }
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            if let row = rows.row(id: id) {
                ParamFieldsView(fields: shape.variants[row.variant].fields, values: values(id))
            }
        }
        .padding(DSSpacing.s)
        .background(DSColor.surface, in: card)
        .overlay(card.stroke(DSColor.separator, lineWidth: DSSize.hairline))
    }

    private func move(_ id: ParamRow.ID, by offset: Int) {
        guard let index = rows.firstIndex(where: { $0.id == id }), rows.indices.contains(index + offset) else { return }
        rows.swapAt(index, index + offset)
    }

    @ViewBuilder
    private var addControl: some View {
        if shape.variants.count > 1 {
            Menu(String(localized: "mac.run.add_row"), systemImage: "plus") {
                ForEach(shape.variants.indices, id: \.self) { index in
                    Button(shape.variants[index].title) { rows.append(shape.makeRow(variant: index)) }
                }
            }
            .fixedSize()
            .buttonStyle(.dsSecondary)
        } else {
            Button(String(localized: "mac.run.add_row"), systemImage: "plus") { rows.append(shape.makeRow()) }
                .buttonStyle(.dsSecondary)
        }
    }

    private static func title(_ key: String) -> String { key.capitalized }

    private func variant(_ id: ParamRow.ID) -> Binding<Int> {
        Binding(
            get: { rows.row(id: id)?.variant ?? 0 },
            set: { kind in
                rows.update(id: id) { $0 = ParamRow(id: id, variant: kind, values: shape.makeRow(variant: kind).values) }
            }
        )
    }

    private func values(_ id: ParamRow.ID) -> Binding<[String: ParamValue]> {
        Binding(
            get: { rows.row(id: id)?.values ?? [:] },
            set: { newValues in rows.update(id: id) { $0.values = newValues } }
        )
    }
}
