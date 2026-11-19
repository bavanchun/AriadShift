import SwiftUI

extension EnvironmentValues {
    /// True while a copy of a chosen tool's icon is flying towards the inspector header. The header hides its own icon
    /// well until the copy lands, so the icon never shows twice. Nothing sets it unless the app runs an icon flight.
    @Entry public var dsFlightHidesHeaderIcon = false
}
