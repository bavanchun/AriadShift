# Worker Report: Installation and First-Run Guide

- **Branch:** `feat/v1-distribution--install`
- **Worktree:** `/home/vchun/Codes/01-AriadShift-wt-p8-install`

## Files Changed

- `docs/install.md` (new): Complete installation, verification, first-run, prerequisites, engine-pack, and self-hosting guide across all distribution channels.
- `docs/desktop.md` (modified): Updated the `## Install and run` section with exact desktop installer asset names, `SHA256SUMS` and detached `.minisig` verification commands against project public key `D87B0E33F55EA02B`, unsigned Gatekeeper (`System Settings › Privacy & Security › Open Anyway` and `xattr -dr com.apple.quarantine` trade-offs) and Windows SmartScreen (`More info › Run anyway`) steps, and cross-references to `docs/install.md`.

## Commits

- `b5d7166` `docs: add installation and first-run verification guide`
- `4fab5bc` `docs(desktop): document unsigned first-run and signature checks`

## What Was Done

1. **CLI channels (`docs/install.md` §1):**
   - Documented the shell installer (`ariad-cli-installer.sh`) and PowerShell installer (`ariad-cli-installer.ps1`).
   - Documented the Homebrew tap (`brew install bavanchun/tap/ashift`), its runtime `pandoc` dependency, Intel macOS source-build caveat, and fallback to the release-attached `ashift.rb` asset when `HOMEBREW_TAP_TOKEN` is unset.
   - Listed all five target triples and exact archive and `.sha256` sidecar names from `dist-workspace.toml` and `.github/workflows/release.yml` (`ariad-cli-x86_64-unknown-linux-musl.tar.xz`, `ariad-cli-aarch64-unknown-linux-musl.tar.xz`, `ariad-cli-aarch64-apple-darwin.tar.xz`, `ariad-cli-x86_64-apple-darwin.tar.xz`, and `ariad-cli-x86_64-pc-windows-msvc.zip`), with Unix and PowerShell SHA-256 verification and extraction steps.
   - Documented optional crates.io installation (`cargo binstall ariad-cli`, `cargo install --locked ariad-cli`), direct git tag builds (`cargo install --locked --git ... --tag v1.0.0 ariad-cli`), and WinGet (`winget install VChun.AriadShift`).
2. **Checksum and Minisign verification (`docs/install.md` §2):**
   - Documented the compiled-in project Minisign public key from `crates/ariad-host/data/pack-keys/d87b0e33f55ea02b.pub` (`crates/ariad-host/src/packs/signature.rs`): key ID `D87B0E33F55EA02B`, public key `RWQroF71Mw572E0d5K9ndPm5/Uf94TRU99qOHfm3ZCNmB7CebJkjmim/`.
   - Provided exact `minisign -Vm` commands for `SHA256SUMS` and individual desktop installer `.minisig` signatures, plus `sha256sum -c`, `shasum -a 256 -c`, and PowerShell `Get-FileHash` checks.
   - Clarified the distinction between detached `.minisig` release signatures (`D87B0E33F55EA02B`) and `.sig` Tauri in-app updater signatures (`CDEBF0BE86A5D2A5` in `apps/desktop/src-tauri/tauri.conf.json`).
3. **Desktop installation and unsigned first-run guide (`docs/install.md` §3 and `docs/desktop.md`):**
   - Listed exact Tauri bundle asset names (`AriadShift_<version>_aarch64.dmg`, `AriadShift_<version>_x64-setup.exe`, `AriadShift_<version>_x64_en-US.msi`, `AriadShift_<version>_amd64.deb`, `AriadShift-<version>-1.x86_64.rpm`, `AriadShift_<version>_amd64.AppImage`).
   - Documented macOS Gatekeeper first launch through **System Settings › Privacy & Security › Open Anyway** and the advanced `xattr -dr com.apple.quarantine /Applications/AriadShift.app` option with its XProtect/quarantine bypass trade-off.
   - Documented Windows Microsoft Defender SmartScreen (**More info › Run anyway**) and Linux `.deb`, `.rpm`, and `.AppImage` installation (including `desktop-file-utils`, `xdg-utils`, and `APPIMAGE_EXTRACT_AND_RUN=1`).
4. **Prerequisites, engine packs, and self-hosting (`docs/install.md` §4–§7):**
   - Covered built-in routes (Markdown, HTML, IR JSON, DoclingDocument JSON, digital PDF reader, Typst PDF writer), Pandoc `>= 3.12, < 4` (`ASHIFT_PANDOC`), and LibreOffice `>= 24.2` (`ASHIFT_SOFFICE` and Linux user/mount namespace requirements).
   - Covered `ashift engines install docling`, offline `--from`, `verify`, `remove`, Tesseract 5 system installation and Linux sandbox prefix rules, Windows Visual C++ Redistributable x64 (`vc_redist.x64.exe`), and `ashift doctor`.
   - Summarized Docker Compose self-hosting (`infra/compose/docker-compose.yml` profiles `core`, `office`, `docling`) and multi-arch GHCR images (`ghcr.io/bavanchun/ariadshift-*`), linking `docs/server.md` and referencing `docs/self-hosting.md`.

## Verification Commands for Coordinator

```sh
just spell
just lint-commits origin/dev..HEAD
```

## Open Concerns

- **Cross-links from root docs:** `README.md` and `AGENTS.md` are owned by sibling task T4; T4 should link `docs/install.md` from `README.md` ("Installation" and "Project documents") and `AGENTS.md` ("Current state").
- **`docs/self-hosting.md` merge order:** `docs/self-hosting.md` lands on `dev` from the cloud-foundation branch (`feat/cloud-foundation`); `docs/install.md` references `docs/self-hosting.md` and links `docs/server.md` so documentation remains valid both before and after that merge.

Status: DONE
