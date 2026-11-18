import AriadDesignSystem
import SwiftUI

/// The welcome window's content (no document open): the brand header, the drop zone, the suggested tasks and the
/// recent documents in a 640 column with a 32 rhythm, with the opening played over it.
///
/// The mark is one view for the whole opening. It takes the frame of the centre anchor (112 pt) or of the header
/// slot (56 pt) through `matchedGeometryEffect`, so the move into the header is a frame change of the same view and
/// its keyframes keep running across it.
struct WelcomeView: View {
    let model: WelcomeModel
    let director: OpeningDirector

    @Namespace private var markSpace
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private enum Anchor: Hashable { case centre, slot }

    var body: some View {
        ZStack {
            column
            Color.clear
                .frame(width: DSSize.markHero, height: DSSize.markHero)
                .matchedGeometryEffect(id: Anchor.centre, in: markSpace)
                .accessibilityHidden(true)
            OpeningMark(director: director)
                .matchedGeometryEffect(id: director.markInHeader ? Anchor.slot : Anchor.centre, in: markSpace, isSource: false)
                .allowsHitTesting(false)
        }
        .opacity(director.contentOpacity)
        .frame(width: DSSize.welcomeWindow.width, height: DSSize.welcomeWindow.height)
        .background(DSColor.window)
    }

    private var column: some View {
        VStack(alignment: .leading, spacing: DSSpacing.xxl) {
            header
            dropZone.settling(index: 1, director: director, reduceMotion: reduceMotion)
            suggested.settling(index: 2, director: director, reduceMotion: reduceMotion)
            if !model.recents.isEmpty {
                recent.settling(index: 3, director: director, reduceMotion: reduceMotion)
            }
        }
        .frame(width: DSSize.welcomeColumn)
    }

    private var header: some View {
        HStack(spacing: DSSpacing.l) {
            Color.clear
                .frame(width: DSSize.markHeader, height: DSSize.markHeader)
                .matchedGeometryEffect(id: Anchor.slot, in: markSpace)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: DSSpacing.xs) {
                Text(verbatim: "AriadShift")
                    .font(DSFont.display)
                    .dsDisplayTracking()
                    .accessibilityAddTraits(.isHeader)
                Text("mac.welcome.tagline")
                    .font(DSFont.callout)
                    .foregroundStyle(.secondary)
            }
            .settling(index: 0, director: director, reduceMotion: reduceMotion)
        }
        .frame(height: DSSize.markHeader)
    }

    private var dropZone: some View {
        DSDropZone(
            title: Text("mac.welcome.drop_title"),
            alternative: Text("mac.welcome.drop_or"),
            openTitle: Text("mac.welcome.open"),
            isTargeted: model.isDropTargeted,
            acceptPulse: model.acceptPulse
        ) {
            model.chooseAndOpen()
        }
        .dropDestination(for: URL.self) { urls, _ in
            director.skip()
            return model.drop(urls)
        } isTargeted: { targeted in
            model.isDropTargeted = targeted
        }
        .accessibilityIdentifier("welcome.drop")
    }

    private var suggested: some View {
        HStack(spacing: DSSpacing.l) {
            rowLabel("mac.welcome.suggested")
            HStack(spacing: DSSpacing.s) {
                ForEach(WelcomeSuggestion.all) { suggestion in
                    Button {
                        model.choose(suggestion)
                    } label: {
                        Label(suggestion.title, systemImage: suggestion.symbol)
                    }
                    .buttonStyle(WelcomeChipStyle())
                    .accessibilityIdentifier("welcome.suggested.\(suggestion.id)")
                }
            }
        }
    }

    private var recent: some View {
        HStack(alignment: .top, spacing: DSSpacing.l) {
            rowLabel("mac.welcome.recent")
                .frame(height: DSSize.recentThumbnailHeight)
            HStack(alignment: .top, spacing: DSSpacing.s) {
                ForEach(model.recents) { document in
                    Button {
                        model.open([document.url])
                    } label: {
                        RecentDocumentCard(document: document)
                    }
                    .buttonStyle(RecentCardStyle())
                    .help(document.name)
                }
            }
        }
    }

    private func rowLabel(_ key: LocalizedStringKey) -> some View {
        Text(key)
            .font(DSFont.headline)
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .frame(width: DSSize.welcomeLabelColumn, alignment: .leading)
    }
}

private extension View {
    /// Content item `index` of the opening: hidden and 8 pt low until the director shows it, then settled.
    func settling(index: Int, director: OpeningDirector, reduceMotion: Bool) -> some View {
        let shown = index < director.shownItems
        return opacity(shown ? 1 : 0)
            .offset(y: shown || reduceMotion ? 0 : DSMotion.settleOffset)
    }
}

/// A suggested-task chip: a 28 pt capsule on `surface` with an accent icon, a hairline outer rim and `e1`. Hover fades a
/// fill in; press scales to 0.97 under `snappy`.
struct WelcomeChipStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        ChipBody(configuration: configuration)
    }

    private struct ChipBody: View {
        let configuration: Configuration
        @State private var hovering = false
        @Environment(\.accessibilityReduceMotion) private var reduceMotion

        var body: some View {
            configuration.label
                .labelStyle(ChipLabelStyle())
                .font(DSFont.body)
                .padding(.horizontal, DSSpacing.m)
                .frame(height: DSControlHeight.regular)
                .background {
                    // The rim is a ring just outside the capsule, as the mockup draws it (a 0.5 pt spread). A hairline
                    // stroked on the capsule's own edge is cut flat at the view bounds and leaves slivers at both ends.
                    Capsule().fill(DSColor.glassRim).padding(-DSSize.hairline)
                    Capsule().fill(DSColor.surface)
                    Capsule().fill(DSColor.accentSoft).opacity(hovering ? 1 : 0)
                }
                .dsElevation(.e1)
                .scaleEffect(configuration.isPressed && !reduceMotion ? DSMotion.pressScale : 1)
                .dsAnimation(.snappy, value: configuration.isPressed)
                .animation(.easeOut(duration: DSMotion.hoverFadeDuration), value: hovering)
                .onHover { hovering = $0 }
                .contentShape(Capsule())
        }
    }

    private struct ChipLabelStyle: LabelStyle {
        func makeBody(configuration: Configuration) -> some View {
            HStack(spacing: DSSpacing.s) {
                configuration.icon.foregroundStyle(DSColor.accent)
                configuration.title
            }
        }
    }
}

/// A recent document: the top of its first page (96 tall, radius 6, `e1`), the name middle-truncated and the date.
struct RecentDocumentCard: View {
    let document: RecentDocument

    var body: some View {
        VStack(alignment: .leading, spacing: DSSpacing.xs) {
            thumbnail
            VStack(alignment: .leading, spacing: 0) {
                Text(verbatim: document.name)
                    .font(DSFont.caption)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(verbatim: document.dateText)
                    .font(DSFont.caption)
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
            }
        }
        .frame(width: DSSize.recentCard.width, height: DSSize.recentCard.height, alignment: .top)
        .accessibilityElement(children: .combine)
    }

    private var thumbnail: some View {
        let shape = RoundedRectangle(cornerRadius: DSRadius.chip, style: .continuous)
        return ZStack(alignment: .top) {
            shape.fill(DSColor.surface)
            if let image = document.thumbnail {
                Image(nsImage: image)
                    .resizable()
                    .scaledToFill()
                    .frame(width: DSSize.recentCard.width, height: DSSize.recentThumbnailHeight, alignment: .top)
            } else {
                Image(systemName: "doc.text")
                    .font(DSFont.title2)
                    .foregroundStyle(.tertiary)
                    .frame(maxHeight: .infinity)
            }
        }
        .frame(width: DSSize.recentCard.width, height: DSSize.recentThumbnailHeight)
        .clipShape(shape)
    }
}

/// A recent card lifts 2 pt to `e2` on hover and presses to 0.97; under Reduce Motion it only changes elevation.
struct RecentCardStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        CardBody(configuration: configuration)
    }

    private struct CardBody: View {
        let configuration: Configuration
        @State private var hovering = false
        @Environment(\.accessibilityReduceMotion) private var reduceMotion

        var body: some View {
            configuration.label
                .dsElevation(hovering ? .e2 : .e1)
                .offset(y: hovering && !reduceMotion ? -DSMotion.hoverLift : 0)
                .scaleEffect(configuration.isPressed && !reduceMotion ? DSMotion.pressScale : 1)
                .dsAnimation(.snappy, value: hovering)
                .dsAnimation(.snappy, value: configuration.isPressed)
                .onHover { hovering = $0 }
                .contentShape(Rectangle())
        }
    }
}
