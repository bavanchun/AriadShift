import SwiftUI

/// The inspector header: a 28pt icon well, the tool name (title2) and its outcome (callout, secondary).
public struct DSInspectorHeader: View {
    let systemImage: String
    let tint: DSIconWell.Tint
    let title: Text
    let subtitle: Text?
    @Environment(\.dsFlightHidesHeaderIcon) private var iconHidden

    public init(systemImage: String, tint: DSIconWell.Tint = .accent, title: Text, subtitle: Text? = nil) {
        self.systemImage = systemImage
        self.tint = tint
        self.title = title
        self.subtitle = subtitle
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: DSSpacing.xs) {
            HStack(spacing: DSSpacing.m) {
                DSIconWell(systemImage: systemImage, tint: tint)
                    .opacity(iconHidden ? 0 : 1)
                title
                    .font(DSFont.title2)
                    .lineLimit(1)
            }
            if let subtitle {
                subtitle
                    .font(DSFont.callout)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(DSSpacing.l)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// An inspector section: a caption header in capitals and its rows, with 16 padding and a 12 row gap.
public struct DSInspectorSection<Content: View>: View {
    let title: Text?
    let content: Content

    public init(_ title: Text? = nil, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: DSSpacing.m) {
            if let title {
                title
                    .textCase(.uppercase)
                    .font(DSFont.caption)
                    .foregroundStyle(.secondary)
                    .accessibilityAddTraits(.isHeader)
            }
            content
        }
        .padding(DSSpacing.l)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// One labelled inspector row: the label in a 96pt trailing-aligned column, the control filling the rest at 28pt.
/// An empty label keeps the column so controls stay on one leading edge.
public struct DSInspectorRow<Control: View>: View {
    let label: Text?
    let control: Control

    public init(_ label: Text?, @ViewBuilder control: () -> Control) {
        self.label = label
        self.control = control()
    }

    public var body: some View {
        HStack(spacing: DSSpacing.m) {
            (label ?? Text(verbatim: ""))
                .font(DSFont.body)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .frame(width: DSSize.inspectorLabelColumn, alignment: .trailing)
            control
                .frame(maxWidth: .infinity, minHeight: DSControlHeight.regular, alignment: .leading)
        }
    }
}

/// The pinned inspector footer: a top hairline and 16 padding around its content (Reset and Run, or a progress row).
public struct DSInspectorFooter<Content: View>: View {
    let content: Content

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public var body: some View {
        content
            .padding(DSSpacing.l)
            .frame(maxWidth: .infinity, alignment: .leading)
            .overlay(alignment: .top) {
                Rectangle()
                    .fill(DSColor.separator)
                    .frame(height: DSSize.hairline)
            }
    }
}

#Preview("Inspector") {
    @Previewable @State var preset = "Balanced"
    @Previewable @State var replace = false
    VStack(spacing: 0) {
        DSInspectorHeader(
            systemImage: "arrow.down.right.and.arrow.up.left",
            title: Text(verbatim: "Compress PDF"),
            subtitle: Text(verbatim: "Make the file smaller, keep text sharp.")
        )
        Divider()
        DSInspectorSection(Text(verbatim: "Quality")) {
            DSInspectorRow(Text(verbatim: "Preset")) {
                Picker(selection: $preset) {
                    Text(verbatim: "Balanced").tag("Balanced")
                    Text(verbatim: "Smallest file").tag("Smallest file")
                } label: { EmptyView() }
                .labelsHidden()
            }
        }
        DSInspectorSection(Text(verbatim: "Output")) {
            DSInspectorRow(Text(verbatim: "Estimate")) {
                Text(verbatim: "≈ 1.1 MB · 74% smaller").monospacedDigit()
            }
            DSInspectorRow(nil) {
                Toggle(isOn: $replace) { Text(verbatim: "Replace original") }
                    .toggleStyle(.switch)
            }
        }
        Spacer(minLength: 0)
        DSInspectorFooter {
            HStack {
                Button(String("Reset")) {}.buttonStyle(.dsSecondary)
                Spacer()
                Button(String("Run")) {}.buttonStyle(.dsPrimary)
            }
        }
    }
    .frame(width: DSSize.inspectorWidth, height: 480)
    .adaptiveGlass(.panel, cornerRadius: DSRadius.panel)
    .padding(DSSpacing.xl)
    .background(DSColor.canvas)
}
