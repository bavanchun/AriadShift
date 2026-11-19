import AppKit
import SwiftUI

/// Reports the window that hosts a SwiftUI view, so a sheet can attach to the right window instead of whichever one
/// happens to be key. Use it as a `.background`.
struct WindowReader: NSViewRepresentable {
    let onChange: @MainActor (NSWindow?) -> Void

    func makeNSView(context: Context) -> NSView {
        let view = TrackingView()
        view.onChange = onChange
        return view
    }

    func updateNSView(_ view: NSView, context: Context) {
        (view as? TrackingView)?.onChange = onChange
    }

    private final class TrackingView: NSView {
        var onChange: (@MainActor (NSWindow?) -> Void)?

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            let window = window
            MainActor.assumeIsolated { onChange?(window) }
        }
    }
}
