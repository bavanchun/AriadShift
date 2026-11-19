import SwiftUI
import XCTest
@testable import AriadDesignSystem

final class FlightLandingTests: XCTestCase {
    func testTheHeaderIconShowsUnlessAFlightHidesIt() {
        var environment = EnvironmentValues()
        XCTAssertFalse(environment.dsFlightHidesHeaderIcon)
        environment.dsFlightHidesHeaderIcon = true
        XCTAssertTrue(environment.dsFlightHidesHeaderIcon)
    }
}
