# Lead review, round 1 (T0 mockup)

Verdict: CHANGES. The work is strong overall. The opening animation, welcome window, toolbar and toast are right, and every one of the 17 gap decisions in notes.md is accepted. Fix the items below, then report back.

1. **The palette glass is blotchy.** In light mode, over the blue cover page, the left half reads cream and the right half reads blue. It must look like one even frosted sheet.
   - Raise the frosted base for floating chrome to 88% light / 82% dark and the blur to 40px.
   - Apply the same values to toasts, the action bar and the drop card.
2. **The palette list cuts a row in half** ("Split PDF" is clipped mid-row).
   - Snap the visible height to whole rows plus the group header.
   - Add a 24pt scroll-edge fade at the bottom, like `scrollEdgeEffectStyle`, so overflow looks intentional.
3. **The inspector has a dead gap** between the form and the footer. Add an **Output** section, which is also a real UX gain:
   - An estimated result line, "≈ 1.1 MB · 74% smaller", that changes with Preset and Images through the tabular numeric roll.
   - "Save as" showing `Q3-Board-Report-compressed.pdf`, editable.
   - A "Replace original" switch, off by default.
   - The footer stays pinned.
4. **Errors are truncated** ("Page 14 couldn't be rec…"). Never truncate an error: wrap to at most 2 lines, and let Details expand below with `smooth`.
5. **Grid mode duplicates the page list.** When the grid is active, the sidebar switches itself to the Tools tab with `smooth`, and switches back to the remembered tab on leaving the grid.
6. **The light-mode document window looks khaki and muddy.** The sidebar glass and canvas stack two warm tints.
   - The light sidebar should read as a near-white frost, like Finder on macOS 26, using a neutral tint.
   - Change the canvas to `#ECEAE5`.
   - Keep the warmth in `window` only.
   - Update design-spec.md section 2 for the canvas value and note it.
7. **The selected thumbnail ring is heavy in dark mode** (ring plus a full halo). Keep the 2.5pt ring and cut the halo to 30% opacity.
8. **The mockup leaks state across screens.** The run error stayed visible after switching to Page grid. Each strip button must reset to a clean state for its screen.

Re-check with your headless-Chrome screenshots in both themes before reporting, then run `hc send t10k 'designer T0 r1 fixed: ...'`.
