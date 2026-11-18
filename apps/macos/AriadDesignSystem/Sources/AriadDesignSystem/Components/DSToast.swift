import SwiftUI

/// What a toast says and offers.
public struct DSToastContent: Identifiable, Equatable, Sendable {
    public struct Action: Identifiable, Equatable, Sendable {
        public let id: String
        public let title: String

        public init(id: String, title: String) {
            self.id = id
            self.title = title
        }
    }

    public let id: UUID
    public let systemImage: String
    /// The message; sizes and counts render with tabular digits.
    public let message: String
    public let actions: [Action]

    public init(id: UUID = UUID(), systemImage: String = "checkmark", message: String, actions: [Action] = []) {
        self.id = id
        self.systemImage = systemImage
        self.message = message
        self.actions = actions
    }
}

/// A 44pt glass capsule with `e3`: a `threadSoft` icon well, the message in tabular digits, then plain actions
/// such as Show in Finder and Undo.
public struct DSToast: View {
    let content: DSToastContent
    let onAction: (DSToastContent.Action) -> Void

    public init(_ content: DSToastContent, onAction: @escaping (DSToastContent.Action) -> Void) {
        self.content = content
        self.onAction = onAction
    }

    public var body: some View {
        HStack(spacing: DSSpacing.m) {
            Image(systemName: content.systemImage)
                .font(DSFont.caption)
                .foregroundStyle(DSColor.thread)
                // A 28pt circle inset 8 inside the 44pt capsule is concentric with it (22 - 8 = 14).
                .frame(width: DSSize.iconWell, height: DSSize.iconWell)
                .background(DSColor.threadSoft, in: Circle())
                .accessibilityHidden(true)
            Text(content.message)
                .font(DSFont.body)
                .monospacedDigit()
                .lineLimit(1)
            ForEach(content.actions) { action in
                Button(action.title) { onAction(action) }
                    .buttonStyle(.plain)
                    .font(DSFont.headline)
                    .foregroundStyle(DSColor.accent)
            }
        }
        .padding(.leading, DSSpacing.s)
        .padding(.trailing, DSSpacing.l)
        .frame(height: DSControlHeight.toast)
        .adaptiveGlass(.floating, in: Capsule())
        .dsElevation(.e3)
        .accessibilityElement(children: .contain)
    }
}

extension View {
    /// Presents `toast` at the bottom center. It rises 8pt and fades in under `smooth`, dismisses itself after
    /// six seconds, and the timer pauses while the pointer rests on it.
    public func dsToast(
        _ toast: Binding<DSToastContent?>,
        onAction: @escaping (DSToastContent.Action) -> Void
    ) -> some View {
        overlay(alignment: .bottom) {
            ToastPresenter(toast: toast, onAction: onAction)
                .padding(.bottom, DSSpacing.xl)
        }
    }
}

private struct ToastPresenter: View {
    @Binding var toast: DSToastContent?
    let onAction: (DSToastContent.Action) -> Void
    @State private var isHovered = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        ZStack {
            if let toast {
                DSToast(toast) { action in
                    onAction(action)
                    self.toast = nil
                }
                .onHover { isHovered = $0 }
                .transition(.dsSettle(reduceMotion: reduceMotion))
                // Restarts whenever a new toast arrives or the hover state changes; hovering pauses the countdown.
                .task(id: TimerKey(id: toast.id, paused: isHovered)) {
                    guard !isHovered else { return }
                    try? await Task.sleep(for: .seconds(DSMotion.toastLifetime))
                    guard !Task.isCancelled, self.toast?.id == toast.id else { return }
                    self.toast = nil
                }
            }
        }
        .dsAnimation(.smooth, value: toast?.id)
    }

    private struct TimerKey: Equatable {
        let id: UUID
        let paused: Bool
    }
}

#Preview("Toast") {
    VStack(spacing: DSSpacing.xl) {
        DSToast(DSToastContent(
            message: "Compressed · 4.2 MB → 1.1 MB",
            actions: [.init(id: "reveal", title: "Show in Finder"), .init(id: "undo", title: "Undo")]
        )) { _ in }
        DSToast(DSToastContent(systemImage: "trash", message: "Deleted 3 pages", actions: [.init(id: "undo", title: "Undo")])) { _ in }
    }
    .padding(DSSpacing.xxl)
    .background(DSColor.canvas)
}
