import PDFKit
import SwiftUI

/// Hosts the model's `PDFView` in SwiftUI. The view belongs to `ViewerModel`, so the toolbar and
/// the menu commands act on the same view that is on screen.
struct PDFViewRepresentable: NSViewRepresentable {
    let model: ViewerModel
    // Read so the view updates when the appearance changes between light and dark.
    @Environment(\.colorScheme) private var colorScheme

    func makeNSView(context: Context) -> PDFView {
        model.pdfView
    }

    func updateNSView(_ view: PDFView, context: Context) {
        // The area around the pages follows the system: gray in light mode, near black in dark mode.
        view.backgroundColor = colorScheme == .dark ? .black : .underPageBackgroundColor
    }
}
