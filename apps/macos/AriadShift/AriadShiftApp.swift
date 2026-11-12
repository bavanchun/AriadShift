import SwiftUI

@main
struct AriadShiftApp: App {
    var body: some Scene {
        WindowGroup {
            FoundationStatusView()
        }
        .windowResizability(.contentSize)
    }
}
