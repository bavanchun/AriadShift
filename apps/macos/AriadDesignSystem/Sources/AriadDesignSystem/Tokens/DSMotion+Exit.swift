import SwiftUI

extension DSMotion {
    /// An exit (palette, footer layer) runs in this fraction of its enter duration.
    public static let exitDurationFactor: Double = 0.7

    /// How long a finished run shows its success check before the footer returns to its buttons.
    public static let doneHoldDuration: Double = 1.4

    /// The exit counterpart of `animation(reduceMotion:)`: the same curve in `exitDurationFactor` of the time.
    public func exitAnimation(reduceMotion: Bool) -> Animation {
        animation(reduceMotion: reduceMotion).speed(1 / Self.exitDurationFactor)
    }
}
