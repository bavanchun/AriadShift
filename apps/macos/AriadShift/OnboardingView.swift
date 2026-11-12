import AriadShiftCore
import SwiftUI

/// First-launch welcome: what AriadShift is, where engine packs live, and that nothing goes
/// over the network by default. Shown once; Help > Show Welcome opens it again.
struct OnboardingView: View {
    @Environment(\.dismissWindow) private var dismissWindow

    /// `~/Library/Application Support/com.ariadnev.ariadshift/packs`, the folder the CLI shares.
    static var packsDirectory: String {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let packs = support.appendingPathComponent("com.ariadnev.ariadshift/packs", isDirectory: true)
        return (packs.path as NSString).abbreviatingWithTildeInPath
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("mac.onboarding.title")
                .font(.largeTitle.bold())
                .accessibilityAddTraits(.isHeader)
            Text("mac.onboarding.about")
            Text(String(localized: "mac.onboarding.packs", defaultValue: "Packs: \(Self.packsDirectory)"))
            Text("mac.onboarding.privacy")
            HStack {
                Spacer()
                Button(String(localized: "mac.onboarding.start")) {
                    AppSettings().completeOnboarding()
                    dismissWindow(id: "welcome")
                }
                .keyboardShortcut(.defaultAction)
                .controlSize(.large)
                .accessibilityIdentifier("onboarding.start")
            }
        }
        .fixedSize(horizontal: false, vertical: true)
        .padding(28)
        .frame(width: 520)
    }
}
