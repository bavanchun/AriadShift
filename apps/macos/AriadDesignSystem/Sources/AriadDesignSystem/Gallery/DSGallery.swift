import SwiftUI

/// Every token and component on one page, for previews and snapshot tests.
public struct DSGallery: View {
    @State private var page = 3
    @State private var query = "cmp"

    public init() {}

    public var body: some View {
        HStack(alignment: .top, spacing: DSSpacing.xxl) {
            VStack(alignment: .leading, spacing: DSSpacing.xl) {
                tokens
                DSDropZone(
                    title: Text(verbatim: "Drop PDFs or documents here"),
                    alternative: Text(verbatim: "or"),
                    openTitle: Text(verbatim: "Open…"),
                    isTargeted: false
                ) {}
                HStack(spacing: DSSpacing.m) {
                    DSToolCard(
                        systemImage: "arrow.down.right.and.arrow.up.left",
                        title: Text(verbatim: "Compress PDF"),
                        outcome: Text(verbatim: "Make the file smaller, keep text sharp.")
                    ) {}
                    DSToolCard(
                        systemImage: "sparkles",
                        tint: .thread,
                        title: Text(verbatim: "Summarize"),
                        outcome: Text(verbatim: "A short summary of the document."),
                        badge: Text(verbatim: "Get pack")
                    ) {}
                }
                HStack(spacing: DSSpacing.m) {
                    DSToolbarPageField(
                        page: $page,
                        pageCount: 20,
                        previousLabel: Text(verbatim: "Previous page"),
                        nextLabel: Text(verbatim: "Next page"),
                        fieldLabel: Text(verbatim: "Page")
                    )
                    Button(String("Reset")) {}.buttonStyle(.dsSecondary)
                    Button(String("Run")) {}.buttonStyle(.dsPrimary)
                }
                DSToast(DSToastContent(
                    message: "Compressed · 4.2 MB → 1.1 MB",
                    actions: [.init(id: "reveal", title: "Show in Finder"), .init(id: "undo", title: "Undo")]
                )) { _ in }
            }
            .frame(width: DSSize.welcomeColumn)
            VStack(alignment: .leading, spacing: DSSpacing.xl) {
                inspector
                DSCommandPalette(
                    query: $query,
                    sections: DSCommandPalette.previewSections,
                    placeholder: Text(verbatim: "Search tools and actions"),
                    dismissHint: Text(verbatim: "esc"),
                    emptyMessage: Text(verbatim: "No matching tools"),
                    onChoose: { _ in },
                    onDismiss: {}
                )
            }
        }
        .padding(DSSpacing.xxl)
        .background(DSColor.canvas)
    }

    private var tokens: some View {
        VStack(alignment: .leading, spacing: DSSpacing.m) {
            HStack(spacing: DSSpacing.m) {
                Text(verbatim: "AriadShift")
                    .font(DSFont.display)
                    .dsDisplayTracking()
                Spacer()
                ForEach(DSElevation.allCases, id: \.self) { level in
                    RoundedRectangle(cornerRadius: DSRadius.card, style: .continuous)
                        .fill(DSColor.surface)
                        .frame(width: DSSpacing.xxl, height: DSSpacing.xxl)
                        .dsElevation(level)
                }
            }
            HStack(spacing: DSSpacing.xs) {
                ForEach(DSColor.Name.allCases.filter { !$0.rawValue.hasPrefix("glass") && $0 != .shadow && $0 != .scrim }, id: \.self) { name in
                    RoundedRectangle(cornerRadius: DSRadius.chip, style: .continuous)
                        .fill(DSColor.named(name))
                        .overlay(
                            RoundedRectangle(cornerRadius: DSRadius.chip, style: .continuous)
                                .strokeBorder(DSColor.separator, lineWidth: DSSize.hairline)
                        )
                        .frame(height: DSSpacing.xl)
                        .help(name.rawValue)
                }
            }
            HStack(alignment: .firstTextBaseline, spacing: DSSpacing.l) {
                Text(verbatim: "Title 2").font(DSFont.title2)
                Text(verbatim: "Headline").font(DSFont.headline)
                Text(verbatim: "Body").font(DSFont.body)
                Text(verbatim: "Callout").font(DSFont.callout).foregroundStyle(.secondary)
                Text(verbatim: "Caption 1 / 20").font(DSFont.caption).monospacedDigit().foregroundStyle(.tertiary)
            }
        }
    }

    private var inspector: some View {
        VStack(spacing: 0) {
            DSInspectorHeader(
                systemImage: "arrow.down.right.and.arrow.up.left",
                title: Text(verbatim: "Compress PDF"),
                subtitle: Text(verbatim: "Make the file smaller, keep text sharp.")
            )
            DSInspectorSection(Text(verbatim: "Output")) {
                DSInspectorRow(Text(verbatim: "Estimate")) {
                    Text(verbatim: "≈ 1.1 MB · 74% smaller").monospacedDigit()
                }
                DSInspectorRow(Text(verbatim: "Save as")) {
                    Text(verbatim: "Q3-Board-Report-compressed.pdf").lineLimit(1).truncationMode(.middle)
                }
            }
            DSInspectorFooter {
                DSProgressRow(
                    state: .running(fraction: 0.42, step: "Optimizing images · page 7 of 20"),
                    cancelLabel: Text(verbatim: "Cancel"),
                    detailsLabel: Text(verbatim: "Details"),
                    onCancel: {}
                )
            }
            DSInspectorFooter {
                DSProgressRow(
                    state: .failed(message: "Page 14 couldn’t be recompressed. Nothing was changed.", details: nil),
                    cancelLabel: Text(verbatim: "Dismiss"),
                    detailsLabel: Text(verbatim: "Details"),
                    onCancel: {}
                )
            }
        }
        .frame(width: DSSize.inspectorWidth)
        .adaptiveGlass(.panel, cornerRadius: DSRadius.panel)
    }
}

#Preview("Design system gallery", traits: .fixedLayout(width: 1440, height: 900)) {
    DSGallery()
}
