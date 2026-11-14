import SwiftUI

/// The floating glass capsule at the bottom center of a selectable surface (the page grid): the selection count as a
/// tabular numeric roll, then icon actions. Place it with `dsSelectionActionBar(isPresented:)` so it rises in only
/// while something is selected.
public struct DSSelectionActionBar<Actions: View>: View {
    let count: Text
    let countValue: Int
    let actions: Actions

    /// `count` is the visible label ("3 selected"); `countValue` drives its numeric content transition.
    public init(count: Text, countValue: Int, @ViewBuilder actions: () -> Actions) {
        self.count = count
        self.countValue = countValue
        self.actions = actions()
    }

    public var body: some View {
        HStack(spacing: DSSpacing.s) {
            count
                .font(DSFont.headline)
                .monospacedDigit()
                .contentTransition(.numericText(value: Double(countValue)))
                .dsAnimation(.snappy, value: countValue)
                .padding(.horizontal, DSSpacing.s)
            Divider()
                .frame(height: DSControlHeight.small)
            actions
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .controlSize(.large)
        }
        .padding(.horizontal, DSSpacing.m)
        .frame(height: DSControlHeight.toast)
        .adaptiveGlass(.floating, in: Capsule())
        .dsElevation(.e3)
        .accessibilityElement(children: .contain)
    }
}

extension View {
    /// Pins `bar` to the bottom center, `DSSpacing.xl` above the edge, while `isPresented` is true. It settles in
    /// under `smooth`, or fades under Reduce Motion.
    public func dsSelectionActionBar(isPresented: Bool, @ViewBuilder bar: () -> some View) -> some View {
        modifier(SelectionActionBarModifier(isPresented: isPresented, bar: bar()))
    }
}

private struct SelectionActionBarModifier<Bar: View>: ViewModifier {
    let isPresented: Bool
    let bar: Bar
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content.overlay(alignment: .bottom) {
            if isPresented {
                bar
                    .padding(.bottom, DSSpacing.xl)
                    .transition(.dsSettle(reduceMotion: reduceMotion))
            }
        }
        .animation(DSMotion.smooth.animation(reduceMotion: reduceMotion), value: isPresented)
    }
}

#Preview("Selection action bar") {
    DSColor.canvas
        .frame(width: 560, height: 200)
        .dsSelectionActionBar(isPresented: true) {
            DSSelectionActionBar(count: Text(verbatim: "3 selected"), countValue: 3) {
                Button(String("Rotate"), systemImage: "rotate.right") {}
                Button(String("Extract"), systemImage: "doc.on.doc") {}
                Button(String("Delete"), systemImage: "trash") {}
            }
        }
}
