import SwiftUI

/// The state shown by a `DSProgressRow`.
public enum DSRunState: Equatable, Sendable {
    /// A run in progress: `fraction` from 0 to 1 and a one-line step text.
    case running(fraction: Double, step: String)
    /// A finished run: the check flashes from `thread` to `success`.
    case done(message: String)
    /// A failed run: one sentence, never truncated (at most two lines), with optional details behind a disclosure.
    case failed(message: String, details: String?)
}

/// The inline progress row of a tool run in the inspector footer.
///
/// Running: a 4pt `threadSoft` track with a `thread` fill that glides under `smooth`, the percent as a tabular numeric
/// transition, the step text and a Cancel (×) control. Done: a check that bounces from `thread` to `success` under
/// `emphasis`. Failed: a `danger` icon, the message and a Details disclosure that expands below with `smooth`.
public struct DSProgressRow: View {
    let state: DSRunState
    let cancelLabel: Text
    let detailsLabel: Text
    let onCancel: () -> Void
    let onDismissError: (() -> Void)?

    @State private var showsDetails = false
    @State private var doneFlash = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    public init(
        state: DSRunState,
        cancelLabel: Text,
        detailsLabel: Text,
        onCancel: @escaping () -> Void,
        onDismissError: (() -> Void)? = nil
    ) {
        self.state = state
        self.cancelLabel = cancelLabel
        self.detailsLabel = detailsLabel
        self.onCancel = onCancel
        self.onDismissError = onDismissError
    }

    public var body: some View {
        Group {
            switch state {
            case let .running(fraction, step):
                running(fraction: fraction, step: step)
            case let .done(message):
                done(message: message)
            case let .failed(message, details):
                failed(message: message, details: details)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func running(fraction: Double, step: String) -> some View {
        let clamped = min(max(fraction, 0), 1)
        return VStack(alignment: .leading, spacing: DSSpacing.s) {
            HStack(spacing: DSSpacing.s) {
                ProgressTrack(fraction: clamped)
                    .dsAnimation(.smooth, value: clamped)
                Text(clamped, format: .percent.precision(.fractionLength(0)))
                    .font(DSFont.caption)
                    .monospacedDigit()
                    .contentTransition(.numericText(value: clamped))
                    .dsAnimation(.snappy, value: clamped)
                    .frame(minWidth: DSSpacing.xxl, alignment: .trailing)
                Button(action: onCancel) {
                    Image(systemName: "xmark.circle.fill")
                        .font(DSFont.headline)
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(cancelLabel)
                .keyboardShortcut(.cancelAction)
            }
            Text(step)
                .font(DSFont.callout)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .accessibilityElement(children: .combine)
        .accessibilityValue(Text(clamped, format: .percent.precision(.fractionLength(0))))
    }

    private func done(message: String) -> some View {
        HStack(spacing: DSSpacing.s) {
            Image(systemName: "checkmark.circle.fill")
                .font(DSFont.title2)
                .foregroundStyle(doneFlash ? DSColor.success : DSColor.thread)
                .symbolEffect(.bounce, value: doneFlash)
            Text(message)
                .font(DSFont.callout)
                .monospacedDigit()
                .lineLimit(1)
        }
        .onAppear {
            withAnimation(DSMotion.emphasis.animation(reduceMotion: reduceMotion)) { doneFlash = true }
        }
    }

    private func failed(message: String, details: String?) -> some View {
        VStack(alignment: .leading, spacing: DSSpacing.s) {
            HStack(alignment: .firstTextBaseline, spacing: DSSpacing.s) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(DSColor.danger)
                    .accessibilityHidden(true)
                Text(message)
                    .font(DSFont.callout)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if let onDismissError {
                    Button(action: onDismissError) {
                        Image(systemName: "xmark")
                            .font(DSFont.caption)
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(cancelLabel)
                }
            }
            if let details {
                DisclosureGroup(isExpanded: $showsDetails) {
                    Text(details)
                        .font(DSFont.callout)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.top, DSSpacing.xs)
                } label: {
                    detailsLabel.font(DSFont.callout)
                }
                .dsAnimation(.smooth, value: showsDetails)
            }
        }
    }
}

/// The 4pt progress track. The fill is a shape whose width animates, so it glides instead of jumping.
private struct ProgressTrack: View {
    let fraction: Double

    var body: some View {
        ProgressFill(fraction: 1)
            .fill(DSColor.threadSoft)
            .overlay(ProgressFill(fraction: fraction).fill(DSColor.thread))
            .frame(height: DSControlHeight.progressTrack)
            .frame(maxWidth: .infinity)
    }
}

private struct ProgressFill: Shape {
    var fraction: Double

    var animatableData: Double {
        get { fraction }
        set { fraction = newValue }
    }

    func path(in rect: CGRect) -> Path {
        let width = rect.width * min(max(fraction, 0), 1)
        guard width > 0 else { return Path() }
        let filled = CGRect(x: rect.minX, y: rect.minY, width: max(width, rect.height), height: rect.height)
        return Capsule().path(in: filled)
    }
}

#Preview("Progress row") {
    VStack(spacing: DSSpacing.xl) {
        DSProgressRow(
            state: .running(fraction: 0.42, step: "Optimizing images · page 7 of 20"),
            cancelLabel: Text(verbatim: "Cancel"),
            detailsLabel: Text(verbatim: "Details"),
            onCancel: {}
        )
        DSProgressRow(
            state: .done(message: "Compressed · 4.2 MB → 1.1 MB"),
            cancelLabel: Text(verbatim: "Cancel"),
            detailsLabel: Text(verbatim: "Details"),
            onCancel: {}
        )
        DSProgressRow(
            state: .failed(
                message: "Page 14 couldn’t be recompressed. Nothing was changed.",
                details: "pdf.compress: image stream 37 uses an unsupported JBIG2 filter."
            ),
            cancelLabel: Text(verbatim: "Cancel"),
            detailsLabel: Text(verbatim: "Details"),
            onCancel: {},
            onDismissError: {}
        )
    }
    .frame(width: DSSize.inspectorWidth - 2 * DSSpacing.l)
    .padding(DSSpacing.l)
    .background(DSColor.window)
}
