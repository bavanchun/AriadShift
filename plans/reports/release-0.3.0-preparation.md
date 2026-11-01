# Release 0.3.0 preparation (CLI + MCP + crates, preview)

Date: 2026-11-01. Branch: `agent-rex/claude-9852` (from `feat/pdf-structural-tools`, 21 commits ahead of `origin/dev`). Source of the procedure: [docs/releasing.md](../../docs/releasing.md) sections 1 to 6.

## Outcome

- **Case that applies: the version bump needs generated files, so no manifest version was edited.** `docs/releasing.md` step 1.3 regenerates `Cargo.lock`, `fuzz/Cargo.lock`, `THIRD_PARTY_LICENSES`, both OpenAPI documents and the SDK lock in the same commit as the manifests. Editing `Cargo.toml` alone would leave `Cargo.lock` stale, and every `--locked` command (`cargo publish --locked`, `just ci`) would fail. The whole bump is therefore one later commit, produced by the integrator through the build lock (commands below).
- Nothing was built, pushed, tagged or published. No cargo, just, pnpm or uv command was run.
- Docs prepared in this branch: the 0.3.0 entry in `docs/release-notes.md` (the release notes source; there is no root changelog and `dist` only generates the GitHub Release body from the tag), and corrections in `docs/releasing.md` so the procedure matches what the publish workflow already does.
- Tool and feature documentation already exists (`docs/tools.md`, `docs/cli.md` with one example per tool, `docs/mcp.md`, `docs/parity-matrix.md`), so the release notes link to them instead of repeating them.

## Commits

| Commit | Subject | Content |
|---|---|---|
| `c6048de` | `docs(release): cover ariad-pdf and ariad-engine-io in the release steps` | `docs/releasing.md`: six published crates, five workspace dependency entries, `tauri.conf.json`, publish order, token scope |
| `dc499c8` | `docs(release): add the 0.3.0 release notes` | `docs/release-notes.md`: 0.3.0 entry marked `(unreleased)` |
| (this report) | `docs(reports): add the 0.3.0 release preparation report` | this file |

Commit messages were checked with `.tools/bin/committed --config committed.toml` (the binary from the main checkout; `scripts/check-commits.sh` itself needs `.tools/bin/committed` in the worktree, which `just lint-tools` would install, so it was not run).

## 1. Files that carry the version

Evidence: `git grep` for `0.2.0` over the tree plus the file list of the `chore(release): prepare 0.2.0` commit (`3397eb3`) and its follow-up `ba73a05`.

**Edited by hand in the bump commit**

| File | What changes |
|---|---|
| `Cargo.toml` | `[workspace.package] version` and the five `[workspace.dependencies]` entries `ariad-core`, `ariad-engine-io`, `ariad-pdf`, `ariad-typst`, `ariad-host` (all currently `0.2.0`) |
| `apps/desktop/src-tauri/tauri.conf.json` | `"version"`. Tauri reads this, not the Cargo version. `desktop.yml` also runs on `v*` tags, so it must match. |

**Inherit the workspace version (no edit)**: `crates/ariad-core`, `ariad-pdf`, `ariad-engine-io`, `ariad-typst`, `ariad-host`, `ariad-cli` (published), and `ariad-server`, `ariad-wasm`, `apps/desktop/src-tauri` (`publish = false`). `fuzz/Cargo.toml` stays at `0.0.0` (`ariad-fuzz`). No `package.json` has a `version` field.

**Generated (must be regenerated, never hand-edited)**

| File | Generator |
|---|---|
| `Cargo.lock` | `cargo check --workspace` (or `cargo update --workspace`); nine workspace entries |
| `fuzz/Cargo.lock` | `cargo update --workspace --manifest-path fuzz/Cargo.toml`; entries `ariad-core`, `ariad-engine-io`, `ariad-host`, `ariad-pdf`, `ariad-typst` |
| `THIRD_PARTY_LICENSES` | `just licenses` (lines 14929 onward list the workspace crates) |
| `crates/ariad-server/tests/snapshots/openapi_v1.json` (`info.version`) | `ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo test --locked -p ariad-server --test openapi_v1_snapshot` |
| `schemas/openapi.v1.json` (`info.version`) | `just openapi` |
| `packages/sdk/generator.lock.json` (`spec_sha256`) | `pnpm --filter @ariadshift/sdk generate` then `lock` |

`just licenses` also rewrites `apps/desktop/THIRD_PARTY_JS_LICENSES` and the web notices through `just desktop-licenses` and `just web-licenses`; stage them only if they change.

**Version strings that must NOT change**: the Docling pack (`engines/docling/pyproject.toml`, `crates/ariad-host/data/packs.toml`, `docs/packs.md`, `docs/docling.md`, `requires_ashift = ">=0.2.0, <2.0.0"`, which 0.3.0 satisfies), `bench/results/heavy-routes.json`, `toolchain-wheels-v0.2.0` pins, and the test fixture versions in `crates/ariad-engine-io/src/lib.rs`.

**Illustrative samples that will show 0.2.0 until refreshed (optional)**: the `ariad-core 0.2.0` engine row and JSON samples in `docs/cli.md` (lines 337, 356, 380 to 389). Docling rows there are pack versions and stay.

## 2. Bump pull request: commands that need builds (integrator, through the build lock)

Run from a branch off an up-to-date `dev` after this branch has merged (step 1.1 of releasing.md creates `chore/release-0.3.0`). Order matters.

```bash
# 1. Hand edits (no build): Cargo.toml (workspace version + 5 dependency entries) and tauri.conf.json -> 0.3.0

# 2. Refresh the main lock and check the workspace
cargo check --workspace            # or: cargo update --workspace
cargo check --locked --workspace

# 3. Fuzz lock (separate workspace)
cargo update --workspace --manifest-path fuzz/Cargo.toml
cargo check --locked --manifest-path fuzz/Cargo.toml

# 4. OpenAPI documents (server snapshot + committed document)
just openapi
ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo test --locked -p ariad-server --test openapi_v1_snapshot

# 5. SDK lock
pnpm --filter @ariadshift/sdk generate
pnpm --filter @ariadshift/sdk lock
just sdk-check

# 6. Licence notices (cargo-about must be exactly 0.9.2: cargo install --locked cargo-about --version 0.9.2)
just licenses
just licenses-check

# 7. Local quality gate
just ci
just check-release-workflow       # also part of `just static`; confirms the six-crate publish list and order

# 8. Stage explicit paths and commit
git add Cargo.toml Cargo.lock THIRD_PARTY_LICENSES apps/desktop/src-tauri/tauri.conf.json \
  fuzz/Cargo.lock crates/ariad-server/tests/snapshots/openapi_v1.json \
  schemas/openapi.v1.json packages/sdk/generator.lock.json
git diff --staged
git commit -m "chore(release): prepare 0.3.0"
```

Expected diff: version lines only, plus the `info.version` fields and the SDK `spec_sha256`. If `just licenses` changes anything beyond the workspace crate versions, stop and inspect.

## 3. Dry run of the crates.io publication

Publish order (from `.github/workflows/publish-crates.yml`, enforced by `scripts/check-release-workflow.sh`): `ariad-core`, `ariad-pdf`, `ariad-engine-io`, `ariad-typst`, `ariad-host`, `ariad-cli`. `ariad-pdf` and `ariad-engine-io` each depend on `ariad-core` only (among workspace crates), so `ariad-core` must exist first; `ariad-host` needs `ariad-core`, `ariad-pdf`, `ariad-engine-io` and, through its optional `typst` feature, `ariad-typst`; `ariad-cli` needs `ariad-host`.

Run after the bump commit exists (the lock must match, because of `--locked`):

```bash
# Acceptance command from the phase file; one invocation lets cargo resolve the unpublished
# 0.3.0 siblings from the workspace, so no crate has to be on crates.io yet.
cargo publish --dry-run --locked -p ariad-core -p ariad-pdf -p ariad-engine-io

# Full rehearsal of the real publish list
cargo publish --dry-run --locked \
  -p ariad-core -p ariad-pdf -p ariad-engine-io -p ariad-typst -p ariad-host -p ariad-cli

# Package contents
cargo package --list --locked -p ariad-pdf
cargo package --list --locked -p ariad-engine-io
```

Caveats for the integrator, not verified here because no cargo command was run:

- `ariad-core` can be dry-run alone. `ariad-pdf` and `ariad-engine-io` dry-run alone only once `ariad-core 0.3.0` is on crates.io; before that, a single-crate run stops with a registry resolution error for `ariad-core ^0.3.0`. That is expected, not a defect. Use the multi-package form above.
- Both new crates have `publish = true`, a description, `readme = "../../README.md"`, keywords, categories and `[package.metadata.dist] dist = false`. `readme` points outside the crate directory; cargo copies it into the package, but confirm that `cargo package --list` shows `README.md`.
- "0.3.0 cannot half-publish": `publish-crates.yml` skips versions already on crates.io and waits for the API after each crate, so a failed run is re-runnable. The crates are published before the smoke run, so an error after `ariad-core` still leaves a partial set on crates.io; follow the "fix forward with the next patch version" row of the failure table.

## 4. Remaining steps after the maintainer's written GO — NOT DONE

Nothing below was started. Each step touches `dev`, `main`, tags, GitHub environments or crates.io and needs the maintainer's explicit written go-ahead ([AGENTS.md](../../AGENTS.md)); the plan's gate is `to-mac/p03-release-gate.md`.

| # | Step | Command or action | Status |
|---|---|---|---|
| a | Land this branch on `dev` (phase pull request, rebase merge) | per plan run authorization | NOT DONE |
| b | Create `chore/release-0.3.0`, apply section 2, open the PR into `dev` | `git push -u origin chore/release-0.3.0`; `gh pr create --base dev --title "chore(release): prepare 0.3.0" --body "Prepare release v0.3.0."` | NOT DONE |
| c | Wait for `ci.yml` (three OSes) and the `release.yml` rehearsal (five archives, `pr-run-mode = "upload"`), then `gh pr merge --rebase --delete-branch` | releasing.md 1.7 | NOT DONE |
| d | Promotion PR `dev` -> `main`, merge commit only (`gh pr merge --merge`, never squash, never `--delete-branch`) | `gh pr create --base main --head dev --title "chore: promote dev to main" --body "Promote dev to main for v0.3.0."` | NOT DONE, awaiting GO |
| e | Tag `v0.3.0` on the `main` merge commit | verify `HEAD^2 == origin/dev`, then `git tag v0.3.0 && git push origin v0.3.0` | NOT DONE, awaiting GO |
| f | Watch `release.yml` to completion | `run_id=$(gh run list --workflow=release.yml --branch=v0.3.0 --limit 1 --json databaseId --jq '.[0].databaseId'); gh run watch "$run_id" --exit-status` | NOT DONE |
| g | Confirm these jobs succeeded individually (a green `announce` hides skipped ones): `host`, `prepare-homebrew-formula`, `publish-homebrew-formula`, `custom-publish-crates` (the `publish-crates.yml` publish of all six crates) | `gh run view "$run_id" --json jobs` | NOT DONE |
| h | Confirm `desktop.yml` also ran for the tag and attached installers plus a valid `latest.json` | releasing.md "Failure responses" | NOT DONE |
| i | Run `release-smoke.yml` (four clean-runner jobs) | `gh workflow run release-smoke.yml -f version=0.3.0`, then watch | NOT DONE |
| j | WinGet manifests | `scripts/winget-manifest.sh 0.3.0` and PR to `microsoft/winget-pkgs` (releasing.md section 5) | NOT DONE |
| k | Close-out docs on `dev`: change `0.3.0 (unreleased)` in the release notes to the tag date; update the version claims in `README.md` (package managers paragraph, crate list, "first published pack" paragraph), `AGENTS.md` "Current state", `docs/parity-matrix.md` ("`exists` means the route ships in v0.2.0"), `docs/cli.md` samples; then `grep -rn TBD-AFTER-RELEASE README.md AGENTS.md docs` (only `docs/releasing.md` may match; it matches nothing outside it today) | releasing.md section 6 | NOT DONE |

## 5. Security scan (read-only, static)

**Secrets in new files.** The 313 files added since `v0.2.0` (`git diff v0.2.0..HEAD --diff-filter=A`) were scanned by content with patterns for AWS keys, private key blocks, GitHub tokens, Slack tokens, `sk-` style keys and Google API keys. No match. The added tree is `crates/ariad-pdf` (87), `plans/reports` (58), `crates/ariad-core` (46), `fixtures/pdf-tools` (44), plan files, fuzz seeds and corpus (22), `crates/ariad-host` (12), `crates/ariad-cli` (8), three schemas and docs. No dotenv, key or credential files were found among the added file names. The only binary additions are the PDF fixtures under `fixtures/pdf-tools/`; `fixtures/manifest.toml` has 69 `pdf-tools` entries, which is the place their source and licence are recorded (not audited line by line here).

**Dependency and licence delta since `v0.2.0` (Cargo.toml and Cargo.lock diffs).**

| Item | Change |
|---|---|
| New workspace crates | `ariad-pdf`, `ariad-engine-io` (first-party, Apache-2.0 via `license.workspace`) |
| New third-party crates in `Cargo.lock` | none. `ariad-pdf` uses `lopdf`, `schemars`, `serde`, `serde_json`, `thiserror`, `unicode-normalization`, all already locked. |
| Version bumps in `Cargo.lock` | `jsonschema`, `jsonschema-regex`, `jsonschema-value`, `referencing` 0.58.6 -> 0.59.0; `libc` 0.2.190 -> 0.2.191; `unicode-normalization` 0.1.25 -> 0.1.26 (Dependabot bumps on `dev`) |
| Removed | `pdfium-render = "0.9"` from `[workspace.dependencies]` (no manifest references it any more; PDFium is planned as an out-of-process pack) |
| `deny.toml` | `MIT-0` added to the allowed licences. `borrow-or-share` (licence `MIT-0` in the local cargo registry source) is the crate that carries it; it was already in `Cargo.lock` at `v0.2.0`, so this allows an existing dependency rather than a new one. MIT-0 is OSI-approved and permissive, so it fits the AGENTS.md rule. |
| `fuzz/Cargo.lock` | 31 packages added (`jsonschema` 0.59 tree: `fancy-regex`, `fluent-uri`, `fraction`, `num*`, `email_address`, `strum*`, `uuid-simd`, `vsimd`, `micromap`, `ahash`, ...; plus `getrandom`, `r-efi`, `wasip2`, `wit-bindgen`, and the two new workspace crates). Licence fields read from the local cargo registry sources for 27 of the 29 third-party names (all but `jsonschema-regex` and `jsonschema-value`, which are released with `jsonschema`): all are MIT, MIT-0, Apache-2.0 or dual/triple permissive. `r-efi` is `MIT OR Apache-2.0 OR LGPL-2.1-or-later`, a choice that allows the permissive options. No GPL, AGPL or non-OSI-only licence found. `cargo deny` itself was not run (it needs a build tool). |
| Copyleft check | None of the added or bumped crates is known to be copyleft. `lopdf` (MIT) was already a dependency. |

The AGPL guard in AGENTS.md (PyMuPDF, pdf2docx, MinerU) is not touched.

## 6. Risks and open questions for the integrator and the maintainer

1. **crates.io token scope.** The `CARGO_REGISTRY_TOKEN` in the `release` environment is documented as `publish-update` limited to the four existing crates. `ariad-pdf` and `ariad-engine-io` do not exist on crates.io, so their first publish needs `publish-new`. Without it `custom-publish-crates` fails with 403 after `ariad-core` has already been published. The maintainer must confirm or rotate the token before the tag. (Documented in `docs/releasing.md`.)
2. **Desktop workflow also fires on the tag.** `desktop.yml` runs on `v*` tags and needs `tauri.conf.json` at 0.3.0. If its installers or `latest.json` fail, the latest release has no desktop updater metadata and in-app updates return 404 until fixed (releasing.md "Failure responses").
3. **Two known defects were reported by the structural-tools verification report** (`plans/reports/worker-p03-t6-argon.md`) and are expected to be handled by the separate fix wave; re-check before the tag: (a) `ariad-host` publishes JSON-emitting actions (`pdf.outline` export, `pdf.attachments` list and extract, `pdf.portfolio` list, `pdf.layers` list) with the tool's fixed `pdf` output format; (b) `--param pages=1` is parsed as a JSON number, which the `PageRange` string type rejects. Neither is mentioned in the release notes; add a Known limitations line if either remains.
4. **WASM size.** The same report measured `ariad_wasm_bg.wasm` at 8,352,779 bytes versus 6,750,968 at `v0.2.0` (+23.7%, +20.3% over phase 2), above the 15% alert. It does not affect the CLI, MCP or crates preview, but the web build ships it. Recheck after the fix wave.
5. **Release notes carry no PR numbers.** Earlier entries cite the pull request that merged each change; the phase pull request does not exist yet. Add the numbers on the bump PR (or close-out). The heading says `(unreleased)`; no placeholder token was used.
6. **SBOM wording.** `docs/releasing.md` still says CycloneDX SBOMs are produced for four crates. `release.yml` runs plain `cargo cyclonedx`, and the two new crates set `[package.metadata.dist] dist = false`, so the SBOM set may or may not include them. Not changed because it was not verifiable without a build; check `target/distrib/*.cdx.xml` in the rehearsal run.
7. **`ashift engines` sample output** in `docs/cli.md` shows `ariad-core 0.2.0`; refresh after the release (step k).
8. **Fuzz gate.** The phase acceptance asks for a 300 s clean run of `pdf_tools_structural` on Linux CI, with the run linked in the gate file. Only a compile check exists locally.
