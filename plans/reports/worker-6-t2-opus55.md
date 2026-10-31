# Worker 6-T2 (Opus 5.5 High): desktop OS integration

Branch `feat/v1-hardening`, worktree `01-AriadShift-wt-v1-hardening`. No builds or tests were run (per brief); only `rustfmt --check`, `plutil -lint`, `xmllint` and the generator's `--check` were run.

## Commits

| Hash | Subject |
|---|---|
| 9d8af11 | feat(desktop): accept files from OS launches and second instances |
| 9734cfc | feat(desktop): register Convert with AriadShift in Finder, Explorer and Linux file managers |
| d1c94d3 | docs(desktop): describe file manager integration |

Commits used explicit pathspecs; other workers' staged files (`crates/ariad-cli/src/render.rs`, `docs/cli.md`) were left staged and untouched.

## Files changed

- `apps/desktop/src-tauri/src/file_open.rs` (new): argument/URL parsing, pending list, `deliver`, `take`, 6 unit tests.
- `apps/desktop/src-tauri/src/lib.rs`: only the file-open entry points: `mod file_open`, single-instance callback now forwards `(args, cwd)`, setup parses the cold-launch argv, macOS `RunEvent::Opened` forwards Apple Event URLs.
- `apps/desktop/os-integration/formats.json` (generated, checked by a Rust test), `generate.mjs`, `generate.test.mjs`.
- macOS: `apps/desktop/src-tauri/Info.plist` (adds `CFBundleDocumentTypes`, rank `Alternate`, plus a Markdown `UTImportedTypeDeclarations`), `os-integration/macos/Convert with AriadShift.workflow/Contents/{Info.plist,document.wflow}`, `tauri.macos.conf.json` (`files` ships the workflow under `Contents/Resources/os-integration/`).
- Windows: `tauri.windows.conf.json` (new; NSIS `installMode: currentUser` + `installerHooks`, WiX `fragmentPaths` + `componentRefs`), `src-tauri/windows/os-integration.nsh` (POSTINSTALL writes / POSTUNINSTALL deletes the verbs, then `SHChangeNotify`), `src-tauri/windows/os-integration.wxs` (MSI component `ExplorerConvertVerbs`, HKCU keys, `ForceDeleteOnUninstall`).
- Linux: `tauri.linux.conf.json` (deb+rpm `desktopTemplate` and `files`), `os-integration/linux/main.desktop.hbs` (`Exec={{exec}} %U`, `MimeType=` deep-link scheme + document types, `Actions=convert`), `linux/dolphin/ariadshift-convert.desktop` (installed to `/usr/share/kio/servicemenus/` and `/usr/share/kservices5/ServiceMenus/`), `linux/nautilus/convert-with-ariadshift` (mode 755, installed to `/usr/share/ariadshift/nautilus/`; users copy it into `~/.local/share/nautilus/scripts/`, since Nautilus has no system scripts dir).
- `docs/desktop.md`: new "OS integration" section.

## What was done

- Registered formats = user-facing `from` formats of the embedded capability graph minus JSON (18 formats, 23 extensions incl. images). Rust test `os_integration_formats_match_the_capability_graph` fails if `formats.json` drifts from `ariad-core`; `generate.mjs` renders every platform file from it (UTI table fails generation for an unmapped format).
- Explorer verbs live under `SystemFileAssociations\.<ext>\shell\AriadShift.Convert` so the default app is never changed; Finder doc types use `LSHandlerRank=Alternate`.
- Launch handling: options (`-psn_…`, flags) skipped until `--`; `file://` URLs converted; other URLs (incl. `ariadshift://`, left to the deep-link plugin) ignored; relative paths resolved against the launching cwd; only existing regular files with a known extension and UTF-8 paths kept; deduped; capped at 1,000 (queue limit). Pending list survives until the window drains it, so cold-launch files are not lost.

## Required follow-up outside my files (coordinator)

The Rust side stores files and emits `open-files`; the window still needs to drain them. Exact change:

1. `apps/desktop/src-tauri/src/command_table.rs`: add `(opened_files, Empty, Vec<String>),`.
2. `apps/desktop/src-tauri/src/backend.rs` (impl Backend):
   ```rust
   pub fn opened_files(&self, _: Empty) -> Result<Vec<String>, String> {
       Ok(crate::file_open::take())
   }
   ```
3. `just desktop-bindings` (regenerates `src/bindings/*` and `permissions/desktop-commands.toml`).
4. `apps/desktop/src/app.tsx`, inside the existing listeners `useEffect` (the one with `onOpenUrl`), plus `import { listen } from "@tauri-apps/api/event";`:
   ```ts
   const opened = async () => {
     try {
       const files = await commands.opened_files({});
       if (files.length && !disposed) addPaths(files);
     } catch (e) {
       if (!disposed) setNotice(message(e));
     }
   };
   void listen("open-files", () => void opened())
     .then(keep)
     .catch((e) => setNotice(message(e)));
   void opened();
   ```
   `core:default` already grants event listening. Optional: a test in `apps/desktop/test/app.test.tsx` mocking `opened_files`.
5. `apps/desktop/package.json` `test:tools`: include `os-integration/*.test.mjs` (e.g. `node --test scripts/*.test.mjs os-integration/*.test.mjs`).
6. Optional UX: a Settings button "Install Finder Quick Action" that opens the bundled workflow with the opener plugin (needs a scoped `opener:allow-open-path`), so users need not browse the app bundle.

## Verify

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
cargo nextest run -p ariad-desktop file_open
node --test apps/desktop/os-integration/generate.test.mjs
node apps/desktop/os-integration/generate.mjs --check
pnpm exec biome check apps/desktop/os-integration apps/desktop/src-tauri/tauri.*.json   # biome was not installed here; apply --write if it reformats
just desktop-ci
just desktop-build
# macOS bundle
plutil -p target/release/bundle/macos/AriadShift.app/Contents/Info.plist | grep -A12 CFBundleDocumentTypes
ls "target/release/bundle/macos/AriadShift.app/Contents/Resources/os-integration/Convert with AriadShift.workflow/Contents"
# Linux bundle (CI)
dpkg-deb -c target/release/bundle/deb/*.deb | grep -E 'servicemenus|ServiceMenus|nautilus|applications'
dpkg-deb --fsys-tarfile target/release/bundle/deb/*.deb | tar -xO ./usr/share/applications/AriadShift.desktop | tee /dev/stderr | desktop-file-validate /dev/stdin || true
rpm -qlp target/release/bundle/rpm/*.rpm | grep -E 'servicemenus|nautilus'
# Windows (after installing the NSIS build)
reg query "HKCU\Software\Classes\SystemFileAssociations\.pdf\shell\AriadShift.Convert" /s
```

Manual acceptance on macOS: install the app from the DMG, open the bundled workflow once, right-click a `.md` in Finder → Quick Actions → Convert with AriadShift; with the frontend change applied the file appears in the Convert selection (cold launch and while running).

## Open concerns

- Frontend/command wiring above is outside my ownership, so the feature is not end to end until it lands.
- Tests and Biome were not run (no builds; `node_modules` absent). JSON/JS were hand-formatted to Biome's style (inline short arrays, expanded objects, 100 columns).
- Explorer with `MultiSelectModel=Player` and `"%1"` should launch one process per selected file (single instance merges them; Windows prompts above 15 items). Verify on Windows; switch to an `IExplorerCommand` handler only if that proves inadequate.
- The macOS Quick Action needs a one-time user install (macOS loads workflows only from `~/Library/Services`); "Open With" and Dock drops work without it. Check `codesign --verify --deep` once Apple signing is configured, since a `.workflow` bundle now sits in `Resources`.
- Images (png/jpeg/tiff/webp) are registered because they are capability inputs, but they need the optional Docling pack. Product call whether to keep them.
- The Dolphin/Nautilus entries call `ariad-desktop` (Cargo `default-run`, which Tauri uses as the main binary name since `mainBinaryName` is unset); the Node test pins this.
- The optional REA study of Raycast/Arc was not done.

Status: DONE_WITH_CONCERNS
