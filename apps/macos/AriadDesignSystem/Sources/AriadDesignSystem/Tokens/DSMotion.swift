import AppKit
import SwiftUI

/// Motion tokens. Only these springs exist; under Reduce Motion every one of them becomes `fade`.
/// Animate transform and opacity only, and scope animations with `dsAnimation(_:value:)` or `withDSAnimation`.
public enum DSMotion: Sendable, CaseIterable {
    /// Hover, press, toggles, selection, list reorder.
    case snappy
    /// Panels, inspector, palette, page transitions.
    case smooth
    /// Success, the bead landing in the opening, drop accepted.
    case emphasis
    /// The Reduce Motion replacement for any movement.
    case fade

    /// SwiftUI spring parameters, or nil for `fade`.
    public var spring: (response: Double, dampingFraction: Double)? {
        switch self {
        case .snappy: (0.25, 0.9)
        case .smooth: (0.42, 1.0)
        case .emphasis: (0.45, 0.72)
        case .fade: nil
        }
    }

    public static let fadeDuration: Double = 0.18

    /// The animation for this token, or `fade` when Reduce Motion is on.
    public func animation(reduceMotion: Bool) -> Animation {
        guard !reduceMotion, let spring else { return .easeOut(duration: Self.fadeDuration) }
        return .spring(response: spring.response, dampingFraction: spring.dampingFraction)
    }

    /// Delay between staggered items.
    public static let staggerStep: Double = 0.03
    /// Items after this many appear together with the last staggered one.
    public static let maxStaggeredItems = 6
    /// Content that settles in moves at most this far.
    public static let settleOffset: CGFloat = 8
    /// Scale of a pressed control.
    public static let pressScale: CGFloat = 0.97
    /// Scale content enters from (palette, cards).
    public static let enterScale: CGFloat = 0.96
    /// Hover fills fade in over this time; nothing moves on hover.
    public static let hoverFadeDuration: Double = 0.12
    /// A toast stays this long unless the pointer rests on it.
    public static let toastLifetime: Double = 6

    /// Stagger delay for item `index`, capped at `maxStaggeredItems`.
    public static func staggerDelay(for index: Int) -> Double {
        Double(min(max(index, 0), maxStaggeredItems - 1)) * staggerStep
    }

    /// The system Reduce Motion setting, for imperative animations outside a view body.
    @MainActor public static var systemReduceMotion: Bool {
        NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
    }
}

/// Runs `body` inside the token's animation, honouring the system Reduce Motion setting.
@MainActor
@discardableResult
public func withDSAnimation<Result>(_ motion: DSMotion, _ body: () throws -> Result) rethrows -> Result {
    try withAnimation(motion.animation(reduceMotion: DSMotion.systemReduceMotion), body)
}

extension View {
    /// Animates changes of `value` with the token, or `fade` under Reduce Motion.
    public func dsAnimation<V: Equatable>(_ motion: DSMotion, value: V) -> some View {
        modifier(MotionModifier(motion: motion, value: value))
    }

    /// Settles content in: opacity plus an 8pt rise, or opacity only under Reduce Motion.
    public func dsSettleTransition() -> some View {
        modifier(SettleTransitionModifier())
    }
}

extension AnyTransition {
    /// Opacity plus an 8pt rise; pass the Reduce Motion setting to get opacity only.
    public static func dsSettle(reduceMotion: Bool) -> AnyTransition {
        reduceMotion ? .opacity : .opacity.combined(with: .offset(y: DSMotion.settleOffset))
    }

    /// Opacity plus a scale from 0.96 and an 8pt rise (palette, cards); opacity only under Reduce Motion.
    public static func dsEnter(reduceMotion: Bool) -> AnyTransition {
        reduceMotion
            ? .opacity
            : .opacity
                .combined(with: .scale(scale: DSMotion.enterScale))
                .combined(with: .offset(y: DSMotion.settleOffset))
    }
}

private struct MotionModifier<V: Equatable>: ViewModifier {
    let motion: DSMotion
    let value: V
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content.animation(motion.animation(reduceMotion: reduceMotion), value: value)
    }
}

private struct SettleTransitionModifier: ViewModifier {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content.transition(.dsSettle(reduceMotion: reduceMotion))
    }
}

/// A button style that presses to 0.97 under `snappy` (no movement under Reduce Motion).
public struct DSPressButtonStyle: ButtonStyle {
    public init() {}

    public func makeBody(configuration: Configuration) -> some View {
        PressBody(configuration: configuration)
    }

    private struct PressBody: View {
        let configuration: Configuration
        @Environment(\.accessibilityReduceMotion) private var reduceMotion

        var body: some View {
            configuration.label
                .scaleEffect(configuration.isPressed && !reduceMotion ? DSMotion.pressScale : 1)
                .opacity(configuration.isPressed && reduceMotion ? 0.8 : 1)
                .dsAnimation(.snappy, value: configuration.isPressed)
        }
    }
}
