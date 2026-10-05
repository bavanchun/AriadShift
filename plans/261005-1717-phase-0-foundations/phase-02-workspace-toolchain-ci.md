---
phase: 2
title: "Workspace, toolchain, CI and public repo"
status: pending
priority: P1
effort: "9h"
dependencies: [1]
---

# Phase 2: Workspace, toolchain, CI and public repo

## Goal

Create a compiling three-crate Cargo workspace, the pnpm and uv workspaces, pinned toolchains, a `justfile` and a hardened 3-OS GitHub Actions workflow. Then make the repository public. Every later phase is verified on Linux, macOS and Windows from its first commit.

## Context links

- [Toolchain research](./research/researcher-01-toolchain-report.md): recommendations table, Q5, Q7–Q9
- [ARCHITECTURE.md](../../ARCHITECTURE.md) §2, §5, §16 (as updated in phase 1)
- [AGENTS.md](../../AGENTS.md): Versions, Git

## Key insights

- CI must exist before the cross-OS-sensitive code (process-tree kill, Pandoc output, comrak on wasm), because this Arch machine cannot verify any of it.
- Local state (2026-10-06):
  - Installed: Node 26.10 (via mise), pnpm 12.9.1, uv 0.12.8, Python 3.14.7, Docker.
  - **Missing: Rust, just, Pandoc, Typst.**
- Windows shells:
  - Never set `shell := ["bash", …]` in the justfile; on Windows `bash` can resolve to WSL.
  - Use just's default `sh` (Git's `sh.exe` on Windows) with POSIX one-liners, and `defaults.run.shell: bash` in the workflow.
- Paths printed by Git Bash are MSYS-style (`/d/a/...`), and Rust cannot spawn them. `ASHIFT_PANDOC` is therefore built from just's `justfile_directory()`, which is a native path. The installer flattens every OS archive into one fixed layout.
- `brand/pnpm-lock.yaml` moves to a root `pnpm-lock.yaml`. The `pnpm --dir brand …` commands keep working.
- Pin versions from the research table, but **re-verify each against its registry on the day you write the manifest** (AGENTS.md rule).
- Commit email: the user decided to keep the existing author email in public history. Do not rewrite history.

## Requirements

Functional:
- `cargo build` produces `ashift`, and `ashift --version` prints `ashift <version>`.
- `just ci` runs the full gate locally and in CI with identical commands.
- `just pandoc` installs Pandoc 3.12 at `.tools/pandoc/bin/pandoc[.exe]` on Linux x64/arm64, macOS arm64/x64 and Windows x64. Every run verifies the downloaded or cached archive against a pinned SHA-256.

Non-functional:
- Edition 2024, `rust-version = "1.99"`.
- Workspace lints, with `forbid(unsafe_code)` in `ariad-core`.
- Lockfiles committed.
- No Turborepo/Nx.
- CI supply chain pinned.

## Architecture

```text
ariadshift/
├── Cargo.toml / Cargo.lock      workspace: crates/*, [workspace.package|dependencies|lints]
├── rust-toolchain.toml          1.99.0 + rustfmt, clippy, wasm32-unknown-unknown
├── deny.toml / typos.toml
├── justfile                     fmt | lint | test | wasm | deny | js | py | pandoc | ci
├── scripts/install-pandoc.sh
├── package.json / pnpm-workspace.yaml / pnpm-lock.yaml   (members: brand)
├── pyproject.toml / uv.lock     uv workspace root, members: fixtures/gen (added in phase 3)
├── .node-version                24
├── .gitattributes / .gitignore
├── LICENSE / NOTICE / README.md / docs/SECURITY.md
├── .github/workflows/ci.yml
├── .tools/                      gitignored: pandoc-3.12 archive + pandoc/bin/pandoc[.exe]
└── crates/{ariad-core, ariad-host, ariad-cli}
```

## Files

| Action | File | Purpose | Size |
|---|---|---|---|
| Create | `Cargo.toml`, `Cargo.lock` | Workspace manifest and pins | ~70 |
| Create | `rust-toolchain.toml` | Rust 1.99.0 + wasm target | 5 |
| Create | `crates/ariad-core/{Cargo.toml,src/lib.rs}` | Crate root (docs only) | ~20 |
| Create | `crates/ariad-host/{Cargo.toml,src/lib.rs}` | Crate root | ~15 |
| Create | `crates/ariad-cli/{Cargo.toml,src/main.rs}` | clap app, `[[bin]] name = "ashift"` | ~30 |
| Create | `crates/ariad-cli/tests/cli_smoke.rs` | `ashift --version` | ~15 |
| Create | `deny.toml`, `typos.toml` | License and spelling gates | ~35 |
| Create | `justfile` | Root tasks | ~50 |
| Create | `scripts/install-pandoc.sh` | Checksum-pinned, layout-flattening installer | ~100 |
| Create | `package.json`, `pnpm-workspace.yaml`, `pnpm-lock.yaml` | pnpm workspace | ~15 |
| Delete | `brand/pnpm-lock.yaml` | Superseded by the root lockfile | — |
| Create | `pyproject.toml`, `uv.lock` | uv workspace root | ~15 |
| Create | `.node-version`, `.gitattributes` | Node 24; byte and line-ending policy | ~15 |
| Modify | `.gitignore` | `target/`, `.tools/`, `.venv/`, `*.pending-snap`, `*.snap.new`, `**/.claude/` | +6 |
| Create | `LICENSE`, `NOTICE` | Apache-2.0; `Copyright 2026 VChun`; fixtures carry per-file licenses | ~210 |
| Create | `README.md` | What it is, status, dev setup, `just` commands, license | ~80 |
| Create | `docs/SECURITY.md` | Private vulnerability reporting | ~20 |
| Create | `.github/workflows/ci.yml` | 3-OS matrix running `just ci` | ~90 |
| Modify | `AGENTS.md` | Current state, commands, lockfile location, `just pandoc` | ~10 |

## Implementation steps

1. **Local toolchain (Arch).**
   - Install `rustup` and `just` through pacman. `rust-toolchain.toml` then provisions 1.99.0.
   - Get Node 24 through mise from `.node-version`. Install the cargo tools (`cargo-nextest`, `cargo-deny`, `cargo-insta`, `typos-cli`) at the same versions CI pins.
   - List these in the README; they are not committed as project requirements.
2. **Re-verify versions** against crates.io, npm and GitHub for every pin in the research table and every pin in this phase. Record changed numbers in ARCHITECTURE.md §2 in the same commit.
3. **Cargo workspace.**
   - `[workspace]`: `resolver = "3"`, members `crates/*`.
   - `[workspace.package]`: edition 2024, rust-version 1.99, `license = "Apache-2.0"`, `repository`, `publish = false`.
   - `[workspace.dependencies]` pins every crate named in ARCHITECTURE §2.3 (phase 1). Notable details:
     - insta with features `json` and `glob`
     - comrak with `default-features = false, features = ["shortcodes"]`
     - zip with `default-features = false, features = ["deflate"]`
   - Crates declare only what they use in the phase that needs it.
   - Lints: `[workspace.lints.rust] unsafe_code = "deny"` and `[workspace.lints.clippy] all = "warn"`; `ariad-core` adds `#![forbid(unsafe_code)]`. CI turns warnings into errors through `CARGO_BUILD_WARNINGS=deny`, which setup-rust-toolchain sets by default.
4. **ariad-cli.** `[[bin]] name = "ashift"`, built with clap derive. The smoke test asserts `ashift --version` prints `ashift 0.0.0`.
5. **`scripts/install-pandoc.sh`.**
   - Compatible with POSIX sh and bash 3.2: no associative arrays, uses `case`.
   - Map `uname -s`/`uname -m` to the 3.12 asset. Windows reports `MINGW64_NT*`/`MSYS_NT*`.
   - Keep the downloaded archive at `.tools/pandoc-3.12.<ext>`. **Verify its SHA-256 on every run**, including when it was restored from the CI cache, with `sha256sum` or `shasum -a 256`. Never trust a binary's `--version` alone.
   - Extract to a temp dir, then move the binary to the flat path `.tools/pandoc/bin/pandoc` (`pandoc.exe` on Windows) whatever the archive's internal layout. Tools: `tar`, `unzip`, or `7z x` as the fallback.
   - Download with `curl -fsSL --retry 3` from `https://github.com/jgm/pandoc/releases/download/3.12/<asset>`.
   - Re-derive the checksum table with `gh api repos/jgm/pandoc/releases/tags/3.12 --jq '.assets[]|.name+" "+.digest'`. The 2026-10-06 values in the research report matched the GitHub digests.
6. **justfile** (default shell, POSIX one-liners):
   - `export ASHIFT_PANDOC := justfile_directory() / ".tools" / "pandoc" / "bin" / ("pandoc" + if os() == "windows" { ".exe" } else { "" })`.
   - Recipes:
     - `pandoc`: `bash scripts/install-pandoc.sh`
     - `fmt`: `cargo fmt --all`
     - `lint`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `typos`
     - `wasm`: `cargo check -p ariad-core --target wasm32-unknown-unknown`
     - `test`: `cargo nextest run --workspace`, then `cargo test --workspace --doc`
     - `deny`: `cargo deny check`
     - `js`: `pnpm install --frozen-lockfile`
     - `py`: `uv lock --check`
     - `ci`: runs `lint wasm test deny js py`
   - `pandoc_bin` (phase 5) treats a path that does not exist as "missing" and prints the `just pandoc` hint, so `just ci` before `just pandoc` fails clearly.
7. **pnpm workspace.**
   - Root `package.json`: `private: true`, `packageManager: "pnpm@12.9.1"`, `engines.node: ">=24"`.
   - `pnpm-workspace.yaml` lists `brand`.
   - Run `pnpm install`, delete `brand/pnpm-lock.yaml`, then run `pnpm --dir brand build`. `git status` must show no change under `brand/svg`, `brand/png`, `brand/icon-composer` or `brand/preview.png`.
8. **uv workspace.** The root `pyproject.toml` is a non-package project:
   - `requires-python = ">=3.14"`
   - `[tool.uv] required-version`
   - `[tool.uv.workspace] members = []` (phase 3 adds `fixtures/gen`)

   Then run `uv lock`.
9. **.gitattributes.** Later lines win:
   ```gitattributes
   * text=auto eol=lf
   fixtures/** -text
   fixtures/gen/**/*.py text eol=lf
   fixtures/gen/**/*.md text eol=lf
   fixtures/gen/**/*.toml text eol=lf
   fixtures/manifest.toml text eol=lf
   *.snap text eol=lf
   *.docx *.png *.jpg *.pdf *.zip *.djvu *.ttf *.otf *.woff2 binary
   ```
10. **CI workflow.**
    - Triggers: `push` to `main`, `pull_request`, `workflow_dispatch`. `concurrency` cancels superseded runs. `permissions: contents: read`.
    - Matrix: `[ubuntu-26.04, macos-26, windows-2025]`, `fail-fast: false`, `defaults.run.shell: bash`.
    - **Every action is pinned by full commit SHA** with a `# vX.Y.Z` comment: checkout v7, setup-rust-toolchain v2, install-action v2, pnpm/action-setup v6, setup-node v7, setup-uv v10, actions/cache.
    - `actions/checkout` sets `persist-credentials: false`.
    - `taiki-e/install-action` installs exact versions (`just@1.58.0,cargo-nextest@0.9.146,cargo-deny@0.20.2,typos@1.50.3`, re-verified).
    - Cache `.tools/` keyed by OS + `hashFiles('scripts/install-pandoc.sh')`.
    - Run `just pandoc`, then a sanity step `"$ASHIFT_PANDOC" --version` (exported by the workflow with the same native path rule: `$GITHUB_WORKSPACE`-based, `cygpath -m` on Windows), then `just ci`.
11. **LICENSE, NOTICE, README, docs/SECURITY.md.**
    - LICENSE is the verbatim Apache-2.0 text.
    - NOTICE reads: `AriadShift`, `Copyright 2026 VChun`, and "Files under fixtures/ are distributed under the per-file licenses recorded in fixtures/manifest.toml".
    - README covers the purpose, status (pre-alpha, Phase 0), the brand lockup, dev setup, `just pandoc` / `just ci`, links to ARCHITECTURE.md, and the license.
    - SECURITY.md points to GitHub private vulnerability reporting and asks reporters not to attach documents with private data.
12. **AGENTS.md.**
    - "Current state": the workspace exists, the gate is `just ci`, run `just pandoc` first, and `ashift convert` is not built yet.
    - Note the root `pnpm-lock.yaml`.
13. **Make the repository public.** This is outward-facing and hard to undo, so **ask the user for an explicit go-ahead first.**
    1. **Review what will be published.**
       - Run `git status --ignored`.
       - Read every untracked file that will be committed (`plans/`, `docs/`). Confirm `**/.claude/` is ignored and that no agent memory or machine-local data is staged.
       - The plan and research files are intended to be public. Confirm with the user.
    2. **Secret scan of history and the working tree.**
       - Use gitleaks with the image pinned by digest and the repo mounted read-only: `docker run --rm -v "$PWD:/repo:ro" zricethezav/gitleaks@sha256:<digest> git /repo` and `… dir /repo`.
       - Expect zero findings. The author email in commits is public by the user's decision.
    3. Push phases 1–2. Run `gh repo edit bavanchun/AriadShift --visibility public --accept-visibility-change-consequences`.
    4. Enable private vulnerability reporting and secret-scanning push protection with `gh api`.
    5. Confirm the first CI run is green on all three OSes.

## Todo

- [ ] Local toolchain installed
- [ ] Versions re-verified; ARCHITECTURE §2 updated if any changed
- [ ] Cargo workspace + three crates + `ashift --version` smoke test
- [ ] `install-pandoc.sh` (flat layout, verify on every run, re-derived checksums)
- [ ] justfile with native `ASHIFT_PANDOC` and `ci`
- [ ] pnpm workspace; brand build byte-identical
- [ ] uv workspace root locked
- [ ] .gitattributes (binary fonts), .gitignore (`**/.claude/`)
- [ ] LICENSE, NOTICE, README.md, docs/SECURITY.md
- [ ] CI workflow (SHA-pinned actions, pinned tools, no persisted credentials, Pandoc sanity step)
- [ ] AGENTS.md current state
- [ ] User go-ahead → publish review → pinned gitleaks (history + tree) → public → first 3-OS run green

## Test scenario matrix

| Priority | Scenario | Expected |
|---|---|---|
| Critical | `just ci` on Linux locally | Exit 0 |
| Critical | CI on ubuntu-26.04, macos-26, windows-2025 | All green |
| Critical | `"$ASHIFT_PANDOC" --version` in CI on each OS | Reports 3.12 |
| High | Tampered cached archive (cache hit) | Script re-hashes, fails, re-downloads |
| High | Tampered checksum table | Exit non-zero before extraction |
| High | `just ci` before `just pandoc` | Clear "run `just pandoc`" message (once tests need Pandoc) |
| Medium | wasm check | Passes |
| Medium | `pnpm --dir brand build` after the lockfile move | No diff |

## Success criteria

- The first public CI run is green on all three OSes.
- `ashift --version` works.
- `cargo deny check` passes.
- The repository is public, with private vulnerability reporting and push protection on.

## Risk assessment

| Risk | Likelihood | Mitigation |
|---|---|---|
| Git Bash lacks `unzip` | Low | `7z x` fallback |
| macOS bash 3.2 breaks the script | Medium | No bash 4 features; also run the script under `sh` on Linux |
| An action's SHA pin goes stale | Certain over time | Renovate later; the comment keeps the version readable |
| The public flip exposes something private | Low | Publish review + gitleaks (history and tree) + user gate |

## Security considerations

- Downloaded binaries are verified on every run, and a cache hit is never trusted.
- CI uses SHA-pinned actions, pinned tool versions, a read-only token that is not persisted, and no secrets.
- Before the flip: the working tree is reviewed and scanned, agent folders are ignored, and the scanner image is pinned and mounted read-only.

## Next steps

Phase 3 adds the fixture suite. Phase 4 starts after phase 3 lands, because its IR snapshots cover phase 3's fixtures.
