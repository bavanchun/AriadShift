# AriadShift for Mac: redesign spec

Owner: lead (design authority). Mockup builder: designer agent. Status: approved by the owner 2026-11-14.
Evidence: [audit + research](../../../reports/researcher-261114-1224-macos-ui-redesign.md). Owner decisions: [phase 18a](../../phase-18a-macos-design-system-and-motion.md).

## 1. Idea

**"The golden thread."** The brand mark is Ariadne's thread, an S that runs from a raw sheet to a finished one. The app tells the same story:
- The document is always the hero.
- Calm Apple glass chrome surrounds it.
- Gold appears only when something is *transforming*: opening, running a tool, progress, success.

Everything else is quiet. Three words guide every choice: **precise, calm, trustworthy**.

Rules:
1. **The document is content.** Glass appears only on chrome: sidebar, toolbar, inspector, palette and toasts. Pages, thumbnails and tables never sit on glass.
2. **One accent, one highlight.**
   - Sage is the interactive accent: selection, primary buttons, focus.
   - Gold ("thread") is reserved for transformation: progress, the opening animation, the success flash and recipe steps.
   - Never use both on the same control.
3. **Everything on the grid.** Spacing comes only from 4/8/12/16/24/32. Alignment uses one leading edge per column.
4. **Motion explains, never decorates.** Every animation shows where something came from or went. Only transform and opacity are animated, with three springs.
5. **Nothing blocks.** The opening animation, sheets and progress never stop input. Any click or key press finishes the animation instantly.

## 2. Tokens

### Color (light / dark)

| Token | Light | Dark | Use |
|---|---|---|---|
| `accent` | `#2F5B43` | `#A8CE94` | Selection, primary button, focus ring, links |
| `accentSoft` | `#E4EBDF` | `#26302A` | Selected row fill, drop zone tint |
| `thread` | `#D4940A` | `#F2C063` | Progress fill, brand moments, success flash |
| `threadSoft` | `#F6DFA4` | `#3B311C` | Progress track, toast icon well |
| `canvas` | `#ECEAE5` | `#121411` | Area behind pages (near-neutral, slightly darker than window) |
| `window` | `#F6F4EE` | `#171A16` | Window and welcome background |
| `surface` | `#FFFFFF` | `#1F231E` | Cards, palette rows hover base |
| `label` / `secondaryLabel` / `tertiaryLabel` | system | system | All text (keeps Apple legibility and contrast modes) |
| `separator` | `#D6D2C6` @ 60% | `#353C34` @ 80% | Hairlines |
| `danger` | `#B42318` | `#F97066` | Destructive only |
| `success` | `#1F6B3F` | `#A8CE94` | Success text and icon |

Review round 1 (2026-11-14) changed the light `canvas` from `#ECE8E0` to `#ECEAE5`. The warm canvas under a warm-tinted sidebar read khaki, so warmth now lives in `window` only, and light-mode panels (sidebar, inspector) use a neutral near-white frost.

Increased Contrast: separators at 100%, `accent` darkened to `#1E3A2A` in light and lightened to `#C6E3B6` in dark, and the glass tint opacity doubled.

### Glass

Glass is used for chrome only: macOS 26+ `glassEffect(.regular)` with a 4% tint of `#F4FFF2` in light and `#0B120D` in dark. Below 26 the fallback is `.regularMaterial`.

The mockup imitates glass with `backdrop-filter: blur(24px) saturate(160%)`, the tint, a 0.5px inner top highlight `rgba(255,255,255,.55)` and a 0.5px outer border `rgba(0,0,0,.08)`.

### Type

The ramp uses the SF system font throughout. Plus Jakarta Sans 700 (tracking -0.02em) is used for display text only.

| Style | Size / weight | Use |
|---|---|---|
| display | 28 / Jakarta 700 | Welcome title only |
| title2 | 17 / semibold | Inspector tool name, palette query |
| headline | 13 / semibold | Section headers, row titles |
| body | 13 / regular | Default |
| callout | 12 / regular | Secondary descriptions |
| caption | 11 / medium | Page numbers, badges, shortcuts |

Numbers that change, such as page counts, percentages and file sizes, use tabular (monospaced) digits.

### Spacing, size, radius, elevation

- **Spacing:** `xs 4`, `s 8`, `m 12`, `l 16`, `xl 24`, `xxl 32`. No other value exists.
- **Control heights:** small 24, regular 28, large 36, palette field 48. Toolbar height is 52.
- **Radius:** `chip 6`, `control 8`, `card 12`, `panel 16`. The window uses concentric corners: an inner radius equals the outer radius minus the padding.
- **Elevation:**
  - `e1` = `0 1px 2px rgba(26,24,20,.08)` for thumbnails.
  - `e2` = `0 4px 16px rgba(26,24,20,.10), 0 1px 3px rgba(26,24,20,.08)` for pages and cards.
  - `e3` = `0 24px 64px rgba(26,24,20,.22), 0 2px 8px rgba(26,24,20,.10)` for the palette and toasts.
  - Dark mode uses the same shadows at 2x opacity on black.

### Motion

| Token | SwiftUI | Use |
|---|---|---|
| `snappy` | `.spring(response: 0.25, dampingFraction: 0.9)` | Hover, press, toggles, selection, list reorder |
| `smooth` | `.spring(response: 0.42, dampingFraction: 1.0)` | Panels, inspector, palette, page transitions |
| `emphasis` | `.spring(response: 0.45, dampingFraction: 0.72)` | Success, the bead landing in the opening, drop accepted |
| `fade` | `.easeOut(duration: 0.18)` | Reduce Motion replacement for any movement |

Rules:
- Movement is at most 8pt for content that settles in. Scale stays between 0.96 and 1.0, except the opening.
- Stagger is 30ms per item, with at most 6 staggered items; later items appear together.
- Press feedback: scale 0.97 under `snappy`.
- Hover: the fill fades in over 120ms. Nothing moves.

## 3. Windows and screens

### 3.1 Welcome window (no document open)

The window is 880×560, centered, with a full-size content view (hidden title text, traffic lights only) and the `window` background.

```
┌──────────────────────────────────────────────────────────────────┐
│ ● ● ●                                                            │
│                                                                  │
│        [mark 56]  AriadShift (display 28)                        │
│        Every PDF tool, private on your Mac.  (callout, secondary)│
│                                                                  │
│   ┌──────────────────────────────────────────────────────────┐   │
│   │            [doc.badge.plus 32]                           │   │
│   │       Drop PDFs or documents here                         │   │
│   │       or  [ Open… ⌘O ]                                    │   │
│   └──────────────────────────────────────────────────────────┘   │
│                                                                  │
│   Suggested   (Merge PDFs) (Compress) (OCR a scan) (To Word)     │
│                                                                  │
│   Recent      [thumb][thumb][thumb][thumb][thumb]                │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

- Content column: 640 wide, centered, with 32 vertical rhythm between blocks.
- **Drop zone:** 640×168, radius `card`, `accentSoft` fill, a 1.5px dashed `accent` @ 40% border, a centered icon and two lines of text.
  - The primary button "Open…" is regular height and uses the accent fill.
  - **Drag-over state:** the border becomes solid `thread`, the fill gets a `threadSoft` overlay, the card scales to 1.01 under `snappy`, and the icon lifts 4pt.
  - **Drop accepted:** an `emphasis` pulse runs, then the window morphs into the document window (see 3.6).
- **Suggested chips:** height 28, radius 14, `surface` fill, `e1`, with an icon and label. Clicking one opens the file picker and then that tool.
- **Recent:** 5 cards of 104×136. Each has a page thumbnail with `e1` and radius 6, the name (caption, one line, middle-truncated) and the relative date in tertiary text. Hover lifts the card 2pt and raises it to `e2`.
- Footer: none. Settings live in the menu.

### 3.2 Opening animation (plays in the welcome window)

The window is visible and interactive at t=0. The animation is layered over the real content state.

**Full variant (~1200ms; first launch and after an update):**

| t (ms) | What happens |
|---|---|
| 0–160 | The mark is centered in the window at 112px. Its two glass sheets fade from 0 to 1 opacity and scale from 0.94 to 1 (`smooth`). The source sheet comes 6pt from the left and the target sheet 6pt from the right. |
| 120–720 | The gold thread draws the S from the source ring to the bead (path stroke `dashoffset` from 1 to 0, eased by `smooth`). A small specular highlight travels along the stroke head. |
| 640–860 | The bead lands with a scale of 0 → 1.12 → 1 (`emphasis`). A soft gold glow (radial, `thread` @ 35%, radius 1.6x) blooms and fades. |
| 800–1200 | The mark travels and scales from 112 to 56 into its header slot (shared-element move, `smooth`). The wordmark and tagline fade in and rise 8pt. The drop zone, suggested row and recent row follow with a 30ms stagger, each rising 8pt and fading in. |

**Short variant (~600ms; every other launch):** the sheets are already present, so the thread draws from 0 to 340ms. The bead lands from 300 to 420ms. The header move and content settle run from 360 to 600ms with a 20ms stagger.

**Reduce Motion:** the final state crossfades in over 180ms. There is no thread draw and no movement.

**Interruptibility:** any key press, click or file drop jumps every running animation to its final state within one `snappy` step.

**SwiftUI mapping:**
- `KeyframeAnimator` for the mark (sheet opacity and scale, thread trim, bead scale, glow).
- `matchedGeometryEffect` for the move from the center to the header.
- `PhaseAnimator` for the content stagger.
- The thread is a `Shape` built from the same geometry as `brand/src/geometry.mjs`, drawn with `.trim(from:to:)`.

### 3.3 Document window

The default size is 1280×820, with a minimum of 900×560 and a unified toolbar.

```
┌───────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●  [▤] Report-Q3.pdf          ‹ [ 3 / 20 ] ›   [– 100% +]   [⟳][⇲][Aa][✎] [⌕ Tools ⌘K] [ⓘ] │  52
│        20 pages · 4.2 MB                                                       │
├────────────┬──────────────────────────────────────────────────────────────────┤
│ Pages│Tools│                                                                  │
│ ────────── │            ┌────────────────────────┐                            │
│ [thumb 1]  │            │                        │                            │
│ [thumb 2]  │            │       page (e2)        │      canvas (warm)         │
│ [thumb 3]● │            │                        │                            │
│ ...        │            └────────────────────────┘                            │
│ glass 240  │                                                                  │
└────────────┴──────────────────────────────────────────────────────────────────┘
```

- **Toolbar:** a 52pt band with groups separated by `ToolbarSpacer`.
  - Leading: the sidebar toggle, then the title (headline) above a subtitle (caption, secondary: "20 pages · 4.2 MB").
  - Center: a page capsule (chevrons plus a 64-wide field "3 / 20", tabular), then a zoom capsule (– 100% +).
  - Trailing: a contextual group of the 4 most used actions for the current selection (Rotate, Extract, OCR, Annotate), a "Tools ⌘K" capsule button with a magnifier, and the inspector toggle.
  - Every toolbar control is 28 tall and every capsule has radius 14.
- **Sidebar:** 240 wide (min 200, max 320), glass.
  - A segmented control at the top: **Pages** / **Tools**, 28 tall, inset 12.
  - **Pages:** a single column of thumbnails, 136 wide, with `e1`, radius 6 and the page number caption below. The selected thumbnail has a 2.5pt `accent` ring with a concentric radius of 8 and an `accentSoft` halo. Thumbnails fade in over 180ms as they render, never popping. Selection moves with `snappy` and the list scrolls the selection into view with `smooth`.
  - **Tools:** a search field, then groups (Organize, Edit, Convert, Secure, OCR, AI). Each group has a headline row plus tools as 28-tall rows (icon 16 + title). A disabled (Pro or missing pack) tool shows a small badge, not a greyed row.
- **Canvas:** the `canvas` color. Pages are centered with 32 margins and `e2`, and page gaps are 24. Continuous scroll.
- **Inspector:** 320 wide (min 280, max 400), glass, closed by default. It opens with the `smooth` spring when a tool is chosen and closes with ⌥⌘I.
- **Find:** ⌘F expands the toolbar search capsule in place (`smooth`). Results show inline under the field ("4 of 12" with ↑↓). There is no separate find strip.
- **Drop anywhere:** dragging a file over the window dims the canvas with a 6% `accent` scrim and shows a centered glass card "Drop to open · ⌥ to insert pages" with a `snappy` scale-in.

### 3.4 Command palette (⌘K)

- An overlay centered horizontally, 640 wide, with its top at 18% of the window height. It uses glass, radius `panel` and `e3`, with a scrim of `rgba(0,0,0,.12)` behind it.
- Enter animation: opacity 0 → 1, scale 0.96 → 1 and 8pt up, all with `smooth`. Exit is the reverse at 70% duration.
- The query field is 48 tall with a 17pt title2 field, a magnifier on the leading side and an "esc" hint on the trailing side.
- Results are grouped as **Recent**, **Suggested for this file**, then each tool group. Each row is 40 tall:
  - a 28pt rounded-square icon well (radius 8), tinted `accentSoft` with an `accent` glyph, or `threadSoft` with a `thread` glyph for AI and recipes;
  - the title (body semibold) and a one-line outcome (callout, secondary);
  - the shortcut on the trailing side (caption, `tertiaryLabel`).
- The selected row has an `accentSoft` fill that moves between rows with a `snappy` shared highlight, not a re-render flash.
- Fuzzy matching highlights the matched characters in `accent` weight 600.
- Enter closes the palette. The row icon flies into the inspector header icon (shared element, `smooth`) while the inspector opens.

### 3.5 Tool run (inside the inspector)

```
┌ Inspector ─────────────────────────┐
│ [icon 28]  Compress PDF            │  title2
│ Make the file smaller, keep text.  │  callout secondary
│ ────────────────────────────────── │
│ QUALITY                            │  caption section header
│  Preset        [ Balanced     ▾ ]  │  label column 96, trailing-aligned
│  Images        [ 150 dpi      ▾ ]  │
│ PAGES                              │
│  Range         [ All          ▾ ]  │
│ ────────────────────────────────── │
│ [ Reset ]                [  Run  ] │  pinned footer, 16 inset
└────────────────────────────────────┘
```

- Sections use 16 padding and 12 row gap. Labels sit in a 96pt trailing-aligned column, and controls fill the rest, each 28 tall.
- **Running:** the footer morphs (`smooth`) into a progress row with:
  - a 4pt `threadSoft` track and a `thread` fill that animates by width under `smooth`, never jumping;
  - the percent as a tabular numeric content transition;
  - the step text "Optimizing images · page 7 of 20" in callout;
  - a Cancel (×) control.
- **Done:** the progress row flashes a `thread` → `success` check (`emphasis`, `symbolEffect` bounce). A toast then rises from the bottom center.
  - The toast is a glass capsule, 44 tall, with `e3`. It reads "Compressed · 4.2 MB → 1.1 MB" with the sizes in tabular digits, followed by **Show in Finder** and **Undo**.
  - It auto-dismisses after 6s, and the timer pauses on hover. The footer returns to Run.
- **Error:** the progress row turns into an inline message with a `danger` icon, one sentence and a "Details" disclosure. There are no modal alerts.

### 3.6 Page grid (organize view)

- The grid toggle in the toolbar (or ⌘2) switches from the continuous view with `smooth`. Each page flies from its scroll position to its grid cell (shared element), with at most 24 items animated and the rest faded.
- Adaptive columns have a minimum width of 168 and a maximum of 220, with a 24 gap and 32 margin. Each cell is the page (`e1`, radius 6) above a centered caption.
- **Selection:** a 2.5pt `accent` concentric ring. Multi-select uses ⌘ or ⇧.
- **Drag reorder:** the lifted cell scales to 1.04 with `e3`. Other cells slide apart (`snappy`). A 3pt `thread` insertion bar with round caps appears between cells.
- **Action bar:** a glass capsule at the bottom center that appears when the selection is non-empty. It offers Rotate, Delete, Extract and Duplicate, plus a "3 selected" count shown as a tabular content transition.

## 4. Accessibility and states

- **Focus ring:** a 3pt `accent` @ 50% ring outside the control, following its radius.
- **Reduce Motion:** every movement becomes `fade`. The shared-element flights become crossfades.
- **Reduce Transparency:** glass becomes a solid `window` or `surface` fill.
- **Increase Contrast:** see the color section above.
- **Right-to-left (`ar`):** the layout mirrors. The thread animation does not mirror, because the mark is a logo.
- **Keyboard:** ⌘K palette, ⌘O open, ⌘F find, ⌘1 / ⌘2 to switch the view, ⌥⌘I inspector, ⌘↩ Run, ⎋ cancel or close. Every toolbar action is also in the menus.
- **Empty and loading states:** a page placeholder uses the `surface` color with a 1.2s shimmer (opacity 0.6 ↔ 1). The shimmer is off under Reduce Motion.

## 5. Mockup (T0) requirements

- A single self-contained `index.html` in this folder: inline CSS and JS, no network, no CDN. It must open by double-click and stay under 2 MB.
- Inline the brand mark geometry from `brand/svg/ariadshift-app-icon.svg` and `brand/svg/ariadshift-wordmark.svg`. The thread must be a separate path so it can be drawn.
- Icons are hand-drawn inline SVGs in the SF Symbols style (1.5px stroke, rounded caps). Do not copy SF Symbols glyphs.
- Display font: Plus Jakarta Sans only via the outlined wordmark. Elsewhere use `-apple-system, BlinkMacSystemFont, "SF Pro Text", system-ui`.
- Implement springs with a small solver that generates CSS `linear()` easings (response and dampingFraction exactly as in the token table). Use the Web Animations API. Animate transform and opacity only.
- A top control strip, outside the fake window, lists the screens:
  - Welcome: Play full, Play short.
  - Document.
  - ⌘K palette.
  - Tool run: runs a fake compress through to the toast.
  - Page grid: drag to reorder.
  - Plus a Light/Dark toggle, a Reduce Motion toggle and a "slow-mo 4x" toggle for review.
- The fake macOS window has traffic lights and real proportions at a 1280×820 design size, scaled to fit the viewport.
- Every number in the mockup comes from this spec, with no invented spacing.
