---
phase: 18a
title: "macOS UI/UX redesign: design system, window shell, motion and opening animation"
status: in-progress
priority: P0
effort: "5 tasks, ~8 worker-days"
dependencies: [17]
blocks: "phase 18 UI tasks T1 remainder, T2, T3, T5, T6"
branch: feat/macos-design-system
milestone: E (macOS) — lands before macOS preview 0.8.0
---

# Phase 18a: macOS UI/UX redesign

Ownership (owner decision 2026-11-14): the lead owns this whole phase: idea, design, plan, alignment and review. One dedicated designer/implementer agent works beside the lead. The coordinator and its workers do not touch 18a; they keep all other work.

Design authority: [design spec](./mockups/macos-redesign/design-spec.md). Evidence: [audit + research](../reports/researcher-261114-1224-macos-ui-redesign.md).

## Why

The owner rejected the v0.6.0 Mac UI (2026-11-14). In their words it is extremely ugly, misaligned everywhere, not smooth, and has no opening moment. The audit found the causes:
- No tokens, asset catalog or AppIcon.
- Seven ad hoc padding values.
- Fixed frames and a `-9` padding hack.
- Zero animations.
- Text-only empty states.
- Tools reachable only from the menu bar.

Phases 18-20 contain no redesign work. The remaining phase 18 UI tasks would otherwise add more screens on this base.

## Owner decisions (2026-11-14)

| Decision | Choice |
|---|---|
| Look | Apple system chrome (Liquid Glass on macOS 26+, `.regularMaterial` fallback on 15-25) plus AriadShift brand: sage/gold accent, brand mark, Plus Jakarta Sans for display text only |
| Opening animation | Plays on every launch into the welcome/empty window: full ~1.2 s on first launch and after an update, short ~0.6 s otherwise. Skippable, never blocks input, Reduce Motion swaps it for a fade. No blocking splash (HIG). |
| Approval | The owner approves an interactive mockup (screens + motion) before any Swift code |
| Order | Now, before the remaining phase 18 UI tasks. 0.8.0 moves accordingly. |

Minimum macOS stays 15 (plan decision). Every macOS 26 API lives behind one availability wrapper.

## Requirements

1. **Design-system package `AriadDesignSystem`** (Swift package in `apps/macos`).
   - Tokens:
     - Spacing scale 4/8/12/16/24/32.
     - Type ramp on system text styles plus a brand display style.
     - Semantic colors in an asset catalog derived from `docs/brand/design-direction.md` and `packages/ui` tokens (light, dark, increased contrast).
     - Radii, with concentric corners on 26+.
     - Motion tokens: `snappy` for controls, `smooth` for panels, light `bouncy` for emphasis.
   - `adaptiveGlass()` is the only `#available(macOS 26, *)` branch.
   - Components:
     - EmptyState/DropZone
     - ToolCard
     - ProgressRow
     - InspectorSection
     - CommandPalette
     - Toast
     - ToolbarPageField
   - An `#Preview` gallery and snapshot tests (`swift-snapshot-testing`, MIT) in light, dark and increased contrast.
   - A lint check fails on literal padding, frame, color or font values outside the package.
2. **Asset catalog + AppIcon + AccentColor** generated from `brand/` (no hand-edited generated assets). Track the `Info.plist` question raised by the audit.
3. **Window shell rebuild.**
   - `NavigationSplitView` sidebar: thumbnails, plus tool groups by job (Organize, Edit, Convert, Secure, OCR, AI).
   - `.inspector`: closed by default, opens on tool selection.
   - `.toolbar` with `ToolbarSpacer` groups and a compact page field.
   - Find moves into `.searchable` / toolbar, so the separate find strip is gone.
   - Page grid on `LazyVGrid(.adaptive)` with no hand-computed columns.
   - Whole-window drop target.
   - Must keep `WindowLayoutStabilityTests` green, so no min-only frames inside split columns.
4. **Tool discovery.**
   - ⌘K command palette with fuzzy search and recents, as an in-window overlay.
   - Tool gallery cards with a one-line outcome each.
   - Contextual toolbar for the 5 most common actions.
   - The run sheet is replaced by inspector-inline params with inline progress, cancel, and an undo/"Show in Finder" toast.
5. **Motion pass.**
   - Scoped animations only (`withAnimation` / `.animation(_:value:)`), with no container-wide implicit animation.
   - Thumbnails fade in.
   - Sidebar scroll is animated.
   - Panels use the `smooth` spring.
   - `matchedGeometryEffect` from tool card to inspector.
   - `contentTransition(.numericText())` on counters and progress; `symbolEffect` on status icons.
   - Bulk reloads disable animation via `Transaction`.
   - Reduce Motion is honoured everywhere.
6. **Opening animation and onboarding.**
   - Brand mark draw-on followed by a staggered content settle (`KeyframeAnimator`/`PhaseAnimator`, native only, no Lottie).
   - Plays in the welcome/empty window per the owner decision.
   - Welcome window: drop zone, recents, 3-4 suggested tasks.
   - A "What's New" sheet on version change.
7. **Smoothness.**
   - The PDFKit representable is created once, and `updateNSView` is diff-guarded.
   - Thumbnails, tool runs and OCR run off the main actor.
   - Instruments Time Profiler and Animation Hitches traces before and after, recorded in the PR.

## Tasks

| Task | Owner | Files owned | Gate |
|---|---|---|---|
| T0 | Designer (lead reviews) | Interactive HTML mockup `plans/261031-1422-next-stage-pdf-suite/mockups/macos-redesign/**`. Screens: welcome with opening animation (first and short variants), main window light/dark, ⌘K palette, tool run inline with progress, page grid. Real motion timings matching the motion tokens. | **Owner approves the mockup before T1 starts** |
| T1 | Designer (lead reviews) | `AriadDesignSystem` package, asset catalog, AppIcon/AccentColor generation, lint check, previews, snapshot tests | Lead review |
| T2 | Designer (lead reviews) | Window shell rebuild (req. 3) | Lead review |
| T3 | Designer (lead reviews) | Tool discovery + inline run (req. 4) | Lead review |
| T4 | Designer (lead reviews) | Motion pass, opening animation, onboarding, What's New, smoothness traces (req. 5-7) | Lead review + owner look on the real app |

T1 runs first. T2 and T3 run in parallel after T1, since their files do not overlap. T4 runs last.

## Acceptance

- [x] The owner approves the T0 mockup (2026-11-14, [mockup](./mockups/macos-redesign/index.html)).
- [ ] There are zero literal spacing, frame, color and font values outside `AriadDesignSystem`, and the lint check is green.
- [ ] Snapshot tests pass for every component in light, dark and increased contrast. CI pins the runner OS.
- [ ] `WindowLayoutStabilityTests` and every existing UI test stay green. Min window width shows the document with the inspector closed.
- [ ] Animation Hitches traces show no hitch above 1 frame on: open document, toggle inspector, open ⌘K, run a tool, scroll 200 thumbnails. Traces are attached to the PR.
- [ ] Time to the first interactive window is unchanged by the opening animation (measured), and input during the animation works.
- [ ] Reduce Motion, Increase Contrast and Reduce Transparency each verified with screenshots.
- [ ] Light and dark screenshots in 3 locales, including `ar` RTL.
- [ ] The owner checks the result on the real app before the phase 18 UI tasks resume.

## Risks

- **Split-view layout crash recurrence.** Mitigation: no min-only frames in split columns, the stability test runs in every PR, and changes stay inside system containers.
- **macOS 26/27 API gates rest partly on secondary sources.** Verify against Apple docs in T1.
- **Repeated SwiftUI popovers reportedly crash on macOS 27.** Use sheets or the inspector instead.
