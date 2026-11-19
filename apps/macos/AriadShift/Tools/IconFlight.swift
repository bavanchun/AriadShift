import AriadDesignSystem
import Observation
import SwiftUI

/// Whether choosing a tool should fly its icon into the inspector. Pure, so the rule is testable without a window.
enum IconFlightPolicy {
    /// An icon flies only when the choice really opens the tool: nothing runs, the tool is not already open, there is
    /// a document to open it on, and Reduce Motion is off (it crossfades instead: the chosen row fades out and the
    /// inspector fades in, which the palette and `DocumentSession.begin` already do).
    static func shouldFly(toolID: String, openToolID: String?, isRunning: Bool, canOpen: Bool, reduceMotion: Bool) -> Bool {
        guard DSIconFlightPlan.style(reduceMotion: reduceMotion) == .travel else { return false }
        return canOpen && !isRunning && openToolID != toolID
    }
}

/// The icon of a chosen tool flying from its palette row or tool card into the inspector header.
///
/// The palette and the inspector are different hosting views (the inspector is a split column of its own), so
/// `matchedGeometryEffect` cannot join them. The flight is one overlay layer over the whole window: it measures the
/// chosen icon, its own frame and the header's landing pad in window coordinates (`DSWindowFrameProbe`), draws a copy
/// of the icon in layer coordinates and moves it under `smooth`. The pad is looked up again every frame while the icon
/// is in the air, because the inspector may still be sliding open; the icon follows it and lands when the pad has stood
/// still long enough for the spring to arrive.
@MainActor
@Observable
final class IconFlightCoordinator {
    /// An icon in the air.
    struct Flight: Equatable {
        let systemImage: String
        let tint: DSIconWell.Tint
        /// Where the icon is drawn, in the layer's coordinates (origin at its top left).
        var frame: CGRect
        var opacity = 1.0
    }

    /// What the layer draws; nil while no icon is in the air.
    private(set) var flight: Flight?
    /// True while the flying icon stands in for the real header icon, which an inspector header that knows about
    /// flights keeps hidden until the copy lands.
    private(set) var hidesTarget = false

    /// The layer the icon is drawn in, and the pad it flies to, measured in window coordinates when asked.
    let layerProbe = DSWindowFrameProbe()
    let targetProbe = DSWindowFrameProbe()

    @ObservationIgnored private weak var session: DocumentSession?
    private let settler: DSFlightSettler
    private let sampleInterval: Double
    @ObservationIgnored private var task: Task<Void, Never>?

    init(
        session: DocumentSession? = nil,
        settler: DSFlightSettler = DSFlightSettler(),
        sampleInterval: Double = DSMotion.flightSampleInterval
    ) {
        self.session = session
        self.settler = settler
        self.sampleInterval = sampleInterval
    }

    /// Flies the icon of tool `toolID` if choosing it opens the tool. Called just before the choice runs.
    func fly(_ launch: DSIconFlightLaunch, toolID: String, reduceMotion: Bool) {
        guard IconFlightPolicy.shouldFly(
            toolID: toolID,
            openToolID: session?.run?.tool.id,
            isRunning: session?.isRunning ?? false,
            canOpen: session?.fileURL != nil,
            reduceMotion: reduceMotion
        ) else { return }
        start(launch, reduceMotion: reduceMotion)
    }

    /// Puts the icon in the air at its source and starts following the landing pad. A flight already in the air is
    /// replaced.
    func start(_ launch: DSIconFlightLaunch, reduceMotion: Bool) {
        cancel()
        guard DSIconFlightPlan.style(reduceMotion: reduceMotion) == .travel, let layer = layerProbe.windowFrame else { return }
        // No animation here: the copy appears exactly where the chosen icon is.
        flight = Flight(
            systemImage: launch.systemImage,
            tint: launch.tint,
            frame: DSIconFlightGeometry.localFrame(of: launch.windowFrame, in: layer)
        )
        hidesTarget = true
        task = Task { await follow() }
    }

    /// Takes the icon out of the air at once (the window closes, or a new flight replaces it).
    func cancel() {
        task?.cancel()
        task = nil
        flight = nil
        hidesTarget = false
    }

    private func follow() async {
        var samples = self.settler
        while !Task.isCancelled {
            switch samples.observe(padFrameInLayer()) {
            case .wait, .hold:
                break
            case let .retarget(frame):
                withDSAnimation(.smooth) {
                    flight?.frame = frame
                    return
                }
            case .land, .abandon:
                await land()
                return
            }
            try? await Task.sleep(for: .seconds(sampleInterval))
        }
    }

    /// The icon has arrived (or has nowhere to go): the real header icon shows again and the copy fades out over it.
    private func land() async {
        hidesTarget = false
        withDSAnimation(.fade) {
            flight?.opacity = 0
            return
        }
        try? await Task.sleep(for: .seconds(DSMotion.fadeDuration))
        guard !Task.isCancelled else { return }
        flight = nil
    }

    /// The pad in the layer's coordinates. A pad that is in the window but not laid out yet has no size; it does not
    /// count, so the icon never swerves towards the window's corner.
    private func padFrameInLayer() -> CGRect? {
        guard let layer = layerProbe.windowFrame, let pad = targetProbe.windowFrame, !pad.isEmpty else { return nil }
        return DSIconFlightGeometry.localFrame(of: pad, in: layer)
    }
}

/// The overlay the flying icon is drawn in. It fills the window content, takes no clicks and is hidden from
/// VoiceOver; the layer's own frame is what the coordinator measures, so the icon lands where it was aimed whatever
/// safe areas surround it.
struct IconFlightLayer: View {
    let coordinator: IconFlightCoordinator

    var body: some View {
        ZStack(alignment: .topLeading) {
            Color.clear
                .dsWindowFrame(coordinator.layerProbe)
            if let flight = coordinator.flight {
                DSIconWell(systemImage: flight.systemImage, tint: flight.tint)
                    .offset(x: flight.frame.minX, y: flight.frame.minY)
                    .opacity(flight.opacity)
            }
        }
        // Frames are physical, so the layer lays out left to right even in a right-to-left window.
        .environment(\.layoutDirection, .leftToRight)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}
