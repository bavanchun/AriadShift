import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// What the pinned footer of a tool in the inspector shows.
enum ToolRunFooter: Equatable {
    /// Reset and Run; Run is off while the tool cannot run.
    case buttons(canRun: Bool)
    /// The inline progress row: running, the success check, or the error with Details.
    case progress(DSRunState)

    /// Identifies the footer layer, so swapping layers animates while progress inside one layer only glides.
    var layer: String {
        switch self {
        case .buttons: "buttons"
        case .progress(.running): "running"
        case .progress(.done): "done"
        case .progress(.failed): "failed"
        }
    }
}

/// One tool in the inspector: the header (icon well, name, outcome, close), its scrolling sections, and the pinned
/// footer that morphs between Reset/Run and the inline progress row. It is a plain view of its inputs so every state
/// can be previewed and snapshot-tested without an engine.
struct ToolRunPanel<Content: View>: View {
    let systemImage: String
    let tint: DSIconWell.Tint
    let title: String
    let outcome: String
    let footer: ToolRunFooter
    let canClose: Bool
    let onClose: () -> Void
    let onReset: () -> Void
    let onRun: () -> Void
    let onCancel: () -> Void
    let onDismissError: () -> Void
    @ViewBuilder let content: Content

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(spacing: 0) {
            DSInspectorHeader(
                systemImage: systemImage,
                tint: tint,
                title: Text(verbatim: title),
                subtitle: Text(verbatim: outcome)
            )
            .overlay(alignment: .topTrailing) {
                if canClose {
                    Button(action: onClose) {
                        Image(systemName: "xmark")
                            .font(DSFont.caption)
                            .foregroundStyle(.secondary)
                            .frame(width: DSControlHeight.small, height: DSControlHeight.small)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(DSPressButtonStyle())
                    .help(String(localized: "mac.run.close_tool"))
                    .accessibilityLabel(String(localized: "mac.run.close_tool"))
                    .accessibilityIdentifier("run.close")
                    .padding(DSSpacing.m)
                }
            }
            hairline
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    content
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .scrollBounceBehavior(.basedOnSize)
            DSInspectorFooter {
                footerLayer
                    .id(footer.layer)
                    .transition(layerTransition)
            }
            .dsAnimation(.smooth, value: footer.layer)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("run.inspector")
    }

    private var hairline: some View {
        Rectangle()
            .fill(DSColor.separator)
            .frame(height: DSSize.hairline)
    }

    /// Footer layers swap with opacity and a scale from 0.96, opacity only under Reduce Motion.
    private var layerTransition: AnyTransition {
        reduceMotion ? .opacity : .opacity.combined(with: .scale(scale: DSMotion.enterScale))
    }

    @ViewBuilder
    private var footerLayer: some View {
        switch footer {
        case let .buttons(canRun):
            HStack(spacing: DSSpacing.s) {
                Button(String(localized: "mac.run.reset"), action: onReset)
                    .buttonStyle(.dsSecondary)
                    .accessibilityIdentifier("run.reset")
                Spacer(minLength: 0)
                Button(action: onRun) {
                    Text("mac.run.run")
                }
                .buttonStyle(.dsPrimary)
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(!canRun)
                .help(String(localized: "mac.run.run_help"))
                .accessibilityIdentifier("run.run")
            }
        case let .progress(state):
            DSProgressRow(
                state: state,
                cancelLabel: Text("mac.run.cancel"),
                detailsLabel: Text("mac.run.details"),
                onCancel: onCancel,
                onDismissError: onDismissError
            )
            .accessibilityIdentifier(progressIdentifier(state))
        }
    }

    private func progressIdentifier(_ state: DSRunState) -> String {
        switch state {
        case .running: "run.progress"
        case .done: "run.done"
        case .failed: "run.failed"
        }
    }
}

/// A note at the top of a tool in the inspector: why it cannot run here (a later release, another format), or that a
/// Pro tool needs a Pro plan.
struct ToolNote: View {
    let systemImage: String
    let message: String
    let identifier: String

    /// The note of a tool that cannot run here.
    static func unavailable(_ reason: UnavailableReason) -> ToolNote {
        ToolNote(
            systemImage: "info.circle",
            message: L10n.branch("mac.tools.disabled", reason.tooltipBranch),
            identifier: "run.unavailable"
        )
    }

    /// The note of a Pro tool on a plan that does not grant Pro: it opens, and the host refuses the run.
    static let proLocked = ToolNote(
        systemImage: "lock", message: String(localized: "mac.run.pro_notice"), identifier: "run.pro-notice"
    )

    var body: some View {
        DSInspectorSection {
            HStack(alignment: .firstTextBaseline, spacing: DSSpacing.s) {
                Image(systemName: systemImage)
                    .foregroundStyle(DSColor.accent)
                    .accessibilityHidden(true)
                Text(verbatim: message)
                    .font(DSFont.callout)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(DSSpacing.m)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(DSColor.accentSoft, in: RoundedRectangle(cornerRadius: DSRadius.control, style: .continuous))
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier(identifier)
        }
    }
}
