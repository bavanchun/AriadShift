# T0 mockup notes

`index.html` is the interactive mockup. It is one self-contained file (about 138 KB) with inline CSS and JS, no network access, and it opens by double-click. This note records the spec gaps I had to decide and how I verified the result.

## How to review

- The control strip at the top lists every screen:
  - Welcome: Play full, Play short, Drop a file (simulated drop: drag-over, then accept, then the morph).
  - Document, ⌘K palette, Tool run (fake compress through to the toast), Run error, Page grid.
  - Toggles: Dark, Reduce Motion, Slow-mo 4×.
- The first full opening plays on load.
- Real file drags from Finder also work, both on the welcome drop zone and anywhere on the document window. Hold ⌥ while dropping on the document window to insert pages.
- Keyboard:
  - ⌘K palette (↑ ↓ ↩ ⎋), ⌘F find (↩ and ⇧↩ step through matches), ⌘↩ Run, ⎋ cancels or closes.
  - ⌘1 / ⌘2 switch the view. Chrome may claim ⌘1/⌘2 for tab switching; Ctrl+1/2 and the toolbar view control also work.
  - ⌥⌘I opens DevTools in Chrome and cannot be intercepted, so use the ⓘ toolbar button for the inspector.

## Spec gaps and my decisions

1. **Window layout uses a floating sidebar.** The §3.3 ASCII shows a full-width toolbar above the sidebar. I used the macOS 26 `NavigationSplitView` form instead: a full-height floating glass sidebar inset 8 that holds the traffic lights and sidebar toggle, with the toolbar items over the content lane.
   - This keeps glass off glass, and it makes the concentric rule literal: window radius 24 = sidebar radius 16 (`panel`) + inset 8.
   - The inspector floats the same way below the 52 band.
   - Toolbar items are glass capsules over a scroll-edge blur rather than a solid band, so pages scroll softly under the toolbar.
2. **Spring timing inside the opening timeline.** A `smooth` spring settles in about 617 ms, but §3.2 gives windows of 160 to 600 ms. I kept every window exactly as specified and used the spring curve as the easing compressed into that window.
   - Interactive motion (panels, palette, toasts, reorder) uses each spring's natural settle time: snappy 333 ms, smooth 617 ms, emphasis 723 ms.
   - The solver is `makeSpring()`. It turns SwiftUI response and dampingFraction into a CSS `linear()` easing with a 0.1% settle threshold.
3. **Bead "0 → 1.12 → 1" under `emphasis`.** The emphasis spring alone overshoots only about 4%. I solve the launch velocity that makes the same spring peak at exactly 1.12 (`velocityForPeak`), so the bead is one physical spring rather than hand keyframes. The done check uses the same bounce.
4. **The squircle tile in the opening.** The spec animates only "the two glass sheets". The green tile would otherwise pop in at t=0, so it fades and scales from 0.94 together with the sheets (0–160 ms). The source ring travels with the source sheet.
5. **Stroke-dashoffset is the one non-transform/opacity animation.** §3.2 prescribes it for the thread draw. It runs on a single mask path, so all thread layers (shadow, glow, gold and specular) draw together. The specular head highlight follows the stroke head using transform keyframes sampled along the same path, with the same easing.
6. **"Any input finishes the animation."** Keydown, pointerdown and drop each take every running opening animation from its current value to its final value in one `snappy` step (about 333 ms). Under Reduce Motion the jump is instant.
7. **Frosted base on floating glass.** The §2 recipe (blur, saturate and a 4% tint) left palette text unreadable over the navy cover page, in both themes.
   - Floating chrome (palette, toasts, action bar, drop card, find pill) adds a `window` colour base at 72% in light and 60% in dark.
   - The sidebar and inspector keep the exact recipe.
   - On macOS 26, `glassEffect(.regular)` provides this adaptation natively. Reduce Transparency switches all glass to solid fills.
8. **Dark-mode glass edges.** A highlight of `rgba(255,255,255,.55)` glares on dark, so dark uses `.14`. The `rgba(0,0,0,.08)` rim is invisible on dark, so dark uses `.5`.
9. **On-accent text.** The spec has no token for it. I used white in light and `canvas` dark (`#121411`) on the light sage accent in dark.
10. **Welcome row labels.** "Suggested" and "Recent" sit in a 72-wide label column with a 16 gap. Five 104-wide cards with 8 gaps then fill the 640 column exactly: 72 + 16 + 5 × 104 + 4 × 8 = 640.
    - The header aligns to the column's leading edge, so there is one leading edge per column.
    - Each recent card is a 96-tall top crop of the page, 8 gap, name line 16, date line 16: 136 in total.
11. **Grid view toggle.** §3.6 mentions a toolbar grid toggle that the §3.3 toolbar does not list. I added a two-segment view capsule (continuous and grid) at the start of the centre group.
12. **Find results "under the field".** ⌘F expands the Tools capsule in place into a 240-wide find field, and siblings reflow with FLIP. A small glass pill directly below shows "n of N" with ↑↓.
    - Hits use a sage highlight, because gold is reserved for transformation.
13. **Drag reorder: "slide apart" plus the insertion bar.** The two neighbours of the insertion point slide 8 apart (snappy, within the 8pt movement rule), and the 3pt thread bar sits centred in the 24 gap.
    - The source cell dims to 35% while dragging. Dragging a multi-selection moves it as a group, with a count badge.
    - On drop, the lifted page flies into its new slot while other cells FLIP. Captions and sidebar thumbnails renumber.
14. **Inspector ⓘ without a tool** shows document info (kind, pages, size, security) with no footer.
15. **Mock limits.**
    - Every recent card and chip opens the same sample file, and chips skip the file picker.
    - Undo on compress restores the subtitle size. "Show in Finder" is inert.
    - Zoom re-lays out pages without animation, as a user-initiated layout change.
16. **Derived, non-token numbers.** These follow from token arithmetic:
    - 84 = toolbar 52 + 32; 44 = 52 − 8 inset; grid bottom 100 = 24 inset + 44 bar + 32.
    - The ring offset 4.5 = 2.5 stroke + 2 gap (concentric radius 8).
    - The segmented-control inset is 2, so its thumb radius 6 = 8 − 2.
17. **Sample content.** "Larkspur Labs, Inc." and every customer name and number are invented. Document colours (navy and sky) are the document's own and never UI tokens.

## Review round 1 changes (2026-11-14)

1. **Palette glass.** Floating chrome (palette, toasts, action bar, drop card and the find pill) now uses a frosted base of 88% light / 82% dark and a 40px blur. Over the blue cover it reads as one even sheet. This supersedes the 72/60% values in decision 7.
2. **Palette list.** The visible height snaps to whole rows (`snapPal`), so a cut never lands mid-row or directly after a group header.
   - A 24pt bottom scroll-edge fade shows whenever more rows sit below.
   - Keyboard scrolling keeps the selected row clear of the fade.
3. **Inspector Output section (Compress).**
   - Estimate: "≈ 1.1 MB · 74% smaller". It is computed from the current size × the Preset factor × the Images factor, and rolls with the numeric transition whenever Preset or Images changes.
   - Save as: an editable field holding `Q3-Board-Report-compressed.pdf`.
   - Replace original: a switch, off by default. The knob moves with `snappy`.
   - The run result, the toast and the title subtitle all use the estimate, so they agree.
   - The footer stays pinned.
4. **Errors never truncate.**
   - The message, "Page 14 couldn’t be recompressed. Nothing was changed.", wraps to exactly 2 lines.
   - Details sits under the message and expands below it. The footer grows upward, and its top edge glides with `smooth` (a FLIP on the footer).
   - The footer now sizes to its active layer only, so the normal Run footer stays at 60 and is not padded to the error's height.
5. **Grid and sidebar.** Entering the grid switches the sidebar to Tools with `smooth` (segment thumb and pane fade) and remembers the previous tab. Leaving the grid restores it. If the user picks a tab by hand while in the grid, that choice stands.
6. **Light window colours.** `canvas` is now `#ECEAE5`, and design-spec.md §2 is updated with a note.
   - Light sidebar and inspector use a neutral near-white frost (white @ 60%). I applied it to the inspector too, because it stacked the same warm tint.
   - Warmth remains only in `window`. Dark mode is unchanged.
7. **Dark selection ring.** The 2.5pt ring stays, and its `accentSoft` halo drops to 30% opacity in dark mode.
8. **No state leaks.** Every strip button resets its screen to a fresh document: palette, find, run, error and Details, toasts, drop overlay, inspector, grid, selection, sidebar tab, zoom, page order and file size. The canvas then crossfades in, and that button's own action plays from the clean state.

## Verification

I checked visuals in headless Chrome over CDP (screenshots at 1×/2×, real key and mouse events, computed-style checks). I did not view it on a physical display.

- Every §3 screen exists: welcome (full and short opening, drag-over, accept-and-morph), document light and dark, palette, tool run (progress, done flash, toast, error with Details), page grid.
- The palette keyboard path works: ↓↓ moves the shared highlight, typing "sum" fuzzy-filters and highlights matches, and ↩ flies the icon into the inspector.
- The fake compress runs to the toast "Compressed · 4.2 MB → 1.1 MB" and updates the subtitle to 1.1 MB.
- Drag reorder moved page 2 after page 7, and the grid, sidebar and page field all followed. Rotate, delete with Undo, and duplicate also work.
- Under Reduce Motion, only opacity animations run (checked with `document.getAnimations()`).
- At a 1280-wide viewport, `scrollWidth` equals `clientWidth` (1280), so there is no horizontal overflow.
- No layout shift: numbers use tabular digits with per-character rolls, and overlays are absolute or in grid stacks. One real layout bug was found and fixed: the grid's `sel` class collided with the inspector `.sel` wrapper and collapsed the cell height.
- No console exceptions occurred during the runs.
- Not verifiable here: true 120 Hz smoothness on the owner's display, and the Increase Contrast and Reduce Transparency media queries, which are implemented but not emulated.

## T1 design-system decisions (2026-11-14)

1. **Concentric corners without a second availability check.** `ConcentricRectangle` needs macOS 26, but `adaptiveGlass()` must stay the only `#available(macOS 26, *)` branch. Nested radii come from `DSRadius.concentric(outer:inset:)` (outer minus inset) on every OS, which gives the same geometry as the mockup.
2. **Glass fallback values.** Below macOS 26, panels draw `.regularMaterial`, then the neutral panel frost (white @ 60% in light, none in dark), then the 4% tint. Floating chrome uses the `window` frost at 88% light / 82% dark from review round 1. The 0.5pt highlight and rim match the mockup. On macOS 26 the native `glassEffect(.regular)` with the 4% tint replaces all of it, as decision 7 anticipated.
3. **Frost and glass edges are color tokens.** `glassFrostFloating`, `glassFrostPanel`, `glassHighlight`, `glassRim`, `glassTint`, `shadow`, `scrim` and `onAccent` are color sets like the spec colors, so the generator and the asset tests cover them. The increased-contrast glass tint doubles to 8%, per the spec.
4. **Type ramp on macOS text styles.** title2 = `.title2` semibold (17), headline = `.body` semibold (13), body = `.body` (13), callout = `.callout` (12), caption = `.subheadline` medium (11). macOS has no 11pt "caption" style, so `.subheadline` carries it.
5. **Display font.** The OFL Latin 700 WOFF2 from the pinned `@fontsource/plus-jakarta-sans` 5.3.0 is bundled unmodified with its licence and registered for the process only. CoreText reads WOFF2 directly, so there is no conversion step. The fallback is the bold rounded system font.
6. **Toast icon well.** The spec gives a 44pt capsule but no well size. A 28pt circle inset 8 is concentric with the capsule (22 − 8 = 14), so the leading padding stays on the grid.
7. **Palette list height.** The list snaps to whole rows under a 320pt cap. The mockup's 304 was one snapped result of the same rule, not a separate token.
8. **Fuzzy ranking.** Consecutive runs, word starts (including camelCase) and an early first match score higher, and each skipped character costs a point, so "cmp" ranks "Compress PDF" above "Crop margins of PDF". Keyword-only matches rank after every title match and highlight nothing.
9. **App icon.** The asset catalog uses the pre-macOS 26 template (100px margin and outer shadow) at every size, rendered from the brand geometry. A generated Icon Composer `.icon` for native Liquid Glass icons is left for later, because its file format is not documented for generation.
10. **Snapshots use the material fallback for glass.** The window server composites Liquid Glass from what lies behind the window, so offscreen capture returns an empty image and could never be a stable reference. The snapshot harness forces the material path (decision 2), which is what macOS 15 users see. The Liquid Glass look is reviewed in Xcode previews and in the running app.

## T2 window shell decisions (2026-11-15)

1. **No `ToolbarSpacer`.** It needs macOS 26, and a second availability branch would break the rule that `adaptiveGlass()` is the only one. The toolbar groups by placement instead (navigation: grid switch; principal: page capsule + zoom capsule; primary: find status, inspector), and macOS 26 already draws each placement as its own glass group.
2. **View shortcuts.** ⌘1…⌘n already switch the page display mode in the View menu, so the grid keeps ⌥⌘P instead of the spec's ⌘2.
3. **Grid action bar actions.** Rotate and Delete only, the page edits that exist today. Extract and Duplicate need new page tools, and they belong with the contextual toolbar (tool discovery).
4. **Grid hint.** The old "drag pages to reorder" line above the grid is now the action bar tooltip and the grid's accessibility hint, so the grid is pages only, as in the mockup.
5. **Selection ring without a selection.** In the Pages tab the page on screen carries the ring while nothing is selected, so the sidebar always shows where you are.
6. **Inspector content without a tool.** ⌥⌘I opens the inspector on the document facts (name, format, size, pages, …) in inspector sections; choosing a tool opens it for that tool.
7. **Drop.** Dropped PDFs open in their own windows and other supported formats are imported. With ⌥, dropped PDFs are inserted through Merge with the files already added, so the run is still confirmed in its parameters (never a silent edit).
8. **Tools tab.** The shell hosts a plain grouped tool list in the Tools tab; the tool browser (search, job groups, cards) replaces it there.
9. **Find.** The toolbar search field is the find field: ⌘F focuses it, Return and ⌘G step, Escape ends the search, and "4 of 12" with ↑↓ sits next to it. Searching leaves the page grid for the reader, where matches show.
