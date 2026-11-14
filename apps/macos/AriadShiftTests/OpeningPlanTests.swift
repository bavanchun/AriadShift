import XCTest

@testable import AriadShift

/// Which opening plays and which sheet follows it, for every launch the welcome window can see.
final class OpeningPlanTests: XCTestCase {
    private func plan(
        lastSeen: String?, current: String = "0.7.0", needsOnboarding: Bool = false, firstAppearance: Bool = true,
        onboardingRequested: Bool = false, reduceMotion: Bool = false
    ) -> OpeningPlan {
        OpeningPlan.resolve(
            lastSeenVersion: lastSeen, currentVersion: current, needsOnboarding: needsOnboarding,
            firstAppearance: firstAppearance, onboardingRequested: onboardingRequested, reduceMotion: reduceMotion
        )
    }

    func testFirstLaunchPlaysTheFullOpeningThenOnboarding() {
        XCTAssertEqual(plan(lastSeen: nil, needsOnboarding: true), OpeningPlan(motion: .play(.full), sheet: .onboarding))
    }

    func testEveryOtherLaunchPlaysTheShortOpeningAlone() {
        XCTAssertEqual(plan(lastSeen: "0.7.0"), OpeningPlan(motion: .play(.short), sheet: nil))
    }

    func testAnUpdatePlaysTheFullOpeningThenWhatsNew() {
        XCTAssertEqual(plan(lastSeen: "0.6.0"), OpeningPlan(motion: .play(.full), sheet: .whatsNew))
        // A release before this one recorded no version; its users get What's New too.
        XCTAssertEqual(plan(lastSeen: nil), OpeningPlan(motion: .play(.full), sheet: .whatsNew))
    }

    func testReopeningTheWindowInTheSameLaunchPlaysTheShortOpening() {
        XCTAssertEqual(plan(lastSeen: "0.6.0", firstAppearance: false), OpeningPlan(motion: .play(.short), sheet: nil))
        XCTAssertEqual(plan(lastSeen: nil, needsOnboarding: true, firstAppearance: false).motion, .play(.short))
    }

    func testShowWelcomeAsksForOnboardingEvenWhenItIsDone() {
        XCTAssertEqual(plan(lastSeen: "0.7.0", firstAppearance: false, onboardingRequested: true).sheet, .onboarding)
        XCTAssertEqual(plan(lastSeen: "0.6.0", onboardingRequested: true).sheet, .onboarding, "onboarding wins over What's New")
    }

    func testReduceMotionCrossfadesInsteadOfEitherVariant() {
        XCTAssertEqual(plan(lastSeen: nil, needsOnboarding: true, reduceMotion: true), OpeningPlan(motion: .fade, sheet: .onboarding))
        XCTAssertEqual(plan(lastSeen: "0.7.0", reduceMotion: true).motion, .fade)
    }

    func testTheClipTakesOverOnlyBeforeTheBeadLands() {
        func path(_ readyAt: Double?, motion: OpeningPlan.Motion = .play(.full), reduceTransparency: Bool = false) -> HeroPath {
            HeroPath.resolve(motion: motion, reduceTransparency: reduceTransparency, readyAt: readyAt)
        }
        XCTAssertEqual(path(0), .clipAtStart, "ready when the opening starts: the clip plays from its first frame")
        XCTAssertEqual(path(-0.2), .clipAtStart)
        XCTAssertEqual(path(0.31), .clipLate(milliseconds: 310), "ready later: it starts at the elapsed time")
        XCTAssertEqual(path(0.639), .clipLate(milliseconds: 639))
        XCTAssertEqual(path(0.64), .vector, "from the bead's landing on, the vector glyph stays")
        XCTAssertEqual(path(nil), .vector, "a clip that cannot play leaves the vector glyph")
        XCTAssertEqual(path(0, motion: .play(.short)), .vector, "the short variant is always vector")
        XCTAssertEqual(path(0, motion: .fade), .vector, "the Reduce Motion crossfade is always vector")
        XCTAssertEqual(path(0, reduceTransparency: true), .vector)
        XCTAssertEqual(HeroPath.clipLate(milliseconds: 310).logName, "clip-late@310ms")
        XCTAssertEqual(HeroPath.clipAtStart.logName, "clip-at-0")
    }

    func testTheRecordKeepsTheLastSeenVersion() throws {
        let suite = "opening-plan-tests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let record = OpeningRecord(defaults: defaults)
        XCTAssertNil(record.lastSeenVersion)
        record.record("0.7.0")
        XCTAssertEqual(record.lastSeenVersion, "0.7.0")
    }
}
