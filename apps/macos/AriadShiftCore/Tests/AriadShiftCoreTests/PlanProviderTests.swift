import Foundation
import XCTest

@testable import AriadShiftCore

final class PlanProviderTests: XCTestCase {
    func testReleaseProviderIsFreeWhateverTheEnvironmentSays() throws {
        let key = "ASHIFT_PLAN"
        let previous = ProcessInfo.processInfo.environment[key]
        setenv(key, "pro", 1)
        defer { if let previous { setenv(key, previous, 1) } else { unsetenv(key) } }
        XCTAssertEqual(try ReleasePlanProvider().currentPlan(), .free)
    }

    func testOnlyNonFreePlansGrantPro() {
        XCTAssertFalse(EntitlementPlan.free.grantsPro)
        for plan in [EntitlementPlan.pro, .team, .enterprise] { XCTAssertTrue(plan.grantsPro) }
    }

    #if DEBUG
    private func provider(_ value: String?) -> EnvironmentPlanProvider {
        EnvironmentPlanProvider { value.map { ["ASHIFT_PLAN": $0] } ?? [:] }
    }

    func testEnvironmentProviderReadsTheCLIValues() throws {
        XCTAssertEqual(try provider(nil).currentPlan(), .free)
        XCTAssertEqual(try provider("free").currentPlan(), .free)
        XCTAssertEqual(try provider(" pro\n").currentPlan(), .pro)
        XCTAssertEqual(try provider("team").currentPlan(), .team)
        XCTAssertEqual(try provider("enterprise").currentPlan(), .enterprise)
    }

    func testEnvironmentProviderRejectsAnythingElse() {
        for bad in ["", "Pro", "platinum"] {
            XCTAssertThrowsError(try provider(bad).currentPlan(), bad) { error in
                guard case .invalidRequest = error as? EngineError else { return XCTFail("\(error)") }
            }
        }
    }

    func testAProClientRunsAProToolPastTheGateAndAFreeClientIsRefused() async throws {
        let input = try fixturePath("pdf/pd-vi-qd-1922-2026.pdf")
        let request = ToolRequest(tool: "pdf.sign_token", inputs: [input], paramsJSON: "{}")
        let free = try makeClient(planProvider: ReleasePlanProvider())
        do {
            _ = try await free.runTool(request)
            XCTFail("a Free client must not run a Pro tool")
        } catch let error as EngineError {
            XCTAssertTrue(error.isTierNotAllowed, "\(error)")
        }
        let pro = try makeClient(planProvider: provider("pro"))
        XCTAssertEqual(pro.displayPlan, .pro)
        do {
            _ = try await pro.runTool(request)
        } catch let error as EngineError {
            XCTAssertFalse(error.isTierNotAllowed, "plan pro must pass the host tier gate: \(error)")
        }
    }
    #endif
}
