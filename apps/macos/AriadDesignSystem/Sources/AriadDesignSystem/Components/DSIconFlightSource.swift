import AppKit
import SwiftUI

/// What a component reports when the user chooses one of its tools: the icon, and where it sits in the window.
public struct DSIconFlightLaunch: Equatable, Sendable {
    /// The id of the chosen item, or nil for a component that does not know one (a tool card).
    public let itemID: String?
    public let systemImage: String
    public let tint: DSIconWell.Tint
    /// The icon well's frame in window base coordinates (origin at the lower left, as AppKit reports it).
    public let windowFrame: CGRect

    public init(itemID: String?, systemImage: String, tint: DSIconWell.Tint, windowFrame: CGRect) {
        self.itemID = itemID
        self.systemImage = systemImage
        self.tint = tint
        self.windowFrame = windowFrame
    }
}

/// Called with the chosen icon just before the choice itself runs, so the icon can fly while the tool opens.
public typealias DSIconFlightHandler = @MainActor (DSIconFlightLaunch) -> Void

private struct DSIconFlightHandlerKey: EnvironmentKey {
    static var defaultValue: DSIconFlightHandler? { nil }
}

extension EnvironmentValues {
    var dsIconFlightHandler: DSIconFlightHandler? {
        get { self[DSIconFlightHandlerKey.self] }
        set { self[DSIconFlightHandlerKey.self] = newValue }
    }
}

extension View {
    /// Tells the tool cards and palette rows below this view to report their icon to `handler` when one is chosen.
    public func dsOnIconFlight(_ handler: DSIconFlightHandler?) -> some View {
        environment(\.dsIconFlightHandler, handler)
    }

    /// Lets `probe` read this view's frame in window coordinates whenever it is asked.
    public func dsWindowFrame(_ probe: DSWindowFrameProbe?) -> some View {
        background {
            if let probe { DSWindowFrameReader(probe: probe) }
        }
    }
}

/// Reads where a view sits in its window, on demand.
///
/// A document window is several hosting views: the sidebar, the detail pane and the inspector are separate columns, so
/// SwiftUI coordinate spaces and preferences do not reach from one to another. AppKit's frame conversion does, which
/// is why a flight measures its source, its layer and its landing pad this way.
@MainActor
public final class DSWindowFrameProbe {
    private weak var view: NSView?

    public init() {}

    /// Points the probe at `view`; the probe never keeps the view alive.
    public func attach(_ view: NSView?) {
        self.view = view
    }

    /// The view's frame in window base coordinates (origin at the lower left), or nil while the view is not in a window.
    public var windowFrame: CGRect? {
        guard let view, view.window != nil else { return nil }
        return view.convert(view.bounds, to: nil)
    }
}

/// An invisible view behind a SwiftUI view that gives a `DSWindowFrameProbe` something to measure. It never takes a
/// click and is not an accessibility element.
struct DSWindowFrameReader: NSViewRepresentable {
    let probe: DSWindowFrameProbe

    func makeNSView(context: Context) -> NSView {
        let view = ProbeView()
        probe.attach(view)
        return view
    }

    func updateNSView(_ view: NSView, context: Context) {
        probe.attach(view)
    }

    private final class ProbeView: NSView {
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
    }
}

/// One probe per key, so the rows of a list each have their own.
@MainActor
final class DSWindowFrameProbes {
    private var probes: [String: DSWindowFrameProbe] = [:]

    func probe(for key: String) -> DSWindowFrameProbe {
        if let existing = probes[key] { return existing }
        let created = DSWindowFrameProbe()
        probes[key] = created
        return created
    }
}
