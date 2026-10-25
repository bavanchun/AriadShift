# Installation and First-Run Guide

How to download, verify, install, and configure AriadShift across every distribution channel: the `ashift` CLI, AriadShift Desktop, on-demand engine packs (`docling`), external format prerequisites (Pandoc, LibreOffice, Tesseract), and self-hosted containers.

---

## Channels Overview

| Surface | Channel | Platforms | Notes |
|---|---|---|---|
| **CLI (`ashift`)** | [GitHub Releases](https://github.com/bavanchun/AriadShift/releases) (archives & installer scripts) | Linux (`x86_64`, `aarch64` static musl), macOS (`aarch64`, `x86_64`), Windows (`x86_64` static CRT) | Primary channel; includes `.sha256` sidecars and CycloneDX SBOMs |
| **CLI (`ashift`)** | Homebrew tap (`bavanchun/tap/ashift`) | macOS (`aarch64`, `x86_64`), Linux (`x86_64`, `aarch64`) | Formula `ashift.rb` generated on every release; installs `pandoc` as a runtime dependency |
| **CLI (`ashift`)** | crates.io (`ariad-cli`) / `cargo-binstall` / git source | All Rust tier-1 platforms | Optional channel; `cargo install` from git works even when crates.io publication is skipped |
| **CLI (`ashift`)** | WinGet (`VChun.AriadShift`) | Windows (`x86_64`) | Portable `.zip` manifest with `ashift` command alias |
| **Desktop (`AriadShift`)** | [GitHub Releases](https://github.com/bavanchun/AriadShift/releases) (`.dmg`, `-setup.exe`, `.msi`, `.deb`, `.rpm`, `.AppImage`) | macOS (`aarch64`), Windows (`x86_64`), Linux (`x86_64`) | v1 ships **unsigned** builds verified via `SHA256SUMS` and `.minisig` signatures |
| **Engine packs** | `ashift engines install docling` (or Desktop **Engine packs** tab) | Linux (`x86_64`, `aarch64` glibc), macOS (`aarch64`), Windows (`x86_64`) | Signed with Minisign (`D87B0E33F55EA02B`); supports offline `--from <dir>` installation |
| **Self-hosted cloud** | Docker Compose & GHCR images (`ghcr.io/bavanchun/ariadshift-*`) | Linux (`amd64`, `arm64`) | Multi-arch OCI images with SBOMs and keyless Cosign signatures |

---

## 1. CLI Installation (`ashift`)

### Shell installer (Linux & macOS)

Downloads the prebuilt archive matching your OS and CPU architecture, verifies its SHA-256 digest, and installs `ashift` into `~/.cargo/bin` (or `$CARGO_HOME/bin`):

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/bavanchun/AriadShift/releases/latest/download/ariad-cli-installer.sh | sh
```

### PowerShell installer (Windows)

Downloads the `x86_64-pc-windows-msvc` release archive, verifies its SHA-256 digest, and adds `ashift.exe` to your user `Path`:

```powershell
irm https://github.com/bavanchun/AriadShift/releases/latest/download/ariad-cli-installer.ps1 | iex
```

### Homebrew tap (macOS & Linux)

```sh
brew install bavanchun/tap/ashift
```

The formula installs `ashift` and declares `pandoc` as a runtime dependency.

- **Intel macOS note (`x86_64-apple-darwin`):** Homebrew may build `pandoc` from source on Intel macOS. To avoid a long source build, install the official Pandoc `.pkg` installer first or use the standalone CLI archive with `ASHIFT_PANDOC`.
- **Direct formula asset:** Each GitHub Release also attaches the generated `ashift.rb` formula directly. When the automated tap push is skipped (if `HOMEBREW_TAP_TOKEN` is not configured), you can download `ashift.rb` from the release assets and install it locally.

### Standalone GitHub Release archives

Every release (`vX.Y.Z`) publishes self-contained archives and `.sha256` checksum sidecars for five targets:

| Platform | Target triple | Release archive | Checksum file | Executable path in archive |
|---|---|---|---|---|
| Linux x86_64 (static musl) | `x86_64-unknown-linux-musl` | `ariad-cli-x86_64-unknown-linux-musl.tar.xz` | `ariad-cli-x86_64-unknown-linux-musl.tar.xz.sha256` | `ariad-cli-x86_64-unknown-linux-musl/ashift` |
| Linux ARM64 (static musl) | `aarch64-unknown-linux-musl` | `ariad-cli-aarch64-unknown-linux-musl.tar.xz` | `ariad-cli-aarch64-unknown-linux-musl.tar.xz.sha256` | `ariad-cli-aarch64-unknown-linux-musl/ashift` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `ariad-cli-aarch64-apple-darwin.tar.xz` | `ariad-cli-aarch64-apple-darwin.tar.xz.sha256` | `ariad-cli-aarch64-apple-darwin/ashift` |
| macOS Intel | `x86_64-apple-darwin` | `ariad-cli-x86_64-apple-darwin.tar.xz` | `ariad-cli-x86_64-apple-darwin.tar.xz.sha256` | `ariad-cli-x86_64-apple-darwin/ashift` |
| Windows x86_64 (static CRT) | `x86_64-pc-windows-msvc` | `ariad-cli-x86_64-pc-windows-msvc.zip` | `ariad-cli-x86_64-pc-windows-msvc.zip.sha256` | `ashift.exe` |

Each archive also includes `LICENSE`, `NOTICE`, and `THIRD_PARTY_LICENSES`.

#### Download, verify, and extract (Linux & macOS)

Set `TARGET` to your platform triple from the table above (for example `x86_64-unknown-linux-musl` or `aarch64-apple-darwin`):

```sh
TARGET="x86_64-unknown-linux-musl"
ARCHIVE="ariad-cli-${TARGET}.tar.xz"
BASE_URL="https://github.com/bavanchun/AriadShift/releases/latest/download"

curl --proto '=https' --tlsv1.2 -LsSf -O "${BASE_URL}/${ARCHIVE}"
curl --proto '=https' --tlsv1.2 -LsSf -O "${BASE_URL}/${ARCHIVE}.sha256"

# Verify SHA-256 checksum (use shasum -a 256 -c on macOS, or sha256sum -c on Linux)
shasum -a 256 -c "${ARCHIVE}.sha256"

# Extract and install into ~/.local/bin
mkdir -p ~/.local/bin
tar -xJf "${ARCHIVE}" --strip-components=1 -C ~/.local/bin "ariad-cli-${TARGET}/ashift"
```

#### Download, verify, and extract (Windows PowerShell)

```powershell
$archive = "ariad-cli-x86_64-pc-windows-msvc.zip"
$baseUrl = "https://github.com/bavanchun/AriadShift/releases/latest/download"

Invoke-RestMethod -Uri "$baseUrl/$archive" -OutFile $archive
Invoke-RestMethod -Uri "$baseUrl/$archive.sha256" -OutFile "$archive.sha256"

$expected = (Get-Content "$archive.sha256").Split(' ')[0].ToLower()
$actual = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $expected) {
  throw "SHA-256 mismatch: expected $expected, got $actual"
}

Expand-Archive -Path $archive -DestinationPath "$env:LOCALAPPDATA\Programs\ashift" -Force
```

### Cargo / crates.io (optional) and WinGet

Publishing `ariad-core`, `ariad-host`, and `ariad-cli` to crates.io is optional in the release workflow (skipped with a workflow notice when `CARGO_REGISTRY_TOKEN` is not set).

When the crates are published on crates.io:

```sh
# Install prebuilt binary from GitHub Releases via cargo-binstall
cargo binstall ariad-cli

# Or compile from crates.io source (installs the `ashift` binary)
cargo install --locked ariad-cli
```

To build and install directly from a release tag without crates.io:

```sh
cargo install --locked --git https://github.com/bavanchun/AriadShift --tag v1.0.0 ariad-cli
```

On Windows, once the release's WinGet manifest (`scripts/winget-manifest.sh`) is merged into `microsoft/winget-pkgs`:

```powershell
winget install VChun.AriadShift
```

---

## 2. Checksum and Minisign Verification

AriadShift v1 distributes **unsigned** desktop builds (without Apple Developer ID or Windows Authenticode certificates). To let you verify integrity and provenance before launching an installer or unpacking an engine pack, release artifacts ship with SHA-256 checksums and detached [Minisign](https://jedisct1.github.io/minisign/) signatures (`.minisig`).

### Project Minisign public key

The project's release and engine-pack signing public key is compiled into `ariad-host` from [`crates/ariad-host/data/pack-keys/d87b0e33f55ea02b.pub`](../crates/ariad-host/data/pack-keys/d87b0e33f55ea02b.pub) (loaded by [`crates/ariad-host/src/packs/signature.rs`](../crates/ariad-host/src/packs/signature.rs)):

- **Key ID:** `D87B0E33F55EA02B`
- **Public key (`-P` base64 string):** `RWQroF71Mw572E0d5K9ndPm5/Uf94TRU99qOHfm3ZCNmB7CebJkjmim/`

Full `d87b0e33f55ea02b.pub` file contents:

```text
untrusted comment: minisign public key D87B0E33F55EA02B
RWQroF71Mw572E0d5K9ndPm5/Uf94TRU99qOHfm3ZCNmB7CebJkjmim/
```

You can also inspect the key IDs compiled into your installed `ashift` binary at any time by running `ashift doctor`.

### Verifying desktop installers (`SHA256SUMS` and `.minisig`)

Each desktop release attaches:

- `SHA256SUMS` — GNU coreutils SHA-256 manifest covering every desktop installer, updater archive, `.sig` updater signature, and `latest.json`.
- `SHA256SUMS.minisig` — Detached Minisign signature over `SHA256SUMS`.
- `<asset>.minisig` — Detached Minisign signature over each desktop installer and updater artifact (for example `AriadShift_1.0.0_aarch64.dmg.minisig` or `AriadShift_1.0.0_x64-setup.exe.minisig`).

1. **Install `minisign`:**
   - macOS: `brew install minisign`
   - Ubuntu / Debian: `sudo apt install minisign`
   - Fedora: `sudo dnf install minisign`
   - Windows: `winget install Jedisct1.Minisign` (or download from the official Minisign releases)

2. **Verify the Minisign signature with `minisign -Vm`:**
   Download your platform installer together with its `.minisig` file (or `SHA256SUMS` and `SHA256SUMS.minisig`), then verify against the project public key:

   ```sh
   # Verify SHA256SUMS against SHA256SUMS.minisig
   minisign -Vm SHA256SUMS \
     -P RWQroF71Mw572E0d5K9ndPm5/Uf94TRU99qOHfm3ZCNmB7CebJkjmim/

   # Or verify an individual installer directly against its <installer>.minisig file
   minisign -Vm AriadShift_1.0.0_aarch64.dmg \
     -P RWQroF71Mw572E0d5K9ndPm5/Uf94TRU99qOHfm3ZCNmB7CebJkjmim/
   ```

   If you have a checkout of the repository, you can pass `-p crates/ariad-host/data/pack-keys/d87b0e33f55ea02b.pub` instead of `-P`. `minisign -Vm` checks both the Ed25519 signature over the prehashed file and the signed trusted comment (`desktop:AriadShift release:v<version> file:<filename>`).

3. **Verify the SHA-256 digest against `SHA256SUMS`:**
   - **Linux:**
     ```sh
     sha256sum -c --ignore-missing SHA256SUMS
     ```
   - **macOS:**
     ```sh
     shasum -a 256 -c --ignore-missing SHA256SUMS
     ```
   - **Windows (PowerShell):**
     ```powershell
     $file = "AriadShift_1.0.0_x64-setup.exe"
     $actual = (Get-FileHash $file -Algorithm SHA256).Hash.ToLower()
     $line = Select-String -Path .\SHA256SUMS -Pattern "^([0-9a-f]{64})  $([regex]::Escape($file))$"
     if (-not $line -or $line.Matches[0].Groups[1].Value -ne $actual) {
       throw "SHA-256 verification failed for $file"
     }
     ```

> [!NOTE]
> **`.minisig` vs `.sig` files:** Detached `.minisig` files are signed with the project release key (`D87B0E33F55EA02B`) for manual `minisign -Vm` verification. Files ending in `.sig` (`*.AppImage.sig`, `*.app.tar.gz.sig`, `*-setup.exe.sig`) are base64-encoded Tauri updater signatures verified automatically by the desktop app during in-app updates against the updater public key (`CDEBF0BE86A5D2A5`) in [`apps/desktop/src-tauri/tauri.conf.json`](../apps/desktop/src-tauri/tauri.conf.json).

---

## 3. Desktop Installation and First-Run Guide

Download the installer for your platform from [GitHub Releases](https://github.com/bavanchun/AriadShift/releases) (replace `<version>` with the release version, such as `1.0.0`):

| Platform | Architecture | Installer assets | Updater artifacts |
|---|---|---|---|
| **macOS** | Apple Silicon (`aarch64` only) | `AriadShift_<version>_aarch64.dmg` | `AriadShift_aarch64.app.tar.gz` (`.sig`) |
| **Windows** | x64 (`x86_64`) | `AriadShift_<version>_x64-setup.exe` (NSIS), `AriadShift_<version>_x64_en-US.msi` (MSI) | `AriadShift_<version>_x64-setup.exe` (`.sig`) |
| **Linux** | x86_64 (`amd64`) | `AriadShift_<version>_amd64.deb`, `AriadShift-<version>-1.x86_64.rpm`, `AriadShift_<version>_amd64.AppImage` | `AriadShift_<version>_amd64.AppImage` (`.sig`) |

Intel macOS (`x86_64-apple-darwin`) is supported by the `ashift` CLI and browser converter, but desktop installers and updater metadata are built for Apple Silicon (`aarch64`) only.

### macOS: Unsigned first launch (Gatekeeper)

Because v1 macOS desktop bundles are unsigned and not notarized with an Apple Developer ID certificate, macOS Gatekeeper quarantines the downloaded `.dmg` and blocks the first launch until you explicitly allow it.

1. **Verify the download first:** Check `AriadShift_<version>_aarch64.dmg` with `minisign -Vm` and `shasum -a 256 -c` as shown in [Checksum and Minisign Verification](#2-checksum-and-minisign-verification).
2. **Install the app:** Open `AriadShift_<version>_aarch64.dmg` and drag `AriadShift.app` into `/Applications`.
3. **First launch via System Settings (recommended):**
   - Double-click `AriadShift.app` in `/Applications`.
   - macOS displays a Gatekeeper prompt stating that `"AriadShift.app" Not Opened` because Apple cannot verify the developer or check it for malware. Click **Done** (do **not** click **Move to Trash**).
   - Open **System Settings › Privacy & Security** and scroll down to the **Security** section.
   - Locate the notice *"AriadShift.app" was blocked to protect your Mac* and click **Open Anyway**.
   - Click **Open** in the confirmation dialog and authenticate with Touch ID or your macOS password.
   - `AriadShift` opens, registers the `ariadshift://` URL scheme with Launch Services, and launches normally on every subsequent run.

#### Advanced option: removing the quarantine attribute (`xattr`)

Power users and automated setups can instead clear the macOS quarantine extended attribute from the installed app bundle in Terminal:

```sh
xattr -dr com.apple.quarantine /Applications/AriadShift.app
```

> [!WARNING]
> **Trade-off of `xattr -dr com.apple.quarantine`:** Using **System Settings › Privacy & Security › Open Anyway** records an explicit per-application Gatekeeper exception while keeping macOS's standard first-run XProtect malware scan intact. Stripping `com.apple.quarantine` bypasses Gatekeeper and quarantine-triggered XProtect checks on the bundle altogether. Never run `xattr -dr com.apple.quarantine` unless you have already verified the `.dmg` against `SHA256SUMS` and its Minisign `.minisig` signature.

### Windows: Unsigned first launch (SmartScreen)

v1 Windows installers (`AriadShift_<version>_x64-setup.exe` and `AriadShift_<version>_x64_en-US.msi`) are not signed with an Authenticode certificate, so Microsoft Defender SmartScreen warns on first execution.

1. **Verify the download first:** Verify the installer's `.minisig` signature with `minisign -Vm` and its SHA-256 hash against `SHA256SUMS`.
2. **Run the installer:** Double-click `AriadShift_<version>_x64-setup.exe` (or `AriadShift_<version>_x64_en-US.msi`).
3. **Allow in SmartScreen:**
   - When the **"Windows protected your PC"** dialog appears (*"Microsoft Defender SmartScreen prevented an unrecognized app from starting"*), click **More info**.
   - Confirm that **App** shows `AriadShift_<version>_x64-setup.exe` (or `.msi`) and **Publisher** shows `Unknown publisher`.
   - Click **Run anyway** to complete the installation.
4. Subsequent launches of `AriadShift` from the Start Menu or desktop shortcut start normally without SmartScreen prompts.

### Linux: `.deb`, `.rpm`, and `.AppImage`

Verify your chosen package against `SHA256SUMS` and its `.minisig` signature before installing.

#### Debian and Ubuntu (`.deb`)

```sh
sudo apt install ./AriadShift_1.0.0_amd64.deb
```

#### Fedora, RHEL, and openSUSE (`.rpm`)

```sh
sudo dnf install ./AriadShift-1.0.0-1.x86_64.rpm
```

#### AppImage (`.AppImage`)

```sh
chmod +x AriadShift_1.0.0_amd64.AppImage
./AriadShift_1.0.0_amd64.AppImage
```

- **Deep-link registration (`ariadshift://`):** Linux URL registration requires `desktop-file-utils` (`update-desktop-database`) and `xdg-utils` (`xdg-mime`). The `.deb` and `.rpm` packages declare these dependencies automatically; install them through your distribution's package manager when using the AppImage.
- **Running without FUSE:** In environments without FUSE (such as minimal hosts or containers), run `APPIMAGE_EXTRACT_AND_RUN=1 ./AriadShift_1.0.0_amd64.AppImage`.
- **In-app updates:** On Linux, Tauri's in-app updater operates on the `.AppImage` bundle. Users who install via `.deb` or `.rpm` update by installing the new package release.

For batch queues, watched folders, deep links, and diagnostics in the desktop app, see [`docs/desktop.md`](desktop.md).

---

## 4. Runtime Prerequisites: Pandoc and LibreOffice

### Built-in routes (zero external dependencies)

Both `ashift` and AriadShift Desktop convert the following formats out of the box with no external tools installed:

- **Markdown (`md` / `markdown`), HTML (`html` / `htm`), and IR JSON (`json`):** parsed and written natively in `ariad-core`.
- **DoclingDocument JSON (`*.docling.json`):** mapped to IR natively in `ariad-core`.
- **Digital PDF input (`pdf`) and PDF output (`typst`):** handled by the built-in pure-Rust PDF text reader and the embedded Typst compiler (with bundled OFL Libertinus and Source Code Pro fonts covering Vietnamese).

### Pandoc (`>= 3.12, < 4`) — required for DOCX and EPUB

Reading and writing `.docx` and `.epub` files invokes Pandoc out of process (`ashift __engine pandoc`). Linux distribution packages (such as Debian, Ubuntu, or Arch repositories) often ship versions older than `3.12`.

- **macOS & Linux (Homebrew):** Installed automatically when you run `brew install bavanchun/tap/ashift`, or manually via `brew install pandoc`.
- **Official release packages (all platforms):** Download Pandoc `3.12` (or any `3.x` release `>= 3.12, < 4`) from the [official Pandoc releases](https://github.com/jgm/pandoc/releases).
- **From a repository checkout:** Run `just pandoc` to download Pandoc `3.12` into `.tools/pandoc/bin/pandoc` with SHA-256 verification.
- **Custom path:** Set `ASHIFT_PANDOC=/path/to/pandoc` if the binary is not on `PATH`.

### LibreOffice (`>= 24.2`, recommended `26.8.x`) — optional for legacy Office and ODF

When system LibreOffice `>= 24.2` is present, AriadShift automatically enables legacy Office and OpenDocument routes (`doc`, `rtf`, `odt` → `docx`; `xls`, `ods` → `xlsx`; `ppt`, `odp` → `pptx`; and all Office/ODF formats → `pdf`). Each conversion uses an isolated temporary profile with macros and automatic link updates disabled.

- **Install:** Download from [libreoffice.org](https://www.libreoffice.org/download/) or install via your package manager (`sudo apt install libreoffice`, `sudo dnf install libreoffice`, `brew install --cask libreoffice`, or `winget install TheDocumentFoundation.LibreOffice`).
- **Custom path:** Set `ASHIFT_SOFFICE=/path/to/soffice` to override `PATH` and standard platform install locations.
- **Linux sandbox namespace requirement:** When LibreOffice is installed on Linux, `ariad-host` runs it inside unprivileged user and mount namespaces to mount a workspace-backed private `/tmp` (because LibreOffice IPC ignores `TMPDIR`). On Ubuntu 24.04+ and 26.04 where AppArmor restricts unprivileged user namespaces by default, either install an [AppArmor profile allowing `userns` for `ashift`](https://documentation.ubuntu.com/security/security-features/privilege-restriction/apparmor/#apparmor-unprivileged-user-namespace-restrictions) or allow them host-wide:
  ```sh
  sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
  ```
  See [`docs/cli.md`](cli.md#ashift-convert) for details on `ASHIFT_SANDBOX`.

---

## 5. Engine Packs (`docling`) and Tesseract OCR

Heavy extraction models are kept out of the base installation and delivered as signed on-demand engine packs ([`docs/packs.md`](packs.md)). The `docling` pack ([`docs/docling.md`](docling.md)) adds layout-aware extraction for PDF (digital and scanned), DOCX, PPTX, XLSX, and images (`png`, `jpeg`, `tiff`, `webp`), including Heron layout analysis, TableFormer table structure recovery, and OCR.

### Installing the `docling` pack

```sh
# Install the recommended version from the built-in index
ashift engines install docling

# Or pin a specific pack version
ashift engines install docling --version 0.2.0
```

You can also install, verify, or remove packs from the **Engine packs** view in AriadShift Desktop.

During installation, `ashift` fetches `pack.toml` and `pack.toml.minisig` from the `pack-docling-v<version>` GitHub Release, verifies the Minisign signature against the compiled-in key (`D87B0E33F55EA02B`) and the `pack:docling version:<version>` trusted comment, verifies each component archive's size and SHA-256 digest (`runtime`, `models`, and `tessdata`), and extracts them into `<data>/packs/docling/<version>/`:

- **Linux:** `~/.local/share/ariadshift/packs`
- **macOS:** `~/Library/Application Support/com.ariadnev.ariadshift/packs`
- **Windows:** `%LOCALAPPDATA%\ariadnev\ariadshift\data\packs`
- **Override:** Set `ASHIFT_DATA_DIR` to relocate the data directory.

#### Offline / air-gapped pack installation

Download `pack.toml`, `pack.toml.minisig`, `docling-<version>-<target>-runtime.tar.zst`, `docling-<version>-models.tar.zst`, and `docling-<version>-tessdata.tar.zst` from the `pack-docling-v<version>` release into a single directory, carry it to the target machine, and run:

```sh
ashift engines install docling --from ./docling-pack
```

`--from` performs the same Minisign, SHA-256, and archive extraction checks without opening any network connection.

#### Managing installed packs

```sh
ashift engines                  # list engines and installed packs
ashift engines verify docling   # re-hash every installed file against receipt.toml
ashift engines remove docling   # uninstall the pack
```

### Tesseract 5 system installation (Vietnamese OCR)

The `docling` pack bundles the pinned `tessdata_best` traineddata (`vie`, `eng`, `osd`) and `configs/tsv`, plus RapidOCR (PP-OCRv6) for non-Vietnamese scripts. To run OCR on Vietnamese or mixed Vietnamese-English scans, install **Tesseract 5** on your system `PATH`:

- **Ubuntu / Debian:**
  ```sh
  sudo apt install tesseract-ocr
  ```
- **Fedora / RHEL:**
  ```sh
  sudo dnf install tesseract
  ```
- **macOS (Homebrew):**
  ```sh
  brew install tesseract
  ```
- **Windows:**
  Install Tesseract 5 from the [UB Mannheim Tesseract installer](https://github.com/UB-Mannheim/tesseract/wiki) and add its installation folder (typically `C:\Program Files\Tesseract-OCR`) to your `Path`.

> [!NOTE]
> **Linux sandbox path rules for Tesseract:** When the Linux sandbox launches Docling, it grants execution access to the canonical `tesseract` binary and read access to `<prefix>/{lib,lib64,libexec,share/tessdata,etc}` for a binary under `<prefix>/bin/`, in addition to base system paths (`/usr`, `/lib`, `/lib64`, `/nix/store`). Do not place `tesseract` directly in `/bin`, `~/bin`, or `~/.local/bin`, and avoid splitting its libraries across separate Linuxbrew Cellar directories; use a system package or a self-contained prefix.

### Windows Visual C++ Redistributable prerequisite for `docling`

While standalone `ashift.exe` links the C runtime statically, the Windows `docling` pack uses the system Microsoft runtime and excludes `vcruntime*` and `msvcp*` DLLs. Before running `docling` on Windows, install or update the latest supported **[Microsoft Visual C++ Redistributable x64 (2015–2022, v14)](https://aka.ms/vc14/vc_redist.x64.exe)**.

---

## 6. Verifying Your Environment (`ashift doctor`)

After installing the CLI or Desktop app and any optional prerequisites, run the environment diagnostics:

```sh
ashift doctor
```

`ashift doctor` checks:

1. **Pandoc (`pandoc_version`):** confirms Pandoc `>= 3.12, < 4` is reachable.
2. **Workspace (`workspace`):** verifies temporary workspace creation and cleanup.
3. **Engine round trips (`engine_describe`):** checks `describe` over Pandoc, built-in PDF, Typst, and optional LibreOffice.
4. **Engine pack directory and keys (`data_dir`, `pack_keys`):** confirms the pack directory is writable and lists compiled-in Minisign key IDs (`D87B0E33F55EA02B`).
5. **Installed packs (`pack_docling`):** checks the active pack receipt and file inventory.
6. **Sandbox (`sandbox`):** reports the effective isolation mode (`best-effort` default), Linux Landlock ABI, and seccomp availability.
7. **Docling and Tesseract (`docling_engine`, `tesseract`):** when the `docling` pack is installed, verifies that the engine answers `describe` with its models and checks for Tesseract 5 on `PATH`.

In AriadShift Desktop, open the **Diagnostics** tab to run the same checks interactively.

---

## 7. Self-Hosting and Container Images (GHCR)

AriadShift's cloud stack (Web app, Better Auth service, `/v1` API, conversion workers, PostgreSQL 18 with `pgmq`, and SeaweedFS S3 storage) runs on a single Linux host with Docker Compose and requires no external cloud accounts.

- **Docker Compose stack:** Located under `infra/compose/docker-compose.yml` with profiles `core` (Pandoc, PDF, Typst), `office` (adds LibreOffice), and `docling` (adds Tesseract and the signed `docling` pack). Copy `.env.example` to `infra/compose/.env`, replace the secret placeholders (for example with `openssl rand -hex 32`), and run:
  ```sh
  docker compose -f infra/compose/docker-compose.yml --profile core up -d --wait
  ```
- **Published GHCR images:** Release tags publish multi-arch (`linux/amd64`, `linux/arm64`) images to GitHub Container Registry (`ghcr.io/bavanchun/ariadshift-api`, `ariadshift-worker-core`, `ariadshift-worker-office`, `ariadshift-worker-docling`, `ariadshift-auth`, and `ariadshift-web`) with CycloneDX/SPDX SBOMs, SLSA provenance attestations, and keyless Cosign signatures tied to the GitHub Actions release workflow OIDC identity.
- **Documentation:** See [`docs/server.md`](server.md) for `ariad-server` configuration, database roles (`infra/server-roles.sql`), container security requirements, and resource sizing, and `docs/self-hosting.md` for the full Docker Compose operational guide.
