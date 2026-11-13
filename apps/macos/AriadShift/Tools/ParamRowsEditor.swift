import AriadShiftCore
import SwiftUI

/// A list of objects: each row is its own nested form, and rows can be added, removed and moved.
/// Rows that come in several kinds (a redaction target is exact text, a pattern, a page...) get a
/// kind picker; the kind's constant is written by `ParamForm`, so the user never types it.
struct ParamRowsEditor: View {
    let field: ParamField
    let shape: ParamRows
    @Binding var rows: [ParamRow]

    var body: some View {
        GroupBox(field.isRequired ? field.title + " *" : field.title) {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(rows.indices, id: \.self) { position in
                    rowView(position)
                }
                addControl
            }
            .accessibilityElement(children: .contain)
            .accessibilityLabel(field.title)
        }
        .help(field.help ?? "")
    }

    private func rowView(_ position: Int) -> some View {
        GroupBox {
            VStack(alignment: .leading) {
                HStack {
                    if shape.variants.count > 1 {
                        Picker(shape.discriminator.map(Self.title) ?? "", selection: variant(position)) {
                            ForEach(shape.variants.indices, id: \.self) { index in
                                Text(verbatim: shape.variants[index].title).tag(index)
                            }
                        }
                    }
                    Spacer()
                    Button(String(localized: "mac.run.move_up"), systemImage: "arrow.up") { rows.swapAt(position, position - 1) }
                        .disabled(position == 0)
                    Button(String(localized: "mac.run.move_down"), systemImage: "arrow.down") { rows.swapAt(position, position + 1) }
                        .disabled(position == rows.count - 1)
                    Button(String(localized: "mac.run.remove"), systemImage: "minus.circle") { rows.remove(at: position) }
                }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                ParamFieldsView(fields: shape.variants[rows[position].variant].fields, values: values(position))
            }
        }
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
        } else {
            Button(String(localized: "mac.run.add_row"), systemImage: "plus") { rows.append(shape.makeRow()) }
        }
    }

    private static func title(_ key: String) -> String { key.capitalized }

    private func variant(_ position: Int) -> Binding<Int> {
        Binding(
            get: { rows[position].variant },
            set: { rows[position] = shape.makeRow(variant: $0) }
        )
    }

    private func values(_ position: Int) -> Binding<[String: ParamValue]> {
        Binding(get: { rows[position].values }, set: { rows[position].values = $0 })
    }
}
