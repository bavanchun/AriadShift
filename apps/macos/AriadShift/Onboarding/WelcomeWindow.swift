import AVFoundation
import AriadDesignSystem
import AriadShiftCore
import Observation
import SwiftUI

/// Requests to the welcome window from elsewhere in the app: Help > Show Welcome asks for the onboarding sheet.
@MainActor
@Observable
final class WelcomePresentation {
    static let shared = WelcomePresentation()

    private(set) var onboardingRequested = false
    /// Whether the welcome window has appeared since the app launched; the full opening plays only on the first.
    fileprivate var hasAppeared = false

    /// Shows the onboarding sheet over the welcome window, now if it is open or when it next appears.
    func requestOnboarding() {
        onboardingRequested = true
    }

    fileprivate func consumeOnboardingRequest() -> Bool {
        defer { onboardingRequested = false }
        return onboardingRequested
    }
}

/// The welcome window (no document open): it plays the opening over the welcome content, then shows the onboarding
/// or What's New sheet the plan calls for.
struct WelcomeWindow: View {
    let imports: ImportModel

    @EnvironmentObject private var engine: EngineModel
    @Environment(\.openWindow) private var openWindow
    @Environment(\.dismissWindow) private var dismissWindow
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.displayScale) private var displayScale

    @State private var session = WelcomeSession()
    private let presentation = WelcomePresentation.shared

    var body: some View {
        WelcomeView(model: session.model, director: session.director)
            .sheet(item: $session.sheet) { sheet in
                switch sheet {
                case .onboarding:
                    OnboardingSheet {
                        AppSettings().completeOnboarding()
                        session.sheet = nil
                    }
                case .whatsNew:
                    WhatsNewSheet(version: OpeningRecord.currentVersion) { session.sheet = nil }
                }
            }
            .onAppear {
                session.model.openFile = openFile
                session.model.onOpened = { dismissWindow(id: "welcome") }
                session.begin(hero: OpeningHero.shared.player(
                    for: session.director.motion, scheme: colorScheme, reduceTransparency: reduceTransparency
                ))
            }
            .task { await session.model.loadThumbnails(scale: displayScale) }
            .onChange(of: presentation.onboardingRequested) { _, requested in
                if requested { session.requestOnboarding() }
            }
    }

    /// A PDF opens in a document window; another importable format goes through File > Import.
    private func openFile(_ url: URL) -> Bool {
        if url.pathExtension.lowercased() == "pdf" {
            NSDocumentController.shared.openDocument(withContentsOf: url, display: true) { _, _, _ in }
            return true
        }
        guard ImportSource.source(for: url) != nil, let client = engine.client else { return false }
        imports.begin(url, client: client)
        openWindow(id: "import")
        return true
    }
}

/// One appearance of the welcome window: its plan, opening and content state.
@MainActor
@Observable
final class WelcomeSession {
    let plan: OpeningPlan
    let model: WelcomeModel
    let director: OpeningDirector
    var sheet: OpeningPlan.Sheet?
    @ObservationIgnored private var pendingSheet: OpeningPlan.Sheet?

    init(record: OpeningRecord = OpeningRecord(), presentation: WelcomePresentation = .shared) {
        plan = OpeningPlan.resolve(
            lastSeenVersion: record.lastSeenVersion,
            currentVersion: OpeningRecord.currentVersion,
            needsOnboarding: AppSettings().needsOnboarding,
            firstAppearance: !presentation.hasAppeared,
            onboardingRequested: presentation.onboardingRequested,
            reduceMotion: DSMotion.systemReduceMotion
        )
        model = WelcomeModel(recents: WelcomeModel.currentRecents())
        director = OpeningDirector(motion: plan.motion)
        pendingSheet = plan.sheet
    }

    /// Plays the opening; the planned sheet follows when it ends.
    func begin(hero: AVPlayer?) {
        let presentation = WelcomePresentation.shared
        presentation.hasAppeared = true
        _ = presentation.consumeOnboardingRequest()
        OpeningRecord().record(OpeningRecord.currentVersion)
        director.onFinish = { [weak self] in
            guard let self else { return }
            sheet = pendingSheet
            pendingSheet = nil
        }
        director.begin(hero: hero)
    }

    /// Help > Show Welcome while the window is open: the onboarding sheet, after the opening if it still plays.
    func requestOnboarding() {
        _ = WelcomePresentation.shared.consumeOnboardingRequest()
        if director.isPlaying {
            pendingSheet = .onboarding
        } else {
            sheet = .onboarding
        }
    }
}
