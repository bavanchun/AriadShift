# Worker 6 polish: desktop interaction polish

Branch `feat/v1-hardening--polish`, based on `origin/feat/v1-hardening` (`c91cc24`).

## Outcome

The desktop frontend has ten polish improvements, chosen from a public-source study of
Raycast, Arc and the Apple HIG. The study is in
[rea-261025-desktop-polish.md](rea-261025-desktop-polish.md). Everything is frontend-only,
with no Rust, capability or dependency changes. I wrote unit and integration tests but, as the
brief requires, did not run them, the typechecker or a build.

The `reverse-engineer-anything` (REA) skill is not installed on this machine. The study used web
search and page fetches instead. The Arc help centre returned HTTP 403, so its items are marked
`[snippet]` (taken from search excerpts) in the study.

## Commits

- `dc70def` docs: add the desktop interaction polish study
- `0043fe5` feat(desktop): add drag feedback, shortcuts and queue progress
- the commit that adds this report

## Files changed

- `apps/desktop/src/app.tsx`: wires in the improvements below. The format tables moved to
  `formats.ts`.
- `apps/desktop/src/formats.ts` (new): the format tables, `fileName`, and the supported-path
  filter used for drops.
- `apps/desktop/src/shortcuts.ts` (new): platform-aware shortcut matching and labels, and the
  `aria-keyshortcuts` values.
- `apps/desktop/src/queue.ts` (new): status counts, batch progress, newly finished jobs, and
  failures-first ordering.
- `apps/desktop/src/feedback.tsx` (new): the key hint, the status badge, the toast live region,
  and the completion toast builder.
- `apps/desktop/src/styles.css`: new styles, reduced-motion gating, and removal of the old
  `[data-status="succeeded"]` colour rule, which would have hidden text on the new badge.
- `apps/desktop/test/polish.test.tsx` (new): see the ownership note under Concerns.
- `plans/reports/rea-261025-desktop-polish.md` and this file.

## What changed for the user

1. **Drag and drop.** Dragging files over the window shows an overlay saying how many documents
   will be added and how many items will be skipped (from the Tauri `enter` paths), and the
   drop panel highlights. A drop adds only supported documents. Folders and other types are
   named in a warning that stays until dismissed. Before this, every dropped path was added and
   then failed inspection.
2. **Keyboard.** ⌘/Ctrl+1–6 switch tabs, ⌘/Ctrl+, opens Settings, ⌘/Ctrl+O chooses files and
   ⌘/Ctrl+↩ converts. ⌘/Ctrl+. cancels pending jobs, but only on the Queue tab. Esc dismisses the
   latest notice or toast. IME composition and Alt or Shift chords are ignored. Controls carry
   `aria-keyshortcuts`, nav rows and primary buttons show visual hints hidden from assistive
   technology, and Settings lists every shortcut.
3. **Queue.**
   - A live summary line: "2 converting · 5 waiting · 3 of 10 finished", then "All done · …".
   - A batch progress bar, counting jobs started since the queue was last idle.
   - Failed jobs listed first.
   - Status badges with an icon and text: Waiting, Converting, Converted, Failed, Cancelled.
   - A "Retry failed (n)" button and a Copy error button on each failed row.
   - "Cancel pending jobs" is disabled when nothing is pending.
4. **Messages.** Notices now have tones. Errors use `role="alert"` and stay until dismissed;
   warnings stay; confirmations ("Settings saved", pack results, deep-link hints) disappear
   after 6 s.
5. **Completion toasts.** A job that finishes while the user is on another tab raises a toast in
   a polite live region. A success offers Show output and disappears after 8 s. A failure offers
   Retry and View queue and stays. More than three at once collapse into one summary.
6. **Empty states.** Queue, Watch folders and Engine packs say why they are empty and offer the
   next step: Choose files, or Reload packs. None of them appears before the first data load.
   A failed pack listing now ends the loading state.
7. **Settings and Diagnostics.**
   - Save is enabled only when a setting has changed, with "Unsaved changes" and a "Discard
     changes" button.
   - Diagnostics shows a "Checking…" state and a summary such as "2 checks need attention, 1
     required".
   - Failing checks get distinct badge variants.
8. **Focus and motion.**
   - Switching tabs moves focus to the page heading, so screen readers announce the new tab.
     The heading shows no focus ring after a mouse click.
   - All transitions and animations run only under `prefers-reduced-motion: no-preference`, so
     the running spinner stands still when Reduce Motion is on. Status is always shown as text.
   - The drop-panel copy now lists PDF, Office and image inputs.

## Verify (coordinator)

```sh
pnpm install --frozen-lockfile
pnpm exec biome check apps/desktop
pnpm --filter @ariadshift/desktop typecheck
pnpm --filter @ariadshift/desktop test
just desktop-ci
# Optional manual check: just desktop-dev, drag a .md and a .zip onto the window, press
# Ctrl/Cmd+O, Ctrl/Cmd+Enter, Ctrl/Cmd+2, Esc; enable Reduce Motion and confirm the
# running badge is static.
```

Before committing, I ran only `pnpm exec biome check --write` on the touched files, which
formats and lints without compiling or testing. The result was clean: no errors or warnings.
`typos` was also clean on the new files.

## Concerns

- **File ownership.** `apps/desktop/test/polish.test.tsx` is outside the owned `apps/desktop/src/**`,
  but `vitest.config.ts` collects only `test/**`. Tests placed under `src/` would never run. It
  is a new file, so it cannot conflict with a sibling branch; the existing `test/app.test.tsx`
  is unchanged.
- **Tests not run.** The tests were written but not executed. These are the most likely places
  to need small fixes:
  - Base UI's disabled rendering. The helper accepts either `disabled` or `aria-disabled`.
  - Fake timers with `shouldAdvanceTime` in the two timer tests.
  - `role="region"` resolution for the labelled drop `section`.
- **Existing tests.** The existing tests' accessible-name queries (`/^Settings$/`, "Start
  conversion", "Choose files") still hold, because the hints are `aria-hidden` and the labels
  are unchanged.
- **Docs impact.** `docs/desktop.md` is not mine to edit. Its "Convert and queue" section
  should gain this sentence: "Dropping files adds supported documents and names skipped items.
  Keyboard: Cmd/Ctrl+1–6 tabs, Cmd/Ctrl+, Settings, Cmd/Ctrl+O choose files, Cmd/Ctrl+Enter
  convert, Cmd/Ctrl+. cancel pending jobs in Queue, Esc dismisses messages; Settings lists
  them. Toasts report results finished while another tab is open."
- **Deferred work.** These study recommendations need Rust, capabilities or plugins, and are
  listed in the study:
  - an unfocused-window system notification (`tauri-plugin-notification`);
  - a native menu bar that mirrors the shortcuts;
  - window-state restore (`tauri-plugin-window-state`);
  - a recoverable "Clear finished" (needs a host command);
  - Dock or taskbar progress.

Status: DONE_WITH_CONCERNS
