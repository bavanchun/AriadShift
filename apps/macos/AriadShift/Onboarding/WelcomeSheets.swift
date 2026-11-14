import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The layout both welcome sheets share: a title, the content and one primary button at the trailing edge.
struct WelcomeSheetLayout<Content: View>: View {
    let title: Text
    let buttonTitle: Text
    let buttonIdentifier: String
    let action: () -> Void
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: DSSpacing.l) {
            title
                .font(DSFont.title2)
                .accessibilityAddTraits(.isHeader)
            content
            HStack {
                Spacer()
                Button(action: action) { buttonTitle }
                    .buttonStyle(.dsPrimary)
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier(buttonIdentifier)
            }
            .padding(.top, DSSpacing.s)
        }
        .fixedSize(horizontal: false, vertical: true)
        .padding(DSSpacing.xxl)
        .frame(width: DSSize.welcomeSheetWidth)
        .background(DSColor.window)
    }
}

/// First-launch onboarding over the welcome window: what AriadShift is, where engine packs live, and that nothing
/// goes over the network by default. Shown until Get Started; Help > Show Welcome shows it again.
struct OnboardingSheet: View {
    let onStart: () -> Void

    /// `~/Library/Application Support/com.ariadnev.ariadshift/packs`, the folder the CLI shares.
    static var packsDirectory: String {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let packs = support.appendingPathComponent("com.ariadnev.ariadshift/packs", isDirectory: true)
        return (packs.path as NSString).abbreviatingWithTildeInPath
    }

    var body: some View {
        WelcomeSheetLayout(
            title: Text("mac.onboarding.title"),
            buttonTitle: Text("mac.onboarding.start"),
            buttonIdentifier: "onboarding.start",
            action: onStart
        ) {
            VStack(alignment: .leading, spacing: DSSpacing.m) {
                Text("mac.onboarding.about")
                Text(String(localized: "mac.onboarding.packs", defaultValue: "Packs: \(Self.packsDirectory)"))
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                Text("mac.onboarding.privacy")
            }
            .font(DSFont.body)
        }
    }
}

/// What's New, shown once over the welcome window after an update.
struct WhatsNewSheet: View {
    let version: String
    let onContinue: () -> Void

    struct Item: Identifiable {
        let id: String
        let symbol: String
        let title: LocalizedStringKey
        let detail: LocalizedStringKey
    }

    /// The release's highlights, newest first.
    static let items = [
        Item(id: "look", symbol: "sparkles", title: "mac.whats_new.look_title", detail: "mac.whats_new.look_detail"),
        Item(id: "tools", symbol: "command", title: "mac.whats_new.tools_title", detail: "mac.whats_new.tools_detail"),
        Item(id: "private", symbol: "lock.shield", title: "mac.whats_new.private_title", detail: "mac.whats_new.private_detail"),
    ]

    var body: some View {
        WelcomeSheetLayout(
            title: Text(String(localized: "mac.whats_new.title", defaultValue: "What’s New in AriadShift \(version)")),
            buttonTitle: Text("mac.whats_new.continue"),
            buttonIdentifier: "whatsNew.continue",
            action: onContinue
        ) {
            VStack(alignment: .leading, spacing: DSSpacing.l) {
                ForEach(Self.items) { item in
                    HStack(alignment: .top, spacing: DSSpacing.m) {
                        DSIconWell(systemImage: item.symbol)
                        VStack(alignment: .leading, spacing: DSSpacing.xs) {
                            Text(item.title).font(DSFont.headline)
                            Text(item.detail).font(DSFont.body).foregroundStyle(.secondary)
                        }
                    }
                    .accessibilityElement(children: .combine)
                }
            }
        }
    }
}
