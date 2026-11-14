# macOS UI/UX redesign: audit and research (2026-11-14)

The owner tested macOS v0.6.0 and rejected the UI. They found it ugly and misaligned, with no motion and no opening moment. Phases 18-20 contain no redesign work: phase 18 adds tool, editor and AI screens, phase 19 adds system integrations, and phase 20 covers hardening and the 1.0 launch.

This report combines a source audit of `apps/macos` at `e7eb7731` with external research.

## Audit: why it looks bad

1. **No visual system.** There are no tokens, no asset catalog, no AppIcon and no accent color.
   - The brand palette (sage/gold/ink/paper, Plus Jakarta Sans; `docs/brand/design-direction.md`) and the web tokens (`packages/ui/src/styles/globals.css`) never reach the Mac app.
   - The result looks like stock SwiftUI.
2. **Ad hoc spacing.** There are 7 padding values (2-28) and 9 spacing values (0-16), with no grid. This is the main source of misalignment.
3. **Fixed frames.** The toolbar page field (40/52/80), the 24pt find buttons, the 140pt grid cells, the 120pt thumbnails, and the 480/520 sheets and windows all use fixed sizes.
   - `PageGridView` computes columns by hand and uses a `-9` padding hack.
   - Min-only frames have already caused a split-view layout crash (guarded by `WindowLayoutStabilityTests`).
4. **Zero animations** anywhere in the app. Thumbnails pop in with no transition, and the sidebar scroll is not animated.
5. **Weak empty states, onboarding and discovery.**
   - Empty states and onboarding are plain text.
   - The window has no drop zone.
   - Tools are reachable only from the menu bar, with no in-window picker or palette.
6. **Inspector and sheets.**
   - The inspector is open by default and takes up to 400pt at an 840pt minimum window width.
   - The run sheet nests GroupBoxes inside GroupBoxes and jumps in height.
7. **Deployment target.** The app targets macOS 15 (owner decision in plan.md). Liquid Glass (`glassEffect`, `ConcentricRectangle`, `ToolbarSpacer`) requires macOS 26 and needs a fallback. `PhaseAnimator`, `KeyframeAnimator` and `.inspector` are available on 15.

## Recommendations

**P0. Use system chrome; never paint custom chrome.**
- Use `NavigationSplitView`, `.inspector` and `.toolbar` with `ToolbarSpacer`, plus `.searchable`. On macOS 26 and later the system draws these as Liquid Glass and animates them.
- Keep glass on controls only, never on content: pages, thumbnails and tables stay off glass (WWDC25 219 and 323).

**P0. One design-system package** (`AriadDesignSystem`).
- Tokens:
  - Spacing 4/8/12/16/24/32.
  - A type ramp on system text styles, with a brand display face for hero text only.
  - Semantic colors in an asset catalog, drawn from the brand palette, in light, dark and increased-contrast variants.
  - Radii, with `ConcentricRectangle` on macOS 26 and later.
  - Motion tokens.
- An `adaptiveGlass()` modifier is the only `#available(macOS 26, *)` branch, with `.regularMaterial` below 26.
- Components: EmptyState/DropZone, ToolCard, ProgressRow, InspectorSection, CommandPalette.
- Every component gets an `#Preview` gallery and `swift-snapshot-testing` (MIT) snapshots in light, dark and increased contrast.
- Literal paddings, frames and colors are banned outside the package, and a lint check enforces this.

**P0. Smoothness at the source.**
- Create the PDFKit representable once and diff-guard `updateNSView`.
- Use `LazyVGrid`/`List` with stable IDs, and no `GeometryReader` in rows (use `onGeometryChange` with bucketing).
- Run heavy work off the main actor, and send progress in coarse steps.
- Profile with Time Profiler and Animation Hitches.

**P1. Motion language.**
- Three named springs: snappy for controls, smooth for panels, and a light bounce for emphasis.
- Animation APIs:
  - `contentTransition(.numericText())` for counters.
  - `symbolEffect` for icon feedback.
  - `matchedGeometryEffect` for tool card → inspector.
  - `glassEffectID` morphing on macOS 26 and later.
- Animations are scoped: `withAnimation` or `.animation(_:value:)` only, never on a whole container.
- Bulk reloads use `Transaction` with animations disabled.
- Reduce Motion swaps movement for fades.

**P1. Information architecture for about 100 tools.**
- A ⌘K command palette with fuzzy search and recents.
- The sidebar groups tools by job (Organize, Edit, Convert, Secure, OCR, AI).
- A tool gallery with a one-line outcome per tool.
- A contextual toolbar with the 5 common actions.
- The empty state is a large drop zone, plus recents and 3-4 suggested tasks.
- Progress shows inline with cancel, and an undo toast appears on completion. No modal progress sheets.

**P1. Opening animation within the HIG.**
- The HIG says Mac apps need no launch screen and should launch instantly. If a splash is wanted, put it at the start of onboarding.
- So the real window appears immediately, and a 0.8-1.2 s logo-mark reveal plays in the welcome/empty window: a draw-on of the brand mark followed by the content settling in with `KeyframeAnimator`.
- The reveal is skippable, respects Reduce Motion and never blocks input.
- A "What's New" sheet appears on version change.
- Native SwiftUI or Core Animation is preferred. Lottie (Apache-2.0) and Rive (MIT) are permissive but add a dependency.

**Avoid:**
- Glass on content, glass on glass, and tinting everything.
- A blocking splash screen.
- An implicit `.animation` on a container.
- `GeometryReader` in rows.
- Custom title bars.
- Hard-coded values.
- Repeated popovers on macOS 27 (one unconfirmed crash report; prefer sheets or the inspector).

## Sources

WWDC25 [323](https://developer.apple.com/videos/play/wwdc2025/323/) and [219](https://developer.apple.com/videos/play/wwdc2025/219/); HIG [Launching](https://developer.apple.com/design/human-interface-guidelines/launching) and [Onboarding](https://developer.apple.com/design/human-interface-guidelines/onboarding); [Michael Tsai, SwiftUI in appleOS 27](https://mjtsai.com/blog/2026/06/19/swiftui-in-appleos-27); [Blake Crosley, SwiftUI performance](https://blakecrosley.com/blog/swiftui-performance-interop-ios-27); [lottie-ios](https://github.com/airbnb/lottie-ios); [rive-ios](https://github.com/rive-app/rive-ios); [swift-snapshot-testing](https://github.com/pointfreeco/swift-snapshot-testing).

The macOS 27 API gates and the reference-app patterns rest on secondary sources or general knowledge. Verify them against Apple's documentation when planning.

## Unresolved questions (owner)

- Brand look (sage/gold + Plus Jakarta Sans) or near-pure Apple system look?
- Should the opening animation play on first launch only, or every time the empty window opens?
- Visual mockups for approval before code?
- Should the redesign phase run before the remaining phase 18 UI tasks?
