import SwiftUI

extension View {
    /// Styles a rendered page in a thumbnail list or the page grid: radius `chip` and `e1`. A selected page gets a
    /// 2.5pt `accent` ring outside it, concentric with the page corners, over an `accentSoft` halo that drops to 30%
    /// in dark appearance. The ring sits outside the page, so selecting never shifts the layout.
    public func dsPageThumbnail(isSelected: Bool) -> some View {
        modifier(PageThumbnailModifier(isSelected: isSelected))
    }
}

private struct PageThumbnailModifier: ViewModifier {
    let isSelected: Bool
    @Environment(\.colorScheme) private var colorScheme

    /// The ring's inner edge is the page edge plus the ring width, so its radius grows by the same amount.
    private var ringShape: RoundedRectangle {
        RoundedRectangle(cornerRadius: DSRadius.chip + DSSize.selectionRing, style: .continuous)
    }

    func body(content: Content) -> some View {
        content
            .clipShape(RoundedRectangle(cornerRadius: DSRadius.chip, style: .continuous))
            .dsElevation(.e1)
            .background {
                if isSelected {
                    ringShape
                        .fill(DSColor.accentSoft.opacity(colorScheme == .dark ? DSOpacity.selectionHaloDark : 1))
                        .padding(-DSSize.selectionRing * 2)
                }
            }
            .overlay {
                if isSelected {
                    ringShape
                        .strokeBorder(DSColor.accent, lineWidth: DSSize.selectionRing)
                        .padding(-DSSize.selectionRing)
                }
            }
            .dsAnimation(.snappy, value: isSelected)
    }
}

/// A page number under a thumbnail: caption, tabular digits, primary when its page is selected.
public struct DSPageCaption: View {
    let number: Int
    let isSelected: Bool

    public init(_ number: Int, isSelected: Bool) {
        self.number = number
        self.isSelected = isSelected
    }

    public var body: some View {
        Text(number, format: .number)
            .font(DSFont.caption)
            .monospacedDigit()
            .foregroundStyle(isSelected ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary))
    }
}

/// The 3pt `thread` bar with round caps that marks where dragged pages will land.
public struct DSInsertionBar: View {
    public enum Axis: Sendable { case vertical, horizontal }

    let axis: Axis

    public init(_ axis: Axis = .vertical) {
        self.axis = axis
    }

    public var body: some View {
        Capsule()
            .fill(DSColor.thread)
            .frame(
                width: axis == .vertical ? DSSize.insertionBar : nil,
                height: axis == .horizontal ? DSSize.insertionBar : nil
            )
            .accessibilityHidden(true)
    }
}

#Preview("Page thumbnails") {
    HStack(alignment: .top, spacing: DSSpacing.xl) {
        ForEach(1...3, id: \.self) { page in
            VStack(spacing: DSSpacing.xs) {
                DSColor.surface
                    .frame(width: DSSize.thumbnailWidth, height: DSSize.thumbnailWidth * DSSize.pagePlaceholderAspect)
                    .dsPageThumbnail(isSelected: page == 2)
                DSPageCaption(page, isSelected: page == 2)
            }
        }
        DSInsertionBar()
            .frame(height: DSSize.thumbnailWidth * DSSize.pagePlaceholderAspect)
    }
    .padding(DSSpacing.xxl)
    .background(DSColor.canvas)
}
