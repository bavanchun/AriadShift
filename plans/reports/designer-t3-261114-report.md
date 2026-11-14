# Designer T3 report: tool discovery and inline run

Branch `feat/macos-tool-discovery` in the worktree `01-AriadShift-p18a-t3`, on top of T1 `131587d9`. The commits are local and nothing is pushed.

## What I built

- **⌘K palette.** The T1 `DSCommandPalette` now runs over the real tool catalogue as an in-window overlay. The scrim is the `scrim` token, the panel sits 18% down the window, and it enters with `dsEnter` under `smooth` and exits in 70% of that time.
  - With an empty query it shows Recent, then Suggested for this file, then the six jobs, and each tool appears only once.
  - Once the user types, it fuzzy-searches every job by title and keywords (the words of the tool id, plus its registry group).
  - ↩ opens the chosen tool in the inspector, ⎋ or a click on the scrim closes the palette, and ⌘K toggles it.
  - ⌘K is a new Tools menu item, "Find a Tool…", so it works even when the toolbar is hidden.
- **Inline run.** The modal `ToolRunSheet` is gone, replaced by `ToolRunInspector` (a model adapter) over `ToolRunPanel` (a pure view).
  - The header has the icon well, title2 name and outcome, and a close button.
  - The settings are restyled on `DSInspectorSection`/`DSInspectorRow`, using the 96pt label column and 28pt controls.
  - The other sections are Additional documents (when a tool needs more files), Output ("Save to" folder and an "Open the result" switch), and Result for JSON tools.
  - The pinned footer morphs between Reset/Run (⌘↩), `DSProgressRow` running with Cancel (⎋), the done check (held for 1.4 s, then back to Reset/Run with the tool still open) and the error, which keeps the engine message under Details.
  - A toast offers Show in Finder and Undo. Undo deletes only the new files the run placed.
- **Tool browsing.**
  - `ToolBrowserView` is the content of the sidebar's Tools tab: a search field, then the six job groups (Organize, Edit, Convert, Secure, OCR, AI) with 28pt rows. AI tools get a gold icon, and tools that can't run show a badge (Later, Import, Pro or Off).
  - `ToolGallery` puts `DSToolCard`s ("Suggested for this file" and "Recent") in the inspector while no tool is open.
  - `ToolbarToolActions` is the contextual toolbar group: Rotate, Extract pages, OCR, Annotate and the "Tools ⌘K" capsule.
- **Model.**
  - `ToolDirectory` maps the registry groups onto the jobs and builds the palette and browser sections, the icons, tints and badges, and the suggestions from the real `InspectReport` facts.
  - `ToolRecents` keeps the last three tools that actually ran.
  - `ToolPresentation` holds each window's palette state and toast.
- **Rules that changed.**
  - Picking a tool replaces an open tool that isn't running (the inspector crossfades). A running tool keeps the inspector.
  - A tool that can't run still opens, with a note explaining why, and Run stays off.
  - Page edits from the viewer run inline the same way. They close again when done and leave a toast.
  - `DocumentSession.requestTool(id:)` queues a registry tool id until the engine catalogue is ready and then opens it through `begin`. An unknown id or a failed engine drops the request with a log line, and a running tool is never replaced. T4 adds the single `WelcomeToolHandoff.shared.take(for:)` call at window open; this was agreed with t149 and the lead.

## Files

- **New in Tools/:** `ToolDirectory`, `ToolRecents`, `ToolPresentation`, `ToolPaletteOverlay`, `ToolRunPanel`, `ToolRunInspector`, `ToolInspectorHost`, `ToolBrowserView`, `ToolbarToolActions`.
- **Changed in Tools/:** `ToolRunModel` (rerun, reset, dismiss error, `fraction`, `onFinish`), `ParamFormView`, `ParamRowsEditor`, and `ToolsMenu` (Find a Tool… ⌘K). `ToolRunSheet` is deleted.
- **Outside Tools/:**
  - `DocumentSession`: `tools`, `report`, `isRunning`, `closeTool()`, the new `begin` rules, and the toast/recents hookup on finish.
  - `Editor/Page/PageEditing.swift`: `run == nil` became `!isRunning`.
- **Design system (additive, approved by the lead):**
  - `@Entry public var dsPrefersMaterialGlass`, so app snapshots can force the material fallback.
  - An optional `badge` on `DSPaletteItem`, shown by `DSPaletteRow` with a new `testPaletteRowBadge` snapshot.
  - New file `Tokens/DSSize+Palette.swift` with `paletteTopFraction` (0.18).
  - New file `Tokens/DSMotion+Exit.swift` with `exitDurationFactor` (0.7), `doneHoldDuration` (1.4) and `exitAnimation(reduceMotion:)`.
  - No existing design-system snapshot changed.
- **Strings:**
  - 23 new `mac.*` keys in `locales/en.json` and `vi.json` (100%).
  - The sheet-only keys `mac.run.title`, `mac.run.open` and `mac.run.result_files` are removed from every locale, because the i18n test fails on unused keys.
  - `Localizable.xcstrings` is regenerated.
- **Lint:** `design-literals-baseline.txt` went from 48 to 35 entries (the sheet's and the old form's literals), with no new ones.
- **Tests:**
  - New: `ToolFixtures` (a fixed 12-tool catalogue through the real `ToolCatalog.build`), `ToolDirectoryTests` (10), and `ToolSnapshotTests`, a `DSSnapshotTestCase` subclass from the shared `AriadDesignSystemTesting` product (11 views × 4 appearances, 44 images).
  - `DocumentSessionTests`: the old "second pick keeps the run" test becomes four tests (replace when idle, keep when running, open but don't start an unavailable tool, and finish → toast + Recent + Undo).
  - `WindowRenderTests`: the Tools menu now starts with Find a Tool… ⌘K.
  - UI tests: `ToolsUITests` is rewritten for the inline flow and the palette, and `ViewerUITests` drops the sheet assertion.
- **project.yml:** T1 already links `AriadDesignSystemTesting` into AriadShiftTests. This branch only excludes `__Snapshots__` from the test bundle.
- **Notes:** `mockups/macos-redesign/notes.md` gets the section "Tool discovery and inline run decisions" (12 decisions).

## Shell hooks for T2 (all in `DocumentWindowView.swift`)

1. Remove `.sheet(item: $session.run) { ToolRunSheet… }`.
2. On the window content: `.toolRunPresentation(session: session, engine: engine)`. This adds the palette overlay and the toast.
3. In `.toolbar`: `ToolbarToolActions(session: session, engine: engine)`.
4. Inspector content: `ToolInspectorHost(session: session, engine: engine) { <document info> }`.
5. **Not wired yet:** the sidebar Tools tab should host `ToolBrowserView(directory: ToolDirectory(groups: engine.groups)) { session.begin($0, client: engine.client) }`. The current sidebar has no tabs, so I left this to T2.

When T2's `DSSidebarToolRow` and `DSSidebarGroupHeader` land, I'll swap them in for my private `ToolBrowserRow` and `ToolBrowserGroupHeader` on rebase. T2's row needs a tint parameter for the gold AI icons.

## Commands and results

All heavy steps ran through `.run/coord/build-lock.sh`. DerivedData is at `.cache/designer-t3-dd` and the SwiftPM cache at `.cache/swiftpm`. I didn't run cargo: the FFI xcframework, the generated bindings and the `ashift` helper are reused from the T1 worktree, which is the same Rust source.

| Command | Result |
|---|---|
| `swift test --package-path apps/macos/AriadDesignSystem` | 34 tests, 0 failures (includes the new badge snapshot) |
| `xcodebuild test -scheme AriadShift` (AriadShiftTests, full) | First run: 90 tests. Every failure was a first recording of the new snapshots; everything else passed. Final run without recording: **90 tests, 0 failures, TEST SUCCEEDED**. After rebasing onto the shared snapshot helper (`9cec40a3`), a run with `ARIAD_SNAPSHOTS=1` again gave 90 tests, 0 failures, and all 11 snapshot tests matched their references. The references are still renders from this Mac; they will be re-recorded from the CI runner artifact once the branch is pushed |
| `just macos-lint` | green, 35 baselined, assets up to date |
| `node --test scripts/i18n-xcstrings.test.mjs`, `node scripts/i18n-check.mjs`, `i18n-xcstrings.mjs --check` | 17/17 pass, 42 locales pass, catalogue up to date |

I did **not** run the UI tests (`AriadShiftUI` scheme). They need Automation Mode and an unlocked screen. They are updated but unverified.

## Screenshots

These are the snapshot references in `apps/macos/AriadShiftTests/__Snapshots__/ToolSnapshotTests/`, each in light, dark, light-contrast and dark-contrast:
- `testPaletteOverlay`, `testPaletteWithAQuery`
- `testToolBrowser`, `testGallery`, `testToolbarActions`
- `testRunPanelEditing`, `testRunPanelWithARequiredFieldError`, `testRunPanelRunning`, `testRunPanelDone`, `testRunPanelFailed`, `testRunPanelForAToolThatCannotRun`

The design-system badge row is `AriadDesignSystem/Tests/AriadDesignSystemTests/__Snapshots__/ComponentSnapshotTests/testPaletteRowBadge.*.png`.

## Decisions

The details are in notes.md. In short:
1. The jobs are mapped from the registry groups as the mockup groups them.
2. Recent and Suggested appear only for an empty query, so no tool shows twice.
3. Suggestions come from three real-fact rules: no text means OCR, 10 MB or more means Compress, and no title means Metadata.
4. Unavailable tools are listed with a badge and open with a note.
5. There is no "Replace original" switch and no size estimate. The engine never overwrites the original and has no estimate, so both would mislead.
6. The gallery cards sit in the inspector's no-tool state.
7. Annotate maps to `pdf.annotations`.

## Open questions and concerns

- **The icon flight is not done.** The palette overlay and the inspector are different hosting views, so `matchedGeometryEffect` can't cross between them. The tool crossfades into the inspector instead. A real flight needs the chosen row's frame from `DSCommandPalette` (a T1 component) and the header's frame in window coordinates. Do you want it in this branch or as a follow-up?
- **The DS palette doesn't sort groups by best score.** The mockup sorts groups by their best match while the user types, and `DSPaletteFilter` keeps section order. That is T1's component, so I left it unchanged.
- **The toast is shown even when the result also opens.** With "Open the result" on, Undo deletes a file that may already be open in another window. The window then shows a removed file and nothing crashes.
- **The UI tests are not run on this machine** (see above).

Status: DONE_WITH_CONCERNS
Summary: The ⌘K palette, the Tools tab content, the inspector gallery cards, the contextual toolbar group and the inline inspector run (progress, done, error, and a toast with Show in Finder and Undo) are built on the design system, with 44 new snapshots. All package and app unit tests, the lint and the i18n gates are green. Still open: the palette-to-inspector icon flight, the Tools-tab host in T2's sidebar, and a run of the UI tests.
