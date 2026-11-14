import SwiftUI

/// The toolbar page capsule: previous and next chevrons around a 64pt "3 / 20" field with tabular digits.
/// Typing a number and pressing ↩ jumps to that page; values outside 1...pageCount are clamped.
/// The capsule is 28 tall with radius 14 and sits on the toolbar's own glass, so it adds only a quiet fill.
public struct DSToolbarPageField: View {
    @Binding var page: Int
    let pageCount: Int
    let previousLabel: Text
    let nextLabel: Text
    let fieldLabel: Text

    @State private var draft: Int = 1
    @FocusState private var isEditing: Bool

    public init(page: Binding<Int>, pageCount: Int, previousLabel: Text, nextLabel: Text, fieldLabel: Text) {
        self._page = page
        self.pageCount = pageCount
        self.previousLabel = previousLabel
        self.nextLabel = nextLabel
        self.fieldLabel = fieldLabel
    }

    public var body: some View {
        HStack(spacing: 0) {
            chevron("chevron.left", label: previousLabel, enabled: page > 1) { go(to: page - 1) }
            HStack(spacing: DSSpacing.xs) {
                TextField(value: $draft, format: .number.grouping(.never)) { fieldLabel }
                    .textFieldStyle(.plain)
                    .multilineTextAlignment(.trailing)
                    .focused($isEditing)
                    .onSubmit {
                        go(to: draft)
                        isEditing = false
                    }
                    .accessibilityLabel(fieldLabel)
                Text(verbatim: "/ \(pageCount)")
                    .foregroundStyle(.secondary)
                    .fixedSize()
            }
            .font(DSFont.body)
            .monospacedDigit()
            .frame(width: DSSize.pageField)
            chevron("chevron.right", label: nextLabel, enabled: page < pageCount) { go(to: page + 1) }
        }
        .frame(height: DSControlHeight.regular)
        .background(Color.primary.opacity(0.05), in: Capsule())
        .onAppear { draft = page }
        .onChange(of: page) { _, newPage in draft = newPage }
        .onChange(of: isEditing) { _, editing in if !editing { draft = page } }
    }

    private func chevron(_ symbol: String, label: Text, enabled: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(DSFont.caption)
                .frame(width: DSControlHeight.regular, height: DSControlHeight.regular)
                .contentShape(Rectangle())
        }
        .buttonStyle(DSPressButtonStyle())
        .disabled(!enabled)
        .accessibilityLabel(label)
    }

    private func go(to target: Int) {
        guard pageCount > 0 else { return }
        page = min(max(target, 1), pageCount)
        draft = page
    }
}

#Preview("Page field") {
    @Previewable @State var page = 3
    DSToolbarPageField(
        page: $page,
        pageCount: 20,
        previousLabel: Text(verbatim: "Previous page"),
        nextLabel: Text(verbatim: "Next page"),
        fieldLabel: Text(verbatim: "Page")
    )
    .padding(DSSpacing.xl)
    .background(DSColor.window)
}
