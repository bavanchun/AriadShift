# Desktop polish study: Raycast, Arc and the Apple HIG

A study of interaction polish in best-in-class desktop productivity apps, used to choose the
frontend improvements for AriadShift Desktop (`apps/desktop/src`). Every source was accessed on
2026-10-25. Only public sources were used: no accounts, no paywalls, no bypassed access
controls and no decompiled binaries.

The `reverse-engineer-anything` (REA) skill is not installed on this machine, so the study used
web search and page fetches instead. The browser-automation skills were not needed.

## Method and source quality

- **Apple HIG**: the developer.apple.com pages render client-side, so a plain fetch returns only the title.
  The text was read from Apple's public JSON for the same pages
  (`developer.apple.com/tutorials/data/design/human-interface-guidelines/<page>.json`).
  Quotes below are verbatim. Credibility: highest, because this is the platform authority.
- **Raycast**: manual.raycast.com, developers.raycast.com, raycast.com/blog and raycast.com/changelog were all
  fetched directly. Credibility: high, because these are first-party pages and current (the changelog's latest entry is v2.9, 2026-10-22).
- **Arc**: resources.arc.net returns HTTP 403 (a bot challenge) to both WebFetch and curl, and web.archive.org is
  blocked for this tool. I did not try to get past the challenge. Arc help-centre claims marked **[snippet]** were
  read from search-engine excerpts of the official article, not from the page itself. start.arc.net and arc.net
  were fetched directly. Credibility: medium. Arc has been in maintenance mode since 2025 (Josh Miller's
  "Letter to Arc Members 2025": no new features, only security and critical fixes), so treat it as a frozen
  design reference, not a moving one.
- **Extra references**: Linear and CleanShot X are included only where they add a pattern the first two lack.
  Tauri 2 and MDN are included for implementation fit only.

## Sources

| # | URL | What it showed |
|---|-----|----------------|
| R1 | https://manual.raycast.com/action-panel | ⌘K Action Panel; ↵ runs the primary action; shortcuts shown beside each action; Esc closes the panel |
| R2 | https://manual.raycast.com/keyboard-shortcuts | Full shortcut table: Esc goes back, ⌘W, ⌘, Settings, ⌘K, ⌘↑/↓ sections, ⌘Esc pops to root, ⌘1–⌘0 |
| R3 | https://manual.raycast.com/settings | Settings sidebar tabs, ⌘, and ⇧⌘, deep link, ⌘F search inside Settings |
| R4 | https://manual.raycast.com/command-aliases-and-hotkeys | Hotkey recorder, conflict warnings, hotkey shown next to the command |
| R5 | https://developers.raycast.com/api-reference/feedback/toast | Toast styles Animated/Success/Failure, primary/secondary actions, fallback to HUD when the window is closed |
| R6 | https://developers.raycast.com/api-reference/feedback/hud | The HUD is a compact message shown after the window closes |
| R7 | https://developers.raycast.com/information/best-practices | "not disrupt the user's flow"; a Toast for errors; `isLoading` |
| R8 | https://developers.raycast.com/api-reference/user-interface/list | List.EmptyView (title, description, icon, actions); Section subtitle; accessories; isLoading |
| R9 | https://developers.raycast.com/basics/prepare-an-extension-for-store | Title Case actions, "…" for submenus, empty-state flicker rule, preferences API |
| R10 | https://developers.raycast.com/api-reference/keyboard | Keyboard.Shortcut.Common table and the "consistent user experience" rule |
| R11 | https://developers.raycast.com/api-reference/user-interface/navigation | "When a user presses ESC, we automatically pop" |
| R12 | https://developers.raycast.com/api-reference/user-interface/form | Inline `error` prop, validate onBlur and clear onChange, drafts |
| R13 | https://raycast.com/blog/a-fresh-look-and-feel (2022-07-19) | Action bar consolidating actions, toasts and titles; shortcut hints; toasts in-window |
| R14 | https://www.raycast.com/changelog/macos | v2.0–v2.9 (2026-08-25 to 2026-10-22): recovery actions in errors, HUD progress, file drop, toast dismissal |
| A1 | https://start.arc.net/command-bar-actions | Command Bar (⌘T) action catalogue grouped into six sections |
| A2 | https://start.arc.net/master-multitasking | Command bar "do anything a mouse can do"; custom shortcuts in Preferences → Shortcuts |
| A3 | https://arc.net | ⌘/Ctrl+S hides the sidebar "when you need to focus" |
| A4 | https://resources.arc.net/hc/en-us/articles/19228855311127 **[snippet]** | Auto-archive of idle tabs (12 h default), View Archive → Restore |
| A5 | https://resources.arc.net/hc/en-us/articles/20595231349911-Keyboard-Shortcuts and /25622899860631 **[snippet]** | Shortcut table; remap in Settings → Shortcuts; ⌘, opens Settings |
| A6 | https://resources.arc.net/hc/en-us/articles/19235387524503-Little-Arc-Quick-Lookups-Instant-Triaging **[snippet]** | Little Arc triage window, ⌘⌥N, auto-archive every 6 h |
| A7 | https://tidbits.com/2025/06/20/dia-browser-debuts-with-contextual-ai-chat-but-arc-users-feel-left-behind/ | Secondary source: Arc development halted in 2025 |
| H1 | https://developer.apple.com/design/human-interface-guidelines/drag-and-drop | Drop-target highlighting, invalid-drop feedback, count badge, progress after a drop |
| H2 | https://developer.apple.com/design/human-interface-guidelines/progress-indicators | Determinate preferred, keep moving, consistent location, Cancel |
| H3 | https://developer.apple.com/design/human-interface-guidelines/feedback | Integrated status, confirm only important completions, explain failures |
| H4 | https://developer.apple.com/design/human-interface-guidelines/alerts | Error title wording, Esc and ⌘. cancel |
| H5 | https://developer.apple.com/design/human-interface-guidelines/keyboards | Standard shortcut table, custom-shortcut and modifier rules |
| H6 | https://developer.apple.com/design/human-interface-guidelines/the-menu-bar | App/File/Window/Help menus; disable, don't hide; app-specific menus |
| H7 | https://developer.apple.com/design/human-interface-guidelines/notifications | Foreground handling, no duplicates, errors use alerts, badge rules |
| H8 | https://developer.apple.com/design/human-interface-guidelines/motion and /accessibility | Purposeful, brief, optional motion; Reduce Motion substitutions |
| H9 | https://developer.apple.com/design/human-interface-guidelines/settings | ⌘, settings, few settings, task options in place, restore the last pane |
| H10 | https://developer.apple.com/design/human-interface-guidelines/launching, /undo-and-redo, /loading | Restore previous state, show undo results, show something fast |
| O1 | https://linear.app/changelog/2021-03-25-keyboard-shortcuts-help and https://linear.app/enablement/guide/navigating-linear | `?` searchable shortcut help, ⌘K command menu |
| O2 | https://cleanshot.com/features | Quick Access Overlay: post-task corner card with save/copy/drag-out and "Restore recently closed overlay" |
| T1 | https://v2.tauri.app/reference/javascript/api/namespacewebview/ | `DragDropEvent` = enter{paths,position} / over{position} / drop{paths,position} / leave |
| T2 | https://v2.tauri.app/reference/javascript/api/namespacewindow/ | `isFocused`, `onFocusChanged`, `requestUserAttention`, `setBadgeCount`, `ProgressBarStatus` |
| T3 | https://v2.tauri.app/plugin/notification/ , https://v2.tauri.app/plugin/window-state/ , https://v2.tauri.app/learn/window-menu/ , https://docs.rs/tauri/latest/tauri/menu/struct.PredefinedMenuItem.html | Notification permission flow; window size/position restore; native menus; predefined items (about, quit, minimize, close_window, …) in tauri 2.12.5 |
| M1 | https://developer.mozilla.org/en-US/docs/Web/CSS/@media/prefers-reduced-motion | `reduce` maps to macOS Accessibility → Reduce motion |

## Findings by area

### 1. Drag and drop affordances

- **HIG (H1)**: "Show people whether a destination can accept dragged content… highlight a containing view only when
  the destination can accept a dragged item, and show no visual feedback — or an explicit 'not allowed' image… when
  it can't." It also says "Display highlighting… only while the content is positioned above the destination,
  removing the visual feedback when people drag the content away." For drops that fail, it says
  "When people drop an item on an invalid destination, or when dropping fails, provide visual feedback."
  On multi-item drags, the macOS guidance is to "Consider displaying a badge during multi-item drag operations… If a destination can accept
  only a subset of dragged items, update the badge to show the new number." After a drop that starts work,
  it says "Provide feedback when dropped content initiates a task… show people that the task has begun." It also says
  "Offer alternative ways to accomplish drag-and-drop actions" (menu commands), and on macOS "Consider letting people drag
  content from your app into the Finder."
- **Raycast (R14)**: v2.1 (2026-09-01) added "dropping files from Finder directly into Quick AI to attach them",
  including while revealing the window with the global hotkey mid-drag. Earlier releases (v1.5.1, v1.22.0) let
  people drag files *out* of the window (seen in search excerpts of the changelog). Raycast documents no drag-hover visuals: **unverified**.
- **Arc**: Arc documents dragging tabs into Favorites and folders, but nothing about file drop targets. **Unverified**: no public source found.
- **CleanShot X (O2)**: after each capture, an overlay lets people "save, copy or drag & drop the files to other apps".
  This is the drag-out pattern applied to a finished task.
- **Fit (T1)**: Tauri's native drop already emits `enter` (with `paths`), `over` and `leave`. The committed handler in
  `apps/desktop/src/app.tsx` only reacts to `drop`, which is why there is no hover state. The `paths` on `enter` are
  enough to count the supported files before the drop happens.
- **How it applies**: highlight the drop panel only between `enter` and `leave`, label it with the result
  ("3 of 4 files can be converted"), show a "not supported" state when none qualify, and confirm that files were added after the drop.

### 2. Queue and progress design

- **HIG (H2)**: "When possible, use a determinate progress indicator… [it] can help people decide whether to do
  something else while waiting." Other rules from the same page:
  - "Keep progress indicators moving… If a process stalls… provide feedback that helps people understand the problem and what they can do."
  - "When possible, switch a progress bar from indeterminate to determinate."
  - "Don't switch from the circular style to the bar style."
  - "Avoid vague terms like loading… Display a progress indicator in a consistent location."
  - "If people can interrupt a process without causing negative side effects, include a Cancel button."
  - On macOS: "Prefer an activity indicator (spinner) to communicate the status of a background operation."
  - The HIG names "a file conversion" as its example of a determinate task.
- **HIG (H3)**: "Consider integrating status feedback into your interface… Mail… displays the number of unread messages in the
  toolbar." It also says "It's generally best to reserve [completion] confirmation for activities that are sufficiently important…
  they only need to know when it doesn't [succeed]."
- **Raycast (R8, R5, R14)**: lists group into `List.Section`, whose "subtitle [is] displayed next to the title of the section",
  which is how Raycast shows counts. Items carry accessories (a coloured tag, a date, an icon). `isLoading` "Indicates whether a loading bar should be shown…
  below the search bar". Long work uses `Toast.Style.Animated`, which is "shown until a process is completed" and then updated in place.
  v2.9 "Added support for pending + progress display in HUDs."
- **Arc (A4 [snippet], A6 [snippet])**: unpinned tabs auto-archive when idle (12 h by default) and Little Arc windows every
  6 h. They can be restored via ⌘T → View Archive → Restore. Arc clears finished items by aging them out to a
  recoverable archive rather than asking people to delete them.
- **How it applies**: give the Queue a summary line ("2 converting · 5 waiting · 12 done · 1 failed"), sections by status with failed
  first, a determinate bar only where the engine reports fractions (stage text otherwise), and a "Clear finished" action that
  keeps the list recoverable or undoable rather than deleting history.

### 3. Keyboard-first flows and shortcuts

- **HIG (H5)**: "Respect standard keyboard shortcuts." The standard table lists:
  - Command-Comma "Open the app's settings window", Command-Period "Cancel an operation", Command-O "Display a dialog for choosing a document to open".
  - Command-W "Close the active window", Command-Q "Quit the app", Command-Z "Undo", Command-M "Minimize".
  - Command-Question mark "Open the app's Help menu".

  The custom-shortcut rules are "Define custom keyboard shortcuts for only the most frequently used app-specific commands",
  "Prefer the Command key", "Avoid using the Control key as a modifier", and to list modifiers in the order "Control, Option, Shift, Command".
  The accessibility page adds "Let people use the keyboard alone to navigate and interact with your app" (Full Keyboard Access).
- **HIG (H4)**: alerts cancel with "Pressing Escape (Esc) or Command-Period (.)". (H9): "people often use the standard Command-Comma… to open an app's settings."
- **Raycast (R1, R2, R10, R11, R4)**:
  - The Action Panel ⌘K "lets you find and take any action… with just a few keystrokes". ↵ runs the primary action and ⌘↵ submits forms.
  - Actions "display their key combination on the right", and the panel search filters actions "in real-time using fuzzy matching".
  - Esc "Goes back to the previous view", and in the Action Panel "Closes the panel". ⌘, opens Settings, ⌘↑/⌘↓ jump between sections, ⌘Esc pops to root.
  - ⌘1–⌘0 jump to the first ten AI chats, and you "Hold ⌘… to see the numbers."
  - The Common shortcuts are New ⌘N, Open ⌘O, Refresh ⌘R, Remove ⌃X, RemoveAll ⌃⇧X, CopyPath ⌘⌃C and ToggleQuickLook ⌘Y, chosen
    "to keep a consistent user experience".
  - The hotkey recorder flags clashes "with a red warning".
- **Arc (A1, A2, A3, A5 [snippet])**:
  - The Command Bar (⌘T) can "do anything a mouse can do (maybe better?)". Its catalogue is grouped into Navigation, Organization, Tools,
    Split View, Other and Settings, and includes "Undo [Last Action]" and "Common Shortcuts".
  - ⌘S hides the sidebar, ⌘, opens Settings, and shortcuts are remappable in Settings → Shortcuts.
  - ⌃1… switches Spaces, per third-party guides only: **unverified** on Arc's own pages.
- **Linear (O1)**: ⌘K command menu, plus "press ? to bring up the help window", which was "made… searchable". Raycast and Arc lack an equivalent cheat-sheet surface.
- **How it applies**:
  - ⌘/Ctrl+, opens Settings, ⌘/Ctrl+O adds files, ⌘/Ctrl+↵ converts, Esc dismisses the notice or closes the palette, and ⌘/Ctrl+. cancels the selected running job.
  - ⌘/Ctrl+1…6 switch tabs (custom but conflict-free), and a ⌘/Ctrl+K palette lists every action with its shortcut.
  - On Windows and Linux, map ⌘ to Ctrl as Raycast for Windows does (R2).

### 4. Empty states

- **Raycast (R8, R9)**: `List.EmptyView` has a title, a "description for why the empty view is shown", an icon and **actions**
  (an ActionPanel), so an empty state can carry its next step. The rule is "the EmptyView is never displayed if the List's
  isLoading property is true", and the store guideline says not to render an empty list before data arrives, to avoid
  "a flickering empty state", and to show a loading indicator instead.
- **HIG (H10 loading)**: "Show something as soon as possible… they can interpret the lack of content as a problem."
  The HIG has no dedicated empty-state page: **unverified** as a named HIG topic.
- **Arc**: no public guidance found: **unverified**.
- **How it applies**: an empty Queue says "No conversions yet" with an "Add files…" (⌘O) button; an empty Watch folders tab
  offers "Add folder…"; and none of these states appears until the first IPC load has resolved.

### 5. Error recovery

- **Raycast (R7, R5, R12, R14)**:
  - "You should aim not to disrupt the user's flow just because something went wrong… show a Toast with information about the error."
  - Toast actions "provide a way to cancel an asynchronous operation, undo an action, or copy the stack trace of an error."
  - Form fields take an inline `error` that is "highlighted with red border and will show an error message", validated onBlur and cleared onChange.
  - v2.0: "Error messages are clearer, more specific, and show a potential action to recover, like retry". v2.5: failing
    requests "stop after a few attempts and show an error with options to change the model, resume, or stop". v2.7 says what startup is waiting for after 10 s.
- **HIG (H4, H3, H7)**: "describe what happened, the context in which it happened, and why. Avoid… 'Error' or 'Error 329347 occurred'".
  It also says "Avoid using an alert merely to provide information" and "Show people when a command can't be carried out and help them understand why".
  The notifications page says to "Use an alert — not a notification — to display an error message."
- **Arc**: "Undo [Last Action]" in the Command Bar (A1) and Archive → Restore (A4 [snippet]) are recovery by reversal. An Arc
  copy-URL toast is described only in user forum posts: **unverified**.
- **How it applies**: show a failed job's cause inline on its row in plain words ("Pandoc 3.12+ not found", not an exit code),
  with Retry, Copy error and an engine-specific fix ("Open Engine packs"); keep the global notice for app-level failures only, with an action and Esc to dismiss.

### 6. Motion and reduced motion

- **HIG (H8)**: "Add motion purposefully… Don't add motion for the sake of adding motion." It also says "Make motion optional… avoid
  using it as the only way to communicate important information" and "In apps, generally avoid adding motion to UI interactions that occur frequently".
  Two more rules: "Let people cancel motion" and "Aim for brevity and precision."
- **HIG (H8, accessibility)**: under Reduce Motion, apps should reduce "automatic and repetitive animations, including zooming, scaling, and peripheral
  motion", and should replace "transitions in x-, y-, and z-axes with fades".
- **MDN (M1)**: `prefers-reduced-motion: reduce` reflects the OS setting ("System Settings > Accessibility > … > Reduce motion"),
  and the page recommends removing, reducing or replacing non-essential motion.
- **Raycast**: the changelog tracks motion defects ("Reduced visible flickering", and the window "no longer shifts position after bounce
  animations", v2.4/v2.9), but no public Reduce Motion behaviour is documented: **unverified**.
- **Arc**: described as animation-rich only in third-party commentary; nothing official on reduced motion: **unverified**.
- **How it applies**: the running spinner is a "repetitive animation". Under `reduce`, replace it with a static icon plus text (the status badge already
  pairs icon and text), turn slide-in toasts and tab transitions into fades or instant changes, and never rely on motion to signal completion.

### 7. Native menus, window behaviour and notifications

- **HIG menus (H6)**: "Always show the same set of menu items… disable the action instead of hiding it" and "Support the keyboard
  shortcuts defined for the standard menu items". The app menu runs About, Settings…, Services, Hide, Hide Others, Show All, Quit.
  The page also says "Provide app-specific menus for custom commands… Even when commands are available elsewhere… list them in the menu bar"
  and "Provide a Window menu even if your app has only one window. Include the Minimize and Zoom menu items."
  File → Open Recent lists names, not paths. Use an ellipsis when more input is needed ("Open…").
- **HIG windows (H10 launching)**: "Restore the previous state when your app restarts… display windows in the same state and location."
- **HIG notifications (H7)**: "Handle notifications gracefully when your app is in the foreground… present the information in a way
  that's discoverable but not distracting." The page also says:
  - "Avoid sending multiple notifications for the same thing" and "Avoid including sensitive, personal, or confidential information" (file names are personal data).
  - "Avoid providing an action that merely opens your app."
  - "Use a badge only to show people how many unread notifications they have", so a dock badge counting queued jobs would go against the HIG.
- **Raycast (R5, R6)**: "showToast() will fallback to showHUD() if the Raycast window is closed". The HUD shows "a compact message at the bottom
  of the screen… to confirm that the action worked". v2.7 adds a success HUD when dictation finishes. Raycast picks the surface by window visibility.
- **CleanShot X (O2)**: a corner card appears "Right after taking a screenshot", with "Configurable Auto-close behavior" and "Restore recently closed overlay".
- **Arc (A6 [snippet])**: Little Arc is a small triage window for links opened from other apps, with an "Open In" action. Window-state behaviour is otherwise undocumented: **unverified**.
- **Fit (T2, T3)**: `isFocused()` and `onFocusChanged()` gate notifications. `PredefinedMenuItem` provides about, services, hide,
  hide_others, show_all, quit, minimize, maximize, close_window, fullscreen and the edit items. On macOS "the first submenu will be placed under
  the application's about menu". `setProgressBar` covers dock and taskbar progress (Indeterminate is treated as Normal on macOS and Linux).
  The notification and window-state plugins are new dependencies, which AGENTS.md requires to be registry-verified and checked against ARCHITECTURE.md.
- **How it applies**: send one system notification per finished batch, only when the window is unfocused ("12 files converted, 1 failed"),
  without file paths and with a "Show in Folder" action. While focused, update the Queue and show an in-app toast. Add a standard menu bar with
  File (Add Files… ⌘O, Convert ⌘↵), View (tabs ⌘1–6) and Window/Help menus, and restore the window size and position.

### 8. Settings layout

- **HIG (H9)**: "Minimize the number of settings you offer." The HIG adds:
  - "prefer letting people modify task-specific options without going to your settings area"
  - "Respect people's systemwide settings and avoid including redundant versions of them" (so theme and motion follow the OS)
  - "Restore the most recently viewed pane"
  - "Include a settings item in the App menu. Avoid adding settings buttons to a window's toolbar."
- **Raycast (R3, R9)**: Settings uses sidebar tabs (General, Extensions, Advanced, …) opened with ⌘,. ⇧⌘, jumps to the selected item's
  settings, and ⌘F searches inside Settings. Preferences are declared per extension instead of as separate "configure" commands.
- **Arc (A5 [snippet], A4 [snippet])**: Settings opens with ⌘, and includes a Shortcuts pane for remapping. Archive timing is set per profile.
- **How it applies**: keep output format and profile on Convert (task options); keep Settings for rare, app-wide choices
  (default output folder, archive and clear-after policy, notifications on/off); open it with ⌘/Ctrl+, and from the app menu;
  reopen the last-used section; and don't add an in-app theme or motion toggle that duplicates the OS.

## Ranked: the 10 most applicable patterns for AriadShift Desktop

Ranked by user-visible gain against cost, given the current UI (no hover feedback, flat queue, one notice line, no shortcuts).

1. **Live drop-target feedback with accepted-type counts.** Use the Tauri `enter`/`over`/`leave` events to highlight the panel
   only while hovering, show "N of M supported", show a "not supported" state, and confirm after the drop. Sources: H1, T1. It costs
   one listener branch and fixes the most visible gap.
2. **Queue summary plus status grouping.** Add a counts line and sections ordered failed → running → waiting → done; use determinate progress only when the engine
   reports fractions; keep progress in one consistent place. Sources: H2, H3, R8.
3. **Inline, actionable job errors.** Give each failed row a plain-language cause, then Retry, Copy error and a targeted fix action. Use toasts with an action for
   app-level errors and avoid "Error 123". Sources: R5, R7, R14 (v2.0, v2.5), H4.
4. **Standard and core shortcuts.** ⌘/Ctrl+, Settings, ⌘O Add files, ⌘↵ Convert, Esc dismiss, ⌘. Cancel, ⌘1–6 tabs.
   Do not repurpose standard keys. Sources: H5, H4, H9, R2, R10.
5. **Completion notification only when unfocused, one per batch.** Leave out file paths, offer "Show in Folder", and update in-app quietly while focused.
   Sources: H7, R5 (toast falls back to HUD when the window is closed), T2.
6. **Native menu bar listing every app command, with window-state restore.** Disable unavailable items rather than hiding them, provide a Window menu,
   and restore size and position on relaunch. Sources: H6, H10, T3.
7. **Visible shortcut hints.** Put `kbd` hints on buttons and tooltips and add a searchable `?` shortcut sheet, so people learn the keys without a manual.
   Sources: R1, R4, R13, O1.
8. **"Clear finished" as recoverable archiving.** Clear succeeded and cancelled jobs with one action (or let them age out), with Undo or restore,
   and never silently drop failed jobs. Sources: A4 [snippet], H10 undo, H3.
9. **Reduced-motion compliance.** Under `prefers-reduced-motion: reduce`, stop the spinner and use fades or instant
   changes; status must never depend on motion. Sources: H8, M1.
10. **Empty states that carry the next action and never flicker.** "No conversions yet" with Add files… (⌘O), and "No watch folders" with
    Add folder…; hide empty states until the first load resolves. Sources: R8, R9, H10 loading.

Below the cut line:
- **⌘K command palette.** It has high value once actions grow, but with six tabs and a few actions, items 4 and 7 deliver most
  of the discoverability. Sources: R1, A1, O1.
- **Drag finished output out to Finder.** Sources: H1 macOS, O2. Native drag-out support in Tauri 2 was not verified in this study.
- **Dock/taskbar progress via `setProgressBar`.** Source: T2. It is cheap, but the macOS and Linux states are limited.

## Unverified or not found

- Arc help-centre content (A4, A5, A6) was read through search-engine excerpts because direct fetches returned 403.
  The wording is likely correct but was not read on the page.
- Arc's default ⌘/⌃+number bindings, its copy-URL toast, its reduced-motion behaviour and its drop-target visuals: no official source found.
- Raycast drag-hover visuals, Raycast's response to the macOS Reduce Motion setting, and the key that expands a toast (the blog only says
  "a dedicated shortcut"): not documented publicly.
- The Raycast Quick AI file-drop entry appears under v2.1 (2026-09-01) on a direct fetch, but a search excerpt placed it in v2.2 (2026-09-08).
  The direct fetch is the one used here.
- The HIG has no standalone "empty states" topic. The empty-state guidance above comes from Raycast and HIG Loading.
- Per-job progress: `JobView` in `apps/desktop/src/bindings/types.ts` carries only `status` and
  `stage` text, with no fraction. The Queue therefore shows a determinate bar for the batch
  (finished jobs out of jobs started since the queue was last idle) and stage text per job.

## Applied in this change

All of these are frontend-only and live in `apps/desktop/src`; the tests are in
`apps/desktop/test/polish.test.tsx`.

| Pattern (rank) | What the app now does | Files |
|---|---|---|
| Live drop feedback (1) | A window-wide overlay shows how many dragged items will be added and how many skipped, from the Tauri `enter` paths; the drop panel highlights; a drop adds only supported documents and a persistent warning names the skipped items | `app.tsx`, `formats.ts`, `styles.css` |
| Queue summary and grouping (2) | A live summary line ("2 converting · 5 waiting · 3 of 10 finished", then "All done · …"), a batch progress bar, failed jobs listed first, icon-and-text status badges, "Retry failed (n)", and Cancel disabled when nothing is pending | `queue.ts`, `feedback.tsx`, `app.tsx` |
| Actionable errors (3) | Failed rows keep their cause and gain Copy error; notices are tiered: errors use `role="alert"` and stay, warnings stay, confirmations leave after 6 s | `app.tsx` |
| Standard shortcuts (4) | ⌘/Ctrl+1–6 tabs, ⌘/Ctrl+, Settings, ⌘/Ctrl+O choose files, ⌘/Ctrl+↩ convert, ⌘/Ctrl+. cancel pending jobs on Queue, Esc dismisses the latest message; IME composition, Alt and Shift chords are ignored | `shortcuts.ts`, `app.tsx` |
| Completion feedback (5, in-app half) | When a job finishes while the user is on another tab, a toast in a polite live region offers Show output (success) or Retry and View queue (failure); more than three at once collapse into one summary; successes leave after 8 s, failures stay | `feedback.tsx`, `app.tsx` |
| Visible shortcut hints (7) | `aria-keyshortcuts` on each control plus a visual hint (hidden from assistive technology) on nav rows and primary buttons, and a Keyboard shortcuts list in Settings | `feedback.tsx`, `app.tsx`, `styles.css` |
| Reduced motion (9) | All transitions and animations, including the running spinner, live under `prefers-reduced-motion: no-preference`; status is always carried by text | `styles.css` |
| Empty states (10) | Queue, Watch folders and Engine packs explain the state and offer the next step (Choose files ⌘O, Reload packs); none shows before the first snapshot or pack listing resolves | `app.tsx` |
| Settings (H9) | Save is enabled only when something changed, with Unsaved changes and Discard changes; Diagnostics summarizes how many checks need attention | `app.tsx` |
| Focus on navigation | Switching tabs moves focus to the page heading, so screen readers announce the new tab; the heading shows no ring after a pointer click | `app.tsx`, `styles.css` |

## Deferred: needs Rust, capabilities or new dependencies

These are outside this frontend-only change. Each needs `apps/desktop/src-tauri` work and, for
plugins, a registry-verified version and an ARCHITECTURE.md check.

- **System notification when the window is unfocused (rank 5).** It needs
  `tauri-plugin-notification` and a capability. Send one per batch, without file paths, and only
  when `isFocused()` is false; the in-app toast stays for the focused case.
- **Native menu bar (rank 6).** App menu with Settings… (⌘,), File with Add Files… (⌘O) and
  Convert (⌘↩), View with the six tabs, and Window and Help menus, emitting events the frontend
  already maps to the same actions. Disable items rather than hide them.
- **Window size and position restore (rank 6).** `tauri-plugin-window-state`.
- **Clear finished, recoverably (rank 8).** The queue has no command to remove finished history;
  it needs a host command plus Undo in the UI.
- **Dock or taskbar progress.** `Window.setProgressBar` needs a capability grant; do not use the
  Dock badge for queued jobs (H7).
- **⌘K command palette and dragging outputs to Finder.** Below the cut for now (see above).
