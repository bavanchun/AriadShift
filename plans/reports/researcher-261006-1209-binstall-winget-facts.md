# cargo-binstall and WinGet Distribution Facts (Verified 2026-10-06)

Scope: `cargo-binstall`, `crates.io` indexing policies, `cargo-dist` 0.33.0 release artifacts, `winget-pkgs` submission requirements, `wingetcreate`, `komac`, and `winget-releaser`.  
Method: crates.io API queries, GitHub API inspection of latest releases/tags, source-code analysis of `cargo-bins/cargo-binstall` and `axodotdev/cargo-dist`, and official Microsoft `winget-pkgs` validation specifications.  
Tags: **[tested]** = verified via live command/API query; **[source]** = verified directly from tagged source code; **[doc]** = verified from official documentation; **[unverified]** = could not confirm.

---

## Executive Summary

| Topic | Key Finding | Status |
|---|---|---|
| **cargo-binstall for unpublished crate** | Supported via `cargo binstall --git <URL> <crate>` or `--manifest-path <PATH> <crate>`. Clones HEAD depth-1. If prebuilt binary is missing, source fallback fails unless published. | **[source]** |
| **crates.io publication requirements** | Publishing only `ariad-cli` is impossible due to crates.io path dependency rejection. `ariad-core` and `ariad-host` must be published to crates.io first. | **[doc]** |
| **binstall + dist archive naming** | dist names archives by **package name** (`ariad-cli`), not binary name (`ashift`). binstall natively resolves dist's `{name}-{target}.{ext}` via built-in heuristics without `[package.metadata.binstall]`. | **[source]** |
| **Crate name availability** | Both `ashift` and `ariad-cli` are currently **free/available** on crates.io. | **[tested]** |
| **WinGet first submission** | Requires multi-file manifest PR (version, installer, defaultLocale). Must be submitted via `wingetcreate new`, `komac new`, or manual PR. `winget-releaser` **only supports updates**, not first submission. | **[doc]** |
| **WinGet portable zip installer** | Configured as `InstallerType: zip`, `NestedInstallerType: portable`, with `NestedInstallerFiles` mapping `RelativeFilePath: ashift.exe` and `PortableCommandAlias: ashift`. | **[doc]** |
| **WinGet unsigned executables** | WinGet accepts unsigned binaries (only MSIX/APPX mandates signing). PR pipeline runs SmartScreen, Defender, and multi-engine AV/PUA checks. | **[doc]** |
| **WinGet tooling & licenses** | `wingetcreate` v1.12.13.0 (MIT); `komac` v2.16.0 (GPL-3.0); `winget-releaser` v2 (AGPL-3.0). `winget-releaser` requires a fork of `winget-pkgs` and a classic PAT with `public_repo` + `workflow`. | **[tested]** |
| **dist 0.33.0 native support** | No native WinGet installer (issue #87 is open). No native binstall metadata emitter (compatibility relies on shared conventions). | **[source]** |

---

## 1. cargo-binstall & crates.io Dependency Rules

### 1.1 Installing from GitHub Releases Without crates.io Publication
- **Can it install unpublished crates?** Yes. `cargo-binstall` supports bypassing crates.io index lookups by pointing directly to a git repository or local manifest **[source]**.
- **Supported CLI Syntax**:
  1. **Git override**:
     ```bash
     cargo binstall --git https://github.com/bavanchun/AriadShift ariad-cli
     ```
     - **Mechanism**: Invokes `simple-git` / `gix` to shallow-clone the repository (`depth = 1`) into a temporary directory, parses the workspace `Cargo.toml` via `load_manifest_from_workspace`, extracts package metadata (`name`, `version`, `repository`, binaries), and resolves prebuilt binaries against GitHub Releases for that version **[source]** (`crates/binstalk/src/ops/resolve.rs:459-484`).
  2. **Local manifest path override**:
     ```bash
     cargo binstall --manifest-path ./crates/ariad-cli/Cargo.toml ariad-cli
     ```
     - **Mechanism**: Reads the local `Cargo.toml` directly, skipping git cloning and crates.io lookups entirely **[source]**.
- **Limitations**:
  - **Single crate only**: When `--git` or `--manifest-path` is provided, only one crate name can be supplied (multiple packages error out due to ambiguity) **[source]** (`crates/bin/src/args.rs:60-63`).
  - **HEAD commit only**: `cargo-binstall` has no `--branch`, `--tag`, or `--rev` flags. It always clones the default branch at `HEAD` **[source]**.
  - **Mutually exclusive**: `--git` and `--manifest-path` conflict with each other (`conflicts_with("manifest_path")`) **[source]**.
  - **Source fallback failure**: If release assets cannot be found or downloaded, binstall falls back to `Resolution::InstallFromSource` (strategy `compile`), which executes `cargo install <name> --version <version>`. Because standard `cargo install` queries crates.io without `--git`, this fallback **fails immediately** for unpublished crates **[source]** (`crates/binstalk/src/ops/resolve/resolution.rs:64-70`). To prevent failed compilation attempts, pass `--disable-strategies compile`.
  - **Build feature requirement**: `--git` requires `cargo-binstall` to be compiled with `feature = "git"` (enabled by default in official release binaries) **[source]**.

### 1.2 crates.io Publication Minimum Requirements
- **Can only `ariad-cli` be published?** **No.**
- **crates.io Policy**: `cargo publish` packages the crate and verifies that all dependencies can be resolved from the registry index. If a workspace member specifies path dependencies (e.g., `ariad-core = { path = "../ariad-core" }`), `cargo publish` will reject the crate unless a `version` requirement is present **and** that version already exists on crates.io **[doc]**.
- **Required Order**: To publish `ariad-cli` to crates.io, all internal path dependencies must be published in topological order first:
  1. Publish `ariad-core` v0.1.0 to crates.io.
  2. Publish `ariad-host` v0.1.0 to crates.io.
  3. Publish `ariad-cli` v0.1.0 to crates.io.
- Without publishing `ariad-core` and `ariad-host`, `ariad-cli` cannot be published to crates.io.

### 1.3 How binstall Locates dist-Produced Archives
- **Does dist emit binstall metadata automatically?** No. `cargo-dist` does not inject `[package.metadata.binstall]` into `Cargo.toml`, nor does it emit any binstall-specific manifest files **[source]**.
- **Is `[package.metadata.binstall]` required?** **No.** `cargo-binstall` maintains built-in default heuristics specifically tuned to match `cargo-dist` release naming conventions **[source]** (`binstalk-fetchers/src/gh_crate_meta.rs`).
- **Archive Naming Convention**:
  - `cargo-dist` derives the archive name from `app_name = package_info.name.clone()` and the variant target:  
    `{package_name}-{target}.{ext}` (e.g., `ariad-cli-x86_64-unknown-linux-musl.tar.xz`) **[source]** (`cargo-dist/src/tasks.rs:1350-1388, 1910-1935`).
  - **Crucial Distinction**: dist archives are named after the **package name** (`ariad-cli`), **NOT the binary name** (`ashift`).
- **Default Resolution Flow**:
  1. `cargo-binstall` constructs potential GitHub release URLs using templates:
     - `{ repo }/releases/download/v{ version }/{ name }-{ target }{ archive-suffix }`
     - `{ repo }/releases/download/{ version }/{ name }-{ target }{ archive-suffix }`
  2. It iterates through all supported package formats (`PkgFmt::iter()`), including `Txz` (`.tar.xz`, `.txz`) and `Zip` (`.zip`) **[source]** (`binstalk-types/src/cargo_toml_binstall/package_formats.rs`).
  3. Inside the archive, dist tarballs place files in a directory named `{name}-{target}/`. `cargo-binstall`'s directory inference checks `{name}-{target}/` and locates `{bin}{binary-ext}` (i.e., `ariad-cli-x86_64-unknown-linux-musl/ashift`) **[source]** (`binstalk-bins/src/lib.rs:57-92`).
  4. On Windows, dist creates flat zip archives without a root directory. `cargo-binstall` falls back to root extraction for `{bin}.exe` (`ashift.exe`) **[source]**.
- **Explicit Template (if added to Cargo.toml)**:
  To avoid heuristic discovery and GitHub API rate-limit calls, the explicit manifest configuration is:
  ```toml
  [package.metadata.binstall]
  pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }{ archive-suffix }"
  bin-dir = "{ name }-{ target }/{ bin }{ binary-ext }"
  pkg-fmt = "txz"

  [package.metadata.binstall.overrides.x86_64-pc-windows-msvc]
  pkg-fmt = "zip"
  bin-dir = "{ bin }.exe"
  ```

---

## 2. crates.io Namespace Check

*Query timestamp: 2026-10-06T12:44:30+07:00 via `https://crates.io/api/v1/crates/<name>` with user-agent verification **[tested]**.*

| Crate Name | Status | Crates.io Response |
|---|---|---|
| `ashift` | **FREE / AVAILABLE** | `{"errors": [{"detail": "crate `ashift` does not exist"}]}` |
| `ariad-cli` | **FREE / AVAILABLE** | `{"errors": [{"detail": "crate `ariad-cli` does not exist"}]}` |

Neither `ashift` nor `ariad-cli` is registered or squatting on crates.io.

---

## 3. Windows Package Manager (WinGet) Distribution

### 3.1 First Submission Requirements (`microsoft/winget-pkgs`)
- **Manifest Architecture**: Must be submitted as a multi-file manifest set consisting of 3 YAML files under `manifests/<p>/<Publisher>/<PackageIdentifier>/<PackageVersion>/` **[doc]**:
  1. `<PackageIdentifier>.yaml` (version manifest, e.g. `bavanchun.AriadShift.yaml`)
  2. `<PackageIdentifier>.installer.yaml` (installer details, architecture, download URLs, SHA256)
  3. `<PackageIdentifier>.locale.en-US.yaml` (default locale metadata: publisher, description, license)
  *Singleton manifests (`ManifestType: singleton`) are strictly prohibited in the community repository.*
- **Schema Headers**: Every file must declare `# yaml-language-server: $schema=...` pointing to an active schema version. Recommended versions: `1.9.0`, `1.10.0`, `1.12.0`, or `1.28.0` (versions `<= 1.6.0` are deprecated and rejected) **[doc]**.
- **PR Rules**:
  - Exactly **one package version** per PR.
  - Manifest files only (no documentation, dictionary, or tooling edits).
  - Installer URL must be HTTPS, publicly reachable, version-specific, and hosted on the publisher's official domain/repository (e.g. GitHub Releases).
  - The submitter must sign the Microsoft Contributor License Agreement (CLA) on the PR **[doc]**.
- **Can `winget-releaser` create a new package?** **NO.**
  - `winget-releaser` documentation explicitly states: *"At least one version of your package should already be present in the Windows Package Manager Community Repository. The action will use that version as a base to create manifests for new versions."* **[doc]**.
  - Brand-new package submissions must be created using `wingetcreate new <installer-url>` or `komac new`, or via manual PR.

### 3.2 Portable Zip Installer Specification
For a GitHub release zip containing `ashift.exe` (as produced by `cargo-dist` on Windows):
```yaml
# yaml-language-server: $schema=https://aka.ms/winget-manifest.installer.1.9.0.schema.json
PackageIdentifier: bavanchun.AriadShift
PackageVersion: 0.1.0
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
  - RelativeFilePath: ashift.exe
    PortableCommandAlias: ashift
Installers:
  - Architecture: x64
    InstallerUrl: https://github.com/bavanchun/AriadShift/releases/download/v0.1.0/ariad-cli-x86_64-pc-windows-msvc.zip
    InstallerSha256: <SHA256_HEX>
ManifestType: installer
ManifestVersion: 1.9.0
```
- WinGet unpacks the portable zip into `%LOCALAPPDATA%\Microsoft\WinGet\Packages\` and places an execution symlink/alias (`ashift.exe`) into `%LOCALAPPDATA%\Microsoft\WinGet\Links`, which is on the user's `PATH` **[doc]**.

### 3.3 Unsigned Executables & Pipeline Validation
- **Does WinGet accept unsigned binaries?** **Yes.** Digital code signatures are only strictly mandatory for MSIX/APPX packages (`SignatureSha256`). Standard `.exe` and portable `.zip` archives do not require code signing **[doc]**.
- **Automated Validation Pipeline (10 Steps)**:
  - **Step 03 (URLs Validation)**: Confirms the download URL is HTTPS, reachable, and not flagged by Defender SmartScreen for poor reputation **[doc]**.
  - **Step 07 (Installers Scan)**: Validates `InstallerSha256`. Runs static antivirus scans across multiple commercial engines and checks Microsoft Potentially Unwanted Application (PUA) criteria. Any PUA or malware detection fails validation immediately **[doc]**.
  - **Step 08 (Installation Validation)**: Installs the package in a clean, non-elevated Windows sandbox environment to verify unattended execution. Microsoft Defender runs a post-installation scan on extracted files (`Validation-Defender-Error`) **[doc]**.
- **Client-Side SmartScreen Behavior**:
  - Even though the PR pipeline approves the manifest, end-user machines running Windows Defender SmartScreen may display an *"unrecognized application"* banner upon first execution of a new unsigned binary until sufficient global reputation is accumulated **[doc]**.

### 3.4 Tooling Comparison: wingetcreate, komac, winget-releaser

| Metric / Requirement | `wingetcreate` | `komac` | `winget-releaser` |
|---|---|---|---|
| **Repository** | [microsoft/winget-create](https://github.com/microsoft/winget-create) | [russellbanks/Komac](https://github.com/russellbanks/Komac) | [vedantmgoyal9/winget-releaser](https://github.com/vedantmgoyal9/winget-releaser) |
| **Latest Stable Version** | **v1.12.13.0** (2026-07-23) **[tested]** | **v2.16.0** (2026-03-29) **[tested]** | **v2** (2025-01-27) **[tested]** |
| **License (SPDX)** | **MIT** **[tested]** | **GPL-3.0** **[tested]** | **AGPL-3.0** **[tested]** |
| **New Package Creation** | Supported (`wingetcreate new <url>`) | Supported (`komac new`) | **Unsupported** (Updates only) |
| **Requires winget-pkgs Fork** | No (API / local fork optional) | Yes (`komac sync`) | **Yes** (Strictly required) |
| **GitHub Token Type** | Personal Access Token | Classic PAT (`public_repo`) | **Classic PAT (`public_repo` + `workflow`)** |
| **Fine-Grained PAT Support**| Partial | Fails on PR creation (#310) | **Unsupported** |

*Note on AGPL-3.0*: `winget-releaser` is licensed under AGPL-3.0. Although it runs as an external CI Action rather than a linked binary, team licensing guidelines (AGENTS.md) prohibit AGPL-3.0 tools if strictly interpreted. `komac` (GPL-3.0 CLI) or `wingetcreate` (MIT CLI) in a custom GitHub Actions step avoids AGPL dependencies.

---

## 4. cargo-dist 0.33.0 Native Feature Assessment

*Source analysis of `axodotdev/cargo-dist` release v0.33.0 (released 2026-09-11) **[source]**.*

1. **Native WinGet Support**:
   - **Status**: **None.**
   - `cargo-dist` 0.33.0 supports installers: `shell`, `powershell`, `npm`, `homebrew`, `msi`, and `pkg` (macOS) (`cargo-dist/src/config/v1/installers/mod.rs`).
   - WinGet integration is tracked under open issue [#87](https://github.com/axodotdev/cargo-dist/issues/87) and is not implemented in dist 0.33.0 **[source]**.
2. **Native cargo-binstall Support**:
   - **Status**: **Implicit by design (no manifest emission).**
   - dist does not emit `[package.metadata.binstall]` or maintain a binstall installer target **[source]**.
   - Compatibility exists because `cargo-binstall` was designed to parse the exact artifact matrix and archive hierarchy generated by dist.

---

## 5. Primary Source Reference Index

1. **cargo-binstall**:
   - Repository: <https://github.com/cargo-bins/cargo-binstall>
   - Latest release: `v1.25.1` (<https://github.com/cargo-bins/cargo-binstall/releases/tag/v1.25.1>)
   - Docs & spec: <https://github.com/cargo-bins/cargo-binstall/blob/main/SUPPORT.md>
2. **crates.io Policies & Index**:
   - Package dependencies: <https://doc.rust-lang.org/cargo/reference/publishing.html>
   - API endpoints: `https://crates.io/api/v1/crates/{ashift,ariad-cli}`
3. **cargo-dist**:
   - Repository: <https://github.com/axodotdev/cargo-dist>
   - Latest release: `v0.33.0` (<https://github.com/axodotdev/cargo-dist/releases/tag/v0.33.0>)
   - Installer concepts: <https://axodotdev.github.io/cargo-dist/book/installers/>
   - WinGet feature request: <https://github.com/axodotdev/cargo-dist/issues/87>
4. **WinGet & Microsoft Community Repository**:
   - Community repo: <https://github.com/microsoft/winget-pkgs>
   - First contribution guide: <https://github.com/microsoft/winget-pkgs/blob/master/doc/FirstContribution.md>
   - Validation pipeline: <https://github.com/microsoft/winget-pkgs/blob/master/doc/Validation.md>
   - Policies & installer types: <https://github.com/microsoft/winget-pkgs/blob/master/doc/Policies.md>
5. **WinGet Manifest Tools**:
   - `wingetcreate`: <https://github.com/microsoft/winget-create> (v1.12.13.0, MIT)
   - `komac`: <https://github.com/russellbanks/Komac> (v2.16.0, GPL-3.0)
   - `winget-releaser`: <https://github.com/vedantmgoyal9/winget-releaser> (v2, AGPL-3.0)
