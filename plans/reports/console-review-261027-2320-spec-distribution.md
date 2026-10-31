# Implementation specs for the distribution fixes (F0 to F11) on `feat/v1-distribution`

Date: 2026-10-27 (Asia/Saigon). Author: Mac console, read-only. Nothing was built, run, fetched or checked out.

Source of truth: `console-review-261027-2045-master-fix-plan-final.md` §4 group F. Supporting reports: `console-review-261027-1935-distribution.md` (DIST), `console-review-261027-2000-verify-distribution.md` (V-DIST, which overrides DIST), `console-review-261027-1935-security-desktop-supply.md` (SDS) and, for cross-checks only, `console-review-261027-2110-ci-justfile.md`. The I-group specs (`2045-dev-fix-specs.md`, `2045-dev-core-repro-tests.md`) are not repeated here. Where they touch the same files, this report says so.

Refs as read for this report:

| Ref | Head | Note |
|---|---|---|
| `origin/feat/v1-distribution` | `2e860da` | Unchanged since the master plan |
| `origin/dev` | `0b118f6` | One docs-only commit (review reports) on top of the plan's `e6bd29b` |
| Merge base | `e0f68a8` | The branch is 4 commits behind `dev` |

Spec labels below (F0, W-A and so on) exist only in this report. AGENTS.md and the review rules forbid them in code, test names and commit messages, so every proposed name and message describes the behaviour instead.

## 1. Verdict

None of the twelve items is stale. Ten can be implemented now, wholly or in part. F5 is fully blocked on OD-5. The owner-only steps are blocked: publishing `pack-docling-v0.2.0` and running the 1.0.0 conformance install (F3, OD-1/OD-2/OD-3), the release publication model (F4, OD-4), CLI build provenance (F9, repository visibility, OD-19b) and pinning the toolchain wheels (F7, needs a `toolchain-wheels-v*` tag).

## 2. Summary

| ID | Sev | Still valid | Branch | Files | Size | Worker |
|---|---|---|---|---|---|---|
| F0 | Medium | Yes. Today 11 paths change on both sides; step 5 adds the hardening and I2 overlaps | `feat/v1-distribution--int` (sub-branch) | `Cargo.toml`, `Cargo.lock`, `crates/ariad-cli/tests/docling_pack.rs`, `AGENTS.md`, `ARCHITECTURE.md`, `README.md`, `docs/releasing.md`, `docs/server.md`, `schemas/openapi.v1.json`, `.github/workflows/{release,publish-crates}.yml` | M | W-A (first, alone) |
| F1 | High | Yes (`THIRD_PARTY_LICENSES:14920-14925` still say `0.1.0`) | same | `THIRD_PARTY_LICENSES` | S | W-A |
| F2 | Medium | Yes (spec `info.version` is `1.0.0`; dev's SDK lock hashes the `0.1.0` spec) | same | `schemas/openapi.v1.json`, `packages/sdk/generator.lock.json`, `docs/releasing.md` (bump checklist), `fuzz/Cargo.lock` | S | W-A |
| F3 | High (release gate) | Partly. The code defect disappears when F0 takes dev's builder. The guard test, the resolve-time test and the doc rewrite are ready. Publishing and the conformance install are blocked | same | new `crates/ariad-host/tests/pack_release_compat.rs`; `docs/server.md:38` (W-A, during conflict resolution); `docs/release-notes.md:36` (W-F) | S (code) | W-B; owner part blocked |
| F4 | Medium | Yes | same | `.github/workflows/desktop.yml`, `scripts/desktop-release-sums.mjs`, `scripts/desktop-release-sums.test.mjs` | M | W-C; draft-then-publish blocked on OD-4 |
| F5 | Medium (OD-5) | Yes, blocked | — | `desktop.yml`, `ARCHITECTURE.md` | — | none |
| F6 | Medium | Yes (no doc names `PACK_MINISIGN_KEY`, `sign-release` or `publish-checksums`) | same | `docs/releasing.md`, `docs/desktop.md` | S | W-F; key wording follows OD-5 |
| F7 | Medium | Yes, but it has to be redone against the integrated dev | same | `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/SECURITY.md`, `docs/install.md`, `docs/release-notes.md` | M | W-F |
| F8 | Medium (unverified) | Yes (`toolchain-wheels.yml:357` on dev; `docs/packs.md:214`) | same, plus a dev PR (see proposal P-1) | `.github/workflows/toolchain-wheels.yml`, `scripts/check-release-workflow.sh`, `scripts/test-check-release-workflow.sh`, `docs/packs.md` | S | W-D (docs line: W-F) |
| F9 | Low | Partly. Dev already excludes the toolchain tags; pack tags still trigger the CLI release | same | `release.yml`, `images.yml`, guard script and its test | M | W-D; provenance blocked |
| F10 | Low | Yes | same | `apps/desktop/scripts/verify-release{,.test}.mjs` (W-E); `docs/install.md` (W-F); `fuzz/Cargo.lock` (W-A) | S | W-E |
| F11 | Medium | Yes | same | `desktop.yml` (W-C), `scripts/test-check-release-workflow.sh` (W-D), recorded runs | M | W-C, W-D, then an operator |

Worker grouping (disjoint file sets; W-A lands first, the rest run in parallel afterwards):

| Worker | Items | Files owned |
|---|---|---|
| W-A | F0, F1, F2, `server.md` part of F3, `fuzz/Cargo.lock` part of F10 | Every conflicted path, `THIRD_PARTY_LICENSES`, `schemas/openapi.v1.json`, `packages/sdk/generator.lock.json`, `fuzz/Cargo.lock`, `docs/releasing.md` (checklist only) |
| W-B | F3 tests | `crates/ariad-host/tests/pack_release_compat.rs` (new) |
| W-C | F4, F11 encrypted-key rehearsal | `.github/workflows/desktop.yml`, `scripts/desktop-release-sums.mjs`, `scripts/desktop-release-sums.test.mjs` |
| W-D | F8, F9, F11 guard mutations | `.github/workflows/{release,images,toolchain-wheels}.yml`, `scripts/check-release-workflow.sh`, `scripts/test-check-release-workflow.sh` |
| W-E | F10 verifier | `apps/desktop/scripts/verify-release.mjs`, `apps/desktop/scripts/verify-release.test.mjs` |
| W-F | Docs for F3, F4 (per OD-4), F6, F7, F8, F9, F10 | `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/{release-notes,install,releasing,desktop,packs,SECURITY}.md` |

## 3. Item specs

### F0: merge the integrated dev into the distribution branch (Medium, M, W-A)

**Current behaviour.**

- The branch is 4 commits behind `origin/dev` (`git log origin/feat/v1-distribution..origin/dev`: `0b118f6`, `e6bd29b`, `8782c49`, `68ac304`).
- Version break: the branch sets `[workspace.package] version = "1.0.0"` and bumps `ariad-core` and `ariad-host` to `1.0.0` (branch `Cargo.toml:7,16-17`). Dev adds `ariad-typst = { path = "crates/ariad-typst", version = "0.1.0" }` between those two lines (dev `Cargo.toml:17`). `crates/ariad-typst` inherits `version.workspace = true`, so under 1.0.0 the requirement `^0.1.0` no longer matches the path crate.
- 11 paths change on both sides: `Cargo.toml`, `Cargo.lock`, `crates/ariad-cli/tests/docling_pack.rs`, `AGENTS.md`, `ARCHITECTURE.md`, `README.md`, `docs/releasing.md`, `docs/server.md`, `schemas/openapi.v1.json`, `.github/workflows/release.yml`, `.github/workflows/publish-crates.yml`. Source: `git diff --name-only e0f68a8 origin/dev` intersected with the branch's diff. I could not run `git merge-tree` (it needs approval), so the exact conflict hunks are still unverified.
- At step 5, more overlaps are expected: hardening edits `commands_cli.rs`, `commands/doctor.rs`, `docs/{cli,desktop,server}.md` and the spec (H-8), and I2 edits `crates/ariad-host/src/convert.rs`. The branch touches all of these only for the Pandoc hint wording (`convert.rs:295`, `engines/mod.rs:84`, `protocol.rs:272-277`, `doctor.rs`, `pandoc_bin.rs`, `engines/pandoc.rs` and three CLI tests).
- If OD-1 picks "release v0.2.0 first", `release/v0.2.0--notes-fix` reaches dev before step 5. It bumps `Cargo.toml` to 0.2.0 and adds `docs/release-notes.md` (87 lines), which is also a new file on this branch. That gives a `Cargo.toml` conflict and an add/add conflict on `docs/release-notes.md`.

**Change (worker procedure).**

1. `git switch -c feat/v1-distribution--int origin/feat/v1-distribution`, then `git merge --no-ff origin/dev -m "chore: merge the integrated dev into the distribution branch"`. Merge commits skip the conventional-commit check, but `scripts/check-commits.sh:25-32` still rejects AI references.
2. Resolve each path by these rules:

| Path | Rule |
|---|---|
| `Cargo.toml` | Keep `version = "1.0.0"`. Keep all three workspace path dependencies at `1.0.0`: `ariad-core = { path = "crates/ariad-core", version = "1.0.0", default-features = false }`, `ariad-typst = { path = "crates/ariad-typst", version = "1.0.0" }`, `ariad-host = { path = "crates/ariad-host", version = "1.0.0", default-features = false }`. Keep every other dev line. |
| `Cargo.lock` | Do not hand-merge. Take dev's file (`git checkout origin/dev -- Cargo.lock`), then run `cargo metadata --format-version 1 >/dev/null`, which rewrites only the workspace crate versions. Do not run `cargo update`. |
| `crates/ariad-cli/tests/docling_pack.rs` | Take dev's hunk unchanged: the `assert_eq!(manifest.requires_ashift, ">=0.2.0, <2.0.0")` and the `CARGO_PKG_VERSION == "0.1.0"` guard. Never restore the branch's `>=0.1.0, <2.0.0` override, because it hides a wrong builder range. |
| `schemas/openapi.v1.json` | Take dev's file, then regenerate it in step 3. |
| `.github/workflows/publish-crates.yml` | The hunks do not overlap. The result must contain the branch's `id: registry-token` and its three step-level `if:` lines, and dev's `crates=("ariad-core" "ariad-typst" "ariad-host" "ariad-cli")`. |
| `.github/workflows/release.yml` | The hunks do not overlap. The result keeps dev's tag exclusions (`'!toolchain-torch-*'`, `'!toolchain-wheels-*'`) and all of the branch's formula verification. F9 later replaces the tag filter. |
| `docs/releasing.md` | Keep the branch's prose. Wherever a crate list was rewritten, keep `ariad-typst` in it: the `build-global-artifacts` SBOM bullet, the `Publish crates in dependency order` bullet and the `CARGO_REGISTRY_TOKEN` bullet. Then apply F2's checklist edit. |
| `docs/server.md:37-38` | Use the F3 wording (below). |
| `AGENTS.md:7`, `README.md`, `ARCHITECTURE.md` | Start from dev's text for any feature that dev now ships (cloud, hardening, security). Keep the branch's install and distribution content. In the ARCHITECTURE Decision Log, keep every row from both sides; the table is append-only. F7 then re-sweeps these files. |
| `docs/release-notes.md` (only if notes-fix landed) | Keep dev's released 0.2.0 section and put the branch's 1.0.0 section above it. |
| Step-5 hint-string hunks (`convert.rs`, `doctor.rs`, `engines/{mod,pandoc}.rs`, `pandoc_bin.rs`, `protocol.rs`, CLI tests) | Take the other side's logic. Re-apply the branch's wording "Install Pandoc 3.12 or newer (<4) or set ASHIFT_PANDOC" wherever the old `just pandoc` hint survives. |

3. Regenerate everything generated, in this order: Cargo lock (above), `just openapi`, F2's SDK steps, F1's `just licenses`, and F10's fuzz lock.

**Regression check (fails before, passes after).** No new test. These existing checks fail on a bad resolution:

- `cargo metadata --locked --format-version 1 >/dev/null` fails with "failed to select a version for `ariad-typst`" if `Cargo.toml` keeps `0.1.0` for `ariad-typst`.
- `git grep -n '"just pandoc' -- crates ':!crates/*/tests/*golden*'` must print nothing. V-DIST found only golden-test `expect()` messages left.
- `git grep -n 'requires_ashift = ">=0.1.0' -- crates/ariad-cli/tests/docling_pack.rs` must print nothing.

**Gate.** `cargo metadata --locked --format-version 1 >/dev/null && just ci && just desktop-ci`.

**Commit.** The merge commit above. Regeneration goes in separate commits (F1, F2).

**Risks.**

- The PR into `dev` must not use "Rebase and merge" while the branch contains this merge commit. GitHub would replay the branch commits without the conflict resolutions. #48, #53 and #51 appear as single "(#NN)" commits on dev, so they were probably squash-merged; squash is safe here. `docs/git-workflow.md:110` prefers rebasing over merging the base in, which contradicts the master plan's "merge the integrated dev". See question U-1.
- A hand-merged `Cargo.lock` or `THIRD_PARTY_LICENSES` can look right and still fail `--locked`.

### F1: regenerate `THIRD_PARTY_LICENSES` (High, S, W-A)

**Current behaviour.** Branch `THIRD_PARTY_LICENSES:14920-14925` lists `ariad-desktop`, `ariad-cli`, `ariad-core`, `ariad-host`, `ariad-server` and `ariad-wasm` at `0.1.0`. Dev's copy adds `ariad-typst 0.1.0` at `:14925`. `licenses-check` (dev `justfile:156-176`) regenerates the file and compares it byte for byte, and it is part of `static` (`justfile:179`). `dist-workspace.toml:36` ships the file inside every CLI archive.

**Change.** After F0, with `cargo-about` exactly `0.9.2` (`justfile:138`): run `just licenses`, then commit `THIRD_PARTY_LICENSES` and, if it changed, `apps/desktop/THIRD_PARTY_JS_LICENSES`. Never resolve this file by hand. If the web-notices fix from `2045-dev-fix-specs.md` (WEB-3, which adds `wasm32-unknown-unknown` to `about.toml`) has reached dev by step 5, the regeneration picks it up automatically.

**Regression check.** The existing `just licenses-check` fails before ("THIRD_PARTY_LICENSES is out of date") and passes after.

**Gate.** `just licenses-check && just static`.

**Commit.** `build: regenerate third-party notices for the 1.0.0 crates`

**Risks.** A different `cargo-about` version rewrites unrelated sections. The recipe refuses to run without the pinned version.

### F2: refresh the OpenAPI document and the SDK lock, and complete the bump checklist (Medium, S, W-A)

**Current behaviour.**

- The branch spec says `"version": "1.0.0"` (branch `schemas/openapi.v1.json:10`). The server derives it from `env!("CARGO_PKG_VERSION")` (`crates/ariad-server/src/api/mod.rs:201`).
- Dev's `packages/sdk/generator.lock.json:4` pins `spec_sha256` for dev's 0.1.0 spec. `check-drift.mjs:70-75` compares the hash, and `sdk-check` is in `static`. The merge applies without a textual conflict, so nothing warns.
- Dev's bump checklist (`docs/releasing.md:58-84`) lists only `Cargo.toml`, `Cargo.lock` and `THIRD_PARTY_LICENSES`.
- The branch also bumps `apps/desktop/src-tauri/tauri.conf.json:4`. `release-config.mjs` overrides it on tag builds, so it matters only for local builds.

**Change.**

1. `just openapi`, then `pnpm --filter @ariadshift/sdk generate`, `pnpm --filter @ariadshift/sdk typecheck`, `pnpm --filter @ariadshift/sdk test`, then `pnpm --filter @ariadshift/sdk lock`.
2. Edit `docs/releasing.md` §1:
   - Step 2: add the bullet ``- [`apps/desktop/src-tauri/tauri.conf.json`](../apps/desktop/src-tauri/tauri.conf.json) -> `"version": "X.Y.Z"` ``.
   - Step 3: say that the OpenAPI document, the SDK lock and `fuzz/Cargo.lock` also embed the workspace version, and extend the command block to:
     ```bash
     cargo check --workspace
     cargo metadata --manifest-path fuzz/Cargo.toml --format-version 1 >/dev/null
     just licenses
     just openapi
     pnpm --filter @ariadshift/sdk generate
     pnpm --filter @ariadshift/sdk lock
     ```
   - Step 5: change the `git add` line to `git add Cargo.toml Cargo.lock fuzz/Cargo.lock THIRD_PARTY_LICENSES schemas/openapi.v1.json packages/sdk/generator.lock.json apps/desktop/src-tauri/tauri.conf.json`.

**Regression check.** After F0 and before this fix, the existing `just sdk-check` fails with `sdk drift: spec_sha256 …`; afterwards it passes. `openapi_drift` (`crates/ariad-server/tests/openapi_drift.rs`) fails if the spec is not regenerated.

**Gate.** `just sdk-check && cargo nextest run -p ariad-server --test openapi_drift && just static`.

**Commits.** `build(schemas): regenerate the OpenAPI document and SDK lock for 1.0.0`, then `docs: list every generated version file in the release bump checklist`.

**Risks.** None beyond F0. Never hand-merge `generator.lock.json` (H-2).

### F3: the Docling pack range against a 1.0.0 host (High release gate; code S, W-B; owner part blocked)

**Current behaviour.**

- The branch builder writes `REQUIRES_ASHIFT = ">=0.2.0, <0.3.0"` (branch `engines/docling/pack/build.py:55`) under a 1.0.0 workspace. Dev already writes `">=0.2.0, <2.0.0"` (dev `build.py:58`, asserted by `engines/docling/tests/test_pack.py:32` and dev's `docling_pack.rs:30`). So F0, by taking dev's `build.py`, fixes the code.
- The index names Docling `0.2.0` (`crates/ariad-host/data/packs.toml:7`). The pack version comes from `engines/docling/pyproject.toml:3` (`version = "0.2.0"`, read by `build.py:98-100`).
- The range is checked at install (`install.rs:342`) and again on every resolve (`install.rs:718`) and health check (`install.rs:794`). The existing test `requires_ashift_mismatch_is_a_version_error` (`crates/ariad-host/tests/packs.rs:812-824`) covers install only.
- Stale docs after F0: branch `docs/release-notes.md:36` ("Packs published for 0.2.x declare `ashift >=0.2.0, <0.3.0` … fetch the pack built for 1.x") and branch `docs/server.md:38` ("requires host `>=0.2.0, <0.3.0`"). Dev's `docs/server.md:38` says "while this workspace is still 0.1.0", which is also wrong after the bump.
- No `pack-docling-v0.2.0` release is visible from local refs, and I could not query GitHub.

**Change.**

1. (W-A, while resolving `docs/server.md`) Replace the paragraph's second, third and fourth sentences with: "The pack manifest requires host `>=0.2.0, <2.0.0`, following the [frozen protocol compatibility policy](packs.md#compatibility), so this 1.0.0 workspace accepts it. If the signed pack assets are not published, this build fails at installation with an explicit signed-pack requirement; it never installs a substitute pack."
2. (W-F) Replace `docs/release-notes.md:36` with: "- **Docling packs keep working.** The Docling 0.2.0 pack declares `ashift >=0.2.0, <2.0.0`, so a 1.0.0 host installs and runs it unchanged ([pack compatibility](packs.md#compatibility)). Pack verification keys are unchanged."
3. (W-B) Add the test file below.
4. (Owner, **blocked** on OD-1, OD-2 and OD-3) Publish `pack-docling-v0.2.0` from the merged builder after I3 and I4, keep `packs.toml` at `0.2.0`, and run the 1.0.0 conformance install from `docs/packs.md:83-86`. Do not proceed without the owner.

**Regression test.** New file `crates/ariad-host/tests/pack_release_compat.rs`. `semver`, `tokio-util` and `toml` are regular dependencies of `ariad-host` (`crates/ariad-host/Cargo.toml:28,37,38`), so integration tests can use them. `minisign` is a dev-dependency used by `support`.

- `docling_builder_range_admits_this_host` fails before: on branch head `2e860da`, `<0.3.0` refuses `1.0.0`. It passes after F0.
- The other two tests guard current behaviour and pass both before and after.
- Do not land this file on `dev` while the workspace is `0.1.0`: dev's range `>=0.2.0` refuses 0.1.0, which is why dev's `docling_pack.rs` special-cases it.

```rust
//! Release consistency of engine packs. The Docling builder's `requires_ashift` must admit
//! the host being released, the embedded index must recommend the version the builder
//! produces, and an installed pack whose range excludes the running host must be refused
//! when it is resolved, not only when it is installed.

mod support;

use std::{fs, path::PathBuf};

use ariad_host::packs::{self, InstallRequest, InstallSource, PackError, PackHealth};
use tokio_util::sync::CancellationToken;

use support::{Key, PackBuilder, Server, context};

fn repository_file(path: &str) -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(path);
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("read {}: {error}", file.display()))
}

/// The string literal assigned to a top-level constant of a Python module.
fn python_string_constant(source: &str, name: &str) -> String {
    let prefix = format!("{name} = \"");
    let line = source
        .lines()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("{name} is not assigned a string literal"));
    line[prefix.len()..]
        .strip_suffix('"')
        .unwrap_or_else(|| panic!("{name} is not a plain string literal: {line}"))
        .to_owned()
}

#[test]
fn docling_builder_range_admits_this_host() {
    let range = python_string_constant(
        &repository_file("engines/docling/pack/build.py"),
        "REQUIRES_ASHIFT",
    );
    let requirement =
        semver::VersionReq::parse(&range).expect("REQUIRES_ASHIFT is a SemVer requirement");
    let host = semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    assert!(
        requirement.matches(&host),
        "the Docling pack builder writes requires_ashift = {range:?}, which refuses ashift {host}"
    );
}

#[test]
fn pack_index_recommends_the_docling_version_the_builder_produces() {
    let project: toml::Table =
        toml::from_str(&repository_file("engines/docling/pyproject.toml")).unwrap();
    let built = project
        .get("project")
        .and_then(|project| project.get("version"))
        .and_then(toml::Value::as_str)
        .expect("engines/docling/pyproject.toml has project.version");
    assert_eq!(packs::index_lookup("docling").unwrap().version, built);
}

#[test]
fn installed_pack_outside_the_host_range_is_refused_when_resolved() {
    let key = Key::generate();
    let source = support::temp();
    let data = support::temp();
    PackBuilder::standard("sample", "1.0.0").write(source.path(), &key);
    let server = Server::start();
    server.put_dir("pack", source.path());
    let ctx = context(data.path(), key.trusted());
    packs::install(
        &ctx,
        &InstallRequest {
            name: "sample".to_owned(),
            version: None,
            source: InstallSource::Manifest(server.url("pack/pack.toml")),
        },
        &CancellationToken::new(),
        &mut |_| {},
    )
    .expect("install");
    assert!(packs::resolve_in(&ctx, "sample").unwrap().is_some());

    // Leave the receipt as an older host would: a validly signed manifest whose range no
    // longer admits the running host.
    let receipt_path = data.path().join("packs/sample/1.0.0/receipt.toml");
    let mut receipt: toml::Table =
        toml::from_str(&fs::read_to_string(&receipt_path).unwrap()).unwrap();
    let mut manifest = packs::Manifest::parse(
        receipt
            .get("manifest")
            .and_then(toml::Value::as_str)
            .expect("the receipt embeds the manifest"),
    )
    .unwrap();
    manifest.requires_ashift = ">=99.0.0".to_owned();
    let text = manifest.to_toml().unwrap();
    let signature = key.sign(text.as_bytes(), "pack:sample version:1.0.0");
    receipt.insert("manifest".to_owned(), toml::Value::String(text));
    receipt.insert("signature".to_owned(), toml::Value::String(signature));
    fs::write(&receipt_path, toml::to_string(&receipt).unwrap()).unwrap();

    let error = packs::resolve_in(&ctx, "sample").unwrap_err();
    assert!(
        matches!(&error, PackError::IncompatibleAshift { requires, .. } if requires == ">=99.0.0"),
        "{error}"
    );
    assert_eq!(error.exit_code(), 5);
    assert!(
        matches!(
            packs::check(&ctx, "sample", "1.0.0"),
            PackHealth::Problem(problem) if problem.contains(">=99.0.0")
        ),
        "doctor and the desktop Engine packs tab must report the incompatibility"
    );
}
```

Verified against the code: `index_lookup` (`packs/mod.rs:29`), `Manifest::parse`/`to_toml` and the public `requires_ashift` field (`manifest.rs:199,214`), `resolve_in` (`mod.rs:413`), `check` and `PackHealth` (`install.rs:139`, re-exported at `mod.rs:30-33`), `PackError::IncompatibleAshift { requires, .. }` and its exit code 5 (`mod.rs:66-74,154`), `InstalledPack: Debug` (`mod.rs:335`, needed by `unwrap_err`), and `support::{Key::sign, PackBuilder::standard, Server::put_dir/url, context, temp}` (`tests/support/mod.rs`, which has `#![allow(dead_code)]`). `resolve_installed` does not verify the signature (`install.rs:707-736`), but `check` does (`:774-790`), which is why the test re-signs the manifest.

**Gate.** `cargo nextest run -p ariad-host --test pack_release_compat`, then `just ci`.

**Commits.** `test(host): tie the Docling pack range and index to the host version` (W-B). `docs: describe Docling pack compatibility for 1.0.0 hosts` (W-F; W-A's `server.md` change rides in the merge resolution).

**Risks.** `ariad-host` is published with `publish = true`, and the test reads files outside the crate. `cargo publish` does not run tests, but a crates.io consumer running `cargo test` on the tarball would fail. That is acceptable for a repository consistency test.

### F4: bind signed desktop assets to their build outputs (Medium, M, W-C)

**Current behaviour.**

- Each `release` matrix leg holds `contents: write` (branch `desktop.yml:139-140`). It runs `tauri-action` with `releaseId`, which uploads installers into the already public CLI release (`:266-284`).
- `sign-release` re-downloads whatever desktop-named assets the release has (`desktop.yml:342-346`, `desktop-release-sums.mjs:104-147`). It compares each download only with `asset.digest`, which GitHub records at upload time (`:139-142`), so a deleted and re-uploaded asset passes.
- `verify-release` checks only the three updater artifacts. `.dmg`, `.msi`, `.deb` and `.rpm` are never bound to a build output before the project key signs them (V-DIST 5, SDS 1).
- The release itself is created already published (`release.yml:389`), so installers are public before signing. Changing that is OD-4 and **blocked**.

**Change.**

1. `scripts/desktop-release-sums.mjs`:
   - Import `dirname` next to `join` from `node:path`.
   - Add a kind helper and use it in `isDesktopAsset`:
     ```js
     // The runner OS whose release job builds each installer or updater kind.
     export const KIND_OS = {
       ".AppImage": "Linux",
       ".deb": "Linux",
       ".rpm": "Linux",
       ".dmg": "macOS",
       ".app.tar.gz": "macOS",
       "-setup.exe": "Windows",
       ".msi": "Windows",
     };

     export function kindOf(name) {
       const artifact = name.endsWith(".sig") ? name.slice(0, -".sig".length) : name;
       return REQUIRED_KINDS.find((kind) => artifact.endsWith(kind));
     }

     export function isDesktopAsset(name, productName) {
       if (name === "latest.json") return true;
       return name.startsWith(productName) && kindOf(name) !== undefined;
     }
     ```
   - Add recording, reading and the binding check:
     ```js
     // Hashes the installers and updater artifacts a release job built, without copying them,
     // so the signing job can refuse release assets that changed after upload.
     export function recordBuild(bundleDir, file, name = productName()) {
       const files = localBundleFiles(bundleDir);
       const entries = selectAssets([...files.keys()], name).map((asset) => ({
         name: asset,
         digest: sha256File(files.get(asset)),
       }));
       mkdirSync(dirname(file), { recursive: true });
       writeFileSync(file, checksumFile(entries));
       return entries;
     }

     // Reads the `<runner-os>.sha256` files that the release jobs uploaded into one directory.
     export function readBuildDigests(dir) {
       const built = new Map();
       for (const entry of readdirSync(dir, { withFileTypes: true })) {
         if (!entry.isFile() || !entry.name.endsWith(".sha256")) continue;
         const digests = new Set();
         for (const line of readFileSync(join(dir, entry.name), "utf8").split("\n")) {
           if (line === "") continue;
           const match = /^([0-9a-f]{64}) {2}\S+$/.exec(line);
           if (!match) throw new Error(`Malformed build digest line in ${entry.name}`);
           digests.add(match[1]);
         }
         built.set(entry.name.slice(0, -".sha256".length), digests);
       }
       return built;
     }

     // Every installer and updater artifact on the release must be byte-identical to a file
     // that its own platform's release job built. GitHub's recorded digest only proves that a
     // download matches the current upload, so a replaced asset would otherwise be signed.
     // latest.json is merged on the release by all three jobs; verify-release checks it.
     export function requireBuiltDigests(entries, built) {
       const foreign = entries
         .filter(
           ({ name, digest }) =>
             name !== "latest.json" && !built.get(KIND_OS[kindOf(name)])?.has(digest),
         )
         .map(({ name }) => name);
       if (foreign.length > 0) {
         throw new Error(`Release assets differ from their build job's output: ${foreign.join(", ")}`);
       }
     }
     ```
   - Change `collectRelease(repository, tag, outDir, name = productName())` to `collectRelease(repository, tag, outDir, built, name = productName())`. Call `requireBuiltDigests(entries, built);` after the `entries` map and before `writeFileSync(join(outDir, SUMS_FILE), …)`, so a refused release writes no checksum file.
   - Replace the CLI block, so release mode cannot run without build digests:
     ```js
     const USAGE =
       "Usage: desktop-release-sums.mjs --from <bundle-dir> <out-dir> | " +
       "--record <bundle-dir> <digest-file> | --built <digest-dir> <out-dir>";

     if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
       const [mode, source, target, ...extra] = process.argv.slice(2);
       if (!source || !target || extra.length > 0) throw new Error(USAGE);
       let entries;
       if (mode === "--from") {
         entries = collectLocal(source, target);
       } else if (mode === "--record") {
         entries = recordBuild(source, target);
       } else if (mode === "--built") {
         entries = collectRelease(
           process.env.GITHUB_REPOSITORY ?? "",
           process.env.RELEASE_TAG ?? "",
           target,
           readBuildDigests(source),
         );
       } else {
         throw new Error(USAGE);
       }
       for (const { name, digest } of entries) console.log(`${digest}  ${name}`);
     }
     ```
   - Update the header comment to list the three modes.
2. `.github/workflows/desktop.yml`, in `release`, after "Verify the Windows installer signatures":
   ```yaml
         - name: Record the built desktop asset digests
           run: node scripts/desktop-release-sums.mjs --record target/release/bundle "$RUNNER_TEMP/desktop-digests/$RUNNER_OS.sha256"
         - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
           with:
             name: desktop-digests-${{ runner.os }}
             path: ${{ runner.temp }}/desktop-digests/
             if-no-files-found: error
             retention-days: 7
   ```
   In `sign-release`, before "Download desktop assets and write SHA256SUMS":
   ```yaml
         - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
           with:
             pattern: desktop-digests-*
             merge-multiple: true
             path: ${{ runner.temp }}/desktop-digests
   ```
   Change that step's `run:` to `node scripts/desktop-release-sums.mjs --built "$RUNNER_TEMP/desktop-digests" "$RUNNER_TEMP/desktop-release"`.
3. (**Blocked** on OD-4) Either move to draft-then-publish (which also changes the `releases/tags/{tag}` lookups, because they do not return drafts), or let W-F document the window in `docs/releasing.md`: "Installers are public from upload until `publish-checksums` attaches `SHA256SUMS`. If `sign-release` refuses a release, do not re-run it; delete the affected assets, re-run the failed desktop jobs and re-sign."

**Regression test.** Append to `scripts/desktop-release-sums.test.mjs`. It fails before, because the new exports do not exist and `collectRelease` signs a swapped `.msi`. It passes after. Add `chmodSync` and `existsSync` to the `node:fs` import, change the path import to `import { delimiter, join } from "node:path";`, and add `collectRelease`, `kindOf`, `KIND_OS`, `readBuildDigests`, `recordBuild` and `requireBuiltDigests` to the module import. This one test also delivers F11's "stubbed-`gh` test for `collectRelease`".

```js
const digestOf = (body) => createHash("sha256").update(body).digest("hex");

const builtBy = {
  Linux: [
    "AriadShift_1.0.0_amd64.AppImage",
    "AriadShift_1.0.0_amd64.AppImage.sig",
    "AriadShift_1.0.0_amd64.deb",
    "AriadShift-1.0.0-1.x86_64.rpm",
  ],
  macOS: [
    "AriadShift_1.0.0_aarch64.dmg",
    "AriadShift_1.0.0_aarch64.app.tar.gz",
    "AriadShift_1.0.0_aarch64.app.tar.gz.sig",
  ],
  Windows: [
    "AriadShift_1.0.0_x64-setup.exe",
    "AriadShift_1.0.0_x64-setup.exe.sig",
    "AriadShift_1.0.0_x64_en-US.msi",
  ],
};
const buildDigests = () =>
  new Map(Object.entries(builtBy).map(([os, names]) => [os, new Set(names.map(digestOf))]));

test("every installer kind belongs to exactly one platform's release job", () => {
  for (const names of Object.values(builtBy)) {
    for (const name of names) assert.ok(KIND_OS[kindOf(name)], name);
  }
  assert.equal(kindOf("latest.json"), undefined);
});

test("refuses release assets that differ from their platform's build output", () => {
  const entries = releaseNames.map((name) => ({ name, digest: digestOf(name) }));
  assert.doesNotThrow(() => requireBuiltDigests(entries, buildDigests()));

  const swapped = entries.map((entry) =>
    entry.name.endsWith(".msi") ? { ...entry, digest: digestOf("replaced") } : entry,
  );
  assert.throws(
    () => requireBuiltDigests(swapped, buildDigests()),
    /AriadShift_1\.0\.0_x64_en-US\.msi/,
  );

  // One platform's job cannot vouch for another platform's installer.
  const crossLeg = buildDigests();
  crossLeg.get("Linux").add(digestOf("replaced"));
  assert.throws(() => requireBuiltDigests(swapped, crossLeg), /\.msi/);

  const noWindows = buildDigests();
  noWindows.delete("Windows");
  assert.throws(() => requireBuiltDigests(entries, noWindows), /x64-setup\.exe/);
});

test("records a build tree's digests and reads them back per runner OS", () => {
  const root = mkdtempSync(join(tmpdir(), "desktop-release-record-"));
  try {
    const bundle = join(root, "bundle");
    mkdirSync(join(bundle, "msi"), { recursive: true });
    mkdirSync(join(bundle, "nsis"), { recursive: true });
    writeFileSync(join(bundle, "msi", "AriadShift_1.0.0_x64_en-US.msi"), "msi");
    writeFileSync(join(bundle, "nsis", "AriadShift_1.0.0_x64-setup.exe"), "nsis");
    writeFileSync(join(bundle, "nsis", "AriadShift_1.0.0_x64-setup.exe.sig"), "sig");
    const digests = join(root, "digests");
    recordBuild(bundle, join(digests, "Windows.sha256"), "AriadShift");
    const built = readBuildDigests(digests);
    assert.deepEqual([...built.keys()], ["Windows"]);
    assert.deepEqual(
      [...built.get("Windows")].sort(),
      [digestOf("msi"), digestOf("nsis"), digestOf("sig")].sort(),
    );
    writeFileSync(join(digests, "Linux.sha256"), "not a digest line\n");
    assert.throws(() => readBuildDigests(digests), /Malformed build digest line in Linux\.sha256/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// Runs `run(outDir)` with a POSIX `gh` stand-in first on PATH that serves one release.
function withStubbedGh(release, files, run) {
  const root = mkdtempSync(join(tmpdir(), "desktop-release-gh-"));
  const savedPath = process.env.PATH;
  try {
    const bin = join(root, "bin");
    const assets = join(root, "assets");
    mkdirSync(bin);
    mkdirSync(assets);
    writeFileSync(join(root, "release.json"), JSON.stringify(release));
    for (const [id, body] of Object.entries(files)) writeFileSync(join(assets, id), body);
    writeFileSync(
      join(bin, "gh"),
      [
        "#!/bin/sh",
        "# Stand-in for `gh api <path> [-H <header>]`.",
        'case "$2" in',
        `  */releases/tags/*) cat '${join(root, "release.json")}' ;;`,
        `  */releases/assets/*) cat '${assets}'/"\${2##*/}" ;;`,
        '  *) echo "unexpected gh call: $*" >&2; exit 2 ;;',
        "esac",
        "",
      ].join("\n"),
    );
    chmodSync(join(bin, "gh"), 0o755);
    process.env.PATH = `${bin}${delimiter}${savedPath}`;
    return run(join(root, "out"));
  } finally {
    process.env.PATH = savedPath;
    rmSync(root, { recursive: true, force: true });
  }
}

test(
  "collects a release only when every asset matches its build record",
  { skip: process.platform === "win32" && "the gh stand-in is a POSIX shell script" },
  () => {
    const release = {
      assets: releaseNames.map((name, id) => ({ id, name, digest: `sha256:${digestOf(name)}` })),
    };
    const files = Object.fromEntries(releaseNames.map((name, id) => [id, name]));
    withStubbedGh(release, files, (out) => {
      const entries = collectRelease("example/project", "v1.0.0", out, buildDigests(), "AriadShift");
      assert.equal(entries.length, releaseNames.length);
      assert.match(readFileSync(join(out, SUMS_FILE), "utf8"), / {2}AriadShift_1\.0\.0_x64_en-US\.msi\n/);
    });

    // Replaced after upload: GitHub records the new bytes' digest, so only the build record
    // catches it.
    const msi = releaseNames.indexOf("AriadShift_1.0.0_x64_en-US.msi");
    release.assets[msi].digest = `sha256:${digestOf("replaced")}`;
    files[msi] = "replaced";
    withStubbedGh(release, files, (out) => {
      assert.throws(
        () => collectRelease("example/project", "v1.0.0", out, buildDigests(), "AriadShift"),
        /differ from their build job's output: AriadShift_1\.0\.0_x64_en-US\.msi/,
      );
      assert.equal(existsSync(join(out, SUMS_FILE)), false);
    });

    // A corrupt download no longer matches what GitHub recorded.
    files[msi] = "corrupt";
    withStubbedGh(release, files, (out) => {
      assert.throws(
        () => collectRelease("example/project", "v1.0.0", out, buildDigests(), "AriadShift"),
        /differs from the digest GitHub recorded/,
      );
    });
  },
);
```

**Gate.** `node --test scripts/desktop-release-sums.test.mjs` (`desktop.yml:74` runs it on all three OSes), `just lint-workflows`, then `just desktop-ci`.

**Commits.** `ci: refuse to sign desktop assets that differ from their build outputs`. If OD-4 picks the documented window, the docs go in W-F's `docs(releasing): …`.

**Risks.**

- This assumes `tauri-action` uploads the bundle files byte for byte under new names; the existing fixture names (`AriadShift_1.0.0_aarch64.app.tar.gz`) show the renaming. If any byte changed, `sign-release` would refuse every release. That fails closed, but it is first exercised on the real tag (U-3).
- A compromised leg can still vouch for its own platform's outputs, because it built them. Splitting build and upload so builds never hold `contents: write` (ci-justfile report finding 18) is the stronger follow-up.

### F5: pack key custody (Medium, blocked on OD-5)

**Current behaviour.** `sign-release` reads `secrets.PACK_MINISIGN_KEY` and `PACK_MINISIGN_KEY_PASSWORD` (branch `desktop.yml:347-352`). This is the first workflow use of the only compiled-in pack trust anchor. `ARCHITECTURE.md:947` records "the project key" but not the move of pack signing into CI (V-DIST 6, SDS 2).

**Status.** Blocked on OD-5 (and OD-3). As instructed, this report proposes no key, environment or ruleset work. Once the owner decides, a worker adds one Decision Log row describing the chosen custody, and edits `desktop.yml` only if option (a), a separate desktop key, is chosen.

### F6: document the release signing secrets and jobs (Medium, S, W-F)

**Current behaviour.** `git grep -c PACK_MINISIGN_KEY origin/feat/v1-distribution -- docs README.md ARCHITECTURE.md AGENTS.md` finds nothing. Branch `docs/releasing.md:187-200` lists only `HOMEBREW_TAP_TOKEN` and `CARGO_REGISTRY_TOKEN`. Branch `docs/desktop.md:133-140` lists only the Tauri updater secrets.

**Change.** Write against the current single-key design. If OD-5 picks a separate desktop key, re-check the secret names and the public key.

1. `docs/releasing.md`, "The `release` environment and secrets": add a list **Required signing secrets for desktop releases** (the desktop workflow fails closed without them):
   - `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the updater key (`desktop.yml` "Require updater signing material", `:165-169`).
   - `PACK_MINISIGN_KEY`: the Minisign secret key, either the whole file written by `minisign -G` or only its second, base64 line (`scripts/desktop-release-sign.sh:31-35`).
   - `PACK_MINISIGN_KEY_PASSWORD`: the key's password, fed through stdin. Leave it empty only for an unencrypted key; an encrypted key with an empty password fails (`:47-52`).
   - The Apple (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`; notarization `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`) and Windows (`WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD`) groups are optional, but a partial group fails the release.
2. Under "Workflows", add a `desktop.yml` subsection: `release` (three legs, attach to the CLI release and, after F4, record digests), `verify-release` (updater signatures against `tauri.conf.json`), `sign-release` (`release` environment, read-only token, signs `SHA256SUMS` and every installer with the trusted comment `desktop:AriadShift release:vX.Y.Z file:<name>`) and `publish-checksums` (the only job with write access that touches signatures). Add what to do on failure: a failed leg or signing step leaves installers on the public release without `SHA256SUMS`. Fix the cause, then `gh run rerun <run-id> --failed`.
3. Under §3 (after the tag), add a post-release check:
   ```bash
   gh release download vX.Y.Z --pattern 'SHA256SUMS*' --dir /tmp/ariadshift-vX.Y.Z
   minisign -Vm /tmp/ariadshift-vX.Y.Z/SHA256SUMS -p crates/ariad-host/data/pack-keys/d87b0e33f55ea02b.pub
   ```
   Confirm that the printed trusted comment reads `desktop:AriadShift release:vX.Y.Z file:SHA256SUMS`.
4. `docs/desktop.md:133-140`: add one sentence that names `PACK_MINISIGN_KEY` and `PACK_MINISIGN_KEY_PASSWORD` and links to the releasing section.

**Regression check.** Docs only. `just static` runs `typos` and the formatters. Review the text against `desktop-release-sign.sh` and `desktop.yml`.

**Commit.** `docs(releasing): document desktop signing secrets and recovery`

**Risks.** Must follow OD-5.

### F7: re-sweep the docs against the integrated dev (Medium, M, W-F)

**Current behaviour (branch).**

- `docs/release-notes.md:7` keeps the "Coordinator: complete at merge time" block, and `:219` keeps a second "Coordinator: verify at merge time" block in the 0.2.0 section.
- `docs/install.md:16` lists a Windows `x86_64` Docling pack. Dev moved it to v1.x (`docs/docling.md:23`, `docs/packs.md:175`, issue #83). `docs/release-notes.md:227` (0.2.0 Known limitations) lists `x86_64-pc-windows-msvc` and the VC++ prerequisite, and so does `release/v0.2.0--notes-fix:docs/release-notes.md:78`.
- `docs/release-notes.md:221` says the toolchain wheels are "pinned by size and SHA-256", but dev's `engines/docling/pack/pins.toml:36-66` is still in bootstrap mode with no target pinned.
- `docs/install.md:12` says the Homebrew formula is "generated on every release", but tap publication is optional and skipped without `HOMEBREW_TAP_TOKEN`. The README leads with `brew install bavanchun/tap/ashift`.
- `docs/SECURITY.md:53` cites `plans/reports/security-audit-261025-v1.md` and `code-review-261025-phase7-final.md` "on `fix/v1-security-audit`", a branch that H1 closes unmerged.
- `AGENTS.md:7`, `README.md:11-16,151` and `ARCHITECTURE.md:3,186,944` describe Seatbelt, remote `/mcp`, file-manager integration and GHCR as shipped. That becomes true only after steps 2-4. Dev's `docs/self-hosting.md` now exists, so the links to it resolve after F0.

**Change (after F0, against the integrated dev).**

1. Delete both "Coordinator" blockquotes. Add the merged PR numbers to every 1.0.0 bullet, as the 0.2.0 entry does.
2. Remove the Windows Docling rows: `install.md:16` (keep Linux x86_64/arm64 glibc and macOS arm64, and add "Windows is planned for v1.x ([#83](https://github.com/bavanchun/AriadShift/issues/83))"), and the Windows triple and VC++ sentence in the release-notes Known limitations. If OD-1 lands `release/v0.2.0--notes-fix` first, apply the same correction to that section.
3. Reword `release-notes.md:221`: "Linux x86_64 toolchain wheels are built in this repository's CI. The pack builder pins them by size and SHA-256 once `toolchain-wheels-v0.2.0` is published (`engines/docling/pack/pins.toml`)." Pinning instead needs that tag (**blocked**, owner).
4. Mark the tap as conditional in `install.md:12` and in the README brew line: "published when the tap token is configured; otherwise use the formula attached to the GitHub Release".
5. `docs/SECURITY.md:53`: cite the audit reports at their path on `dev` once step 4 re-lands them. If they are not re-landed, drop the branch reference and keep the summary.
6. For each sentence in `AGENTS.md:7`, the README surface table and `ARCHITECTURE.md:3,186,944`, confirm the feature with one command on the merged tree and delete the sentence if the command finds nothing:
   - Seatbelt: `git grep -n sandbox-exec -- crates`
   - Remote MCP: `git grep -n '"/mcp"' -- crates/ariad-server`
   - File-manager integration: `git grep -n "Convert with AriadShift" -- apps/desktop`
   - GHCR images: `git grep -n ghcr.io -- .github/workflows/images.yml`
   - Confirm that the links resolve with `ls docs/self-hosting.md docs/install.md docs/release-notes.md`.

**Regression check.** Docs only. Run the grep checks above, then `just static`.

**Commit.** `docs: align release documentation with the integrated 1.0.0 tree`

**Risks.** H-8 (hardening edits the same files); do this after F0. Hardening's D27 and #84's B16/B17 also rewrite `ARCHITECTURE.md` earlier in the order.

### F8: keep toolchain and pack releases from becoming Latest (Medium, unverified, S, W-D)

**Current behaviour.**

- Dev `toolchain-wheels.yml:356-360` runs `gh release create "$TAG" …` without `--latest=false`.
- `docs/packs.md:214` tells the maintainer to publish pack assets on `pack-<name>-v<version>` with no Latest guidance.
- The desktop updater endpoint is `https://github.com/bavanchun/AriadShift/releases/latest/download/latest.json` (`apps/desktop/src-tauri/tauri.conf.json:54`), and the installer one-liners use `releases/latest/download/` (`README.md:25,31`).
- If GitHub ranks a toolchain or pack release as Latest, updates and installers return 404. How GitHub's automatic selection ranks non-SemVer tags is unverified.

**Change.**

1. `toolchain-wheels.yml`, inside the `gh release create` command, add the line `              --latest=false \` directly after `gh release create "$TAG" --repo "$GH_REPO" \`.
2. (W-F) `docs/packs.md:214`, step 5: "Create the release with `gh release create pack-<name>-v<version> --latest=false --title pack-<name>-v<version> pack.toml pack.toml.minisig <component files>`. Only `vX.Y.Z` app releases may be Latest, because the desktop updater and the installer scripts resolve `releases/latest`."
3. `scripts/check-release-workflow.sh`:
   - Pass a third argument to Python: `"$workflow" "$publish_crates" "${RELEASE_WORKFLOWS_DIR:-$repo_root/.github/workflows}"`.
   - Add a header line: "15. Every `gh release create` outside release.yml passes --latest=false."
   - Before the final `print("OK: …")`, add:
     ```python
     # --- 9. Only the app release may become the repository's Latest release ---
     # The desktop updater and the CLI installer one-liners resolve releases/latest, so a
     # toolchain or pack release marked Latest would break both.
     workflows_dir = sys.argv[3] if len(sys.argv) > 3 else ""
     if workflows_dir and os.path.isdir(workflows_dir):
         for entry in sorted(os.listdir(workflows_dir)):
             if not entry.endswith(('.yml', '.yaml')) or entry == 'release.yml':
                 continue
             other_path = os.path.join(workflows_dir, entry)
             try:
                 with open(other_path, 'r', encoding='utf-8') as wf:
                     other = yaml.safe_load(wf)
             except Exception as e:
                 fail(f"Failed to parse YAML in {other_path}: {e}")
             other_jobs = other.get('jobs') if isinstance(other, dict) else None
             for jname, job in (other_jobs or {}).items():
                 for step in ((job.get('steps') or []) if isinstance(job, dict) else []):
                     run = step.get('run') if isinstance(step, dict) else None
                     if not isinstance(run, str):
                         continue
                     for command in re.sub(r'\\\n\s*', ' ', run).splitlines():
                         if re.search(r'\bgh\s+release\s+create\b', command) and '--latest=false' not in command:
                             fail(f"{entry} job '{jname}' creates a release without --latest=false")
     ```

**Regression test.** Append to `scripts/test-check-release-workflow.sh`, before `=== Control Test (Semantic Equivalence) ===`. It fails before the guard change, because the mutation survives, and passes after:

```sh
echo ""
echo "=== Latest Release Ownership ==="

# A toolchain release that GitHub may mark as Latest
latest_dir="$tmpdir/workflows-latest"
mkdir -p "$latest_dir"
python3 - "$repo_root/.github/workflows/toolchain-wheels.yml" "$latest_dir/toolchain-wheels.yml" << 'PYEOF'
import sys
orig_path, mut_path = sys.argv[1:3]
with open(orig_path, 'r', encoding='utf-8') as f:
    orig = f.read()
old = '              --latest=false \\\n'
if old not in orig:
    sys.exit(f"Target string not found in toolchain-wheels.yml: {old!r}")
with open(mut_path, 'w', encoding='utf-8') as f:
    f.write(orig.replace(old, '', 1))
PYEOF
export RELEASE_WORKFLOWS_DIR="$latest_dir"
assert_mutation_fails "toolchain-release-may-become-latest" \
  "toolchain-wheels.yml creates a release without --latest=false" "$orig_workflow"
unset RELEASE_WORKFLOWS_DIR
```

The existing controls keep passing, because they scan the real workflow directory, where only `release.yml` and the fixed `toolchain-wheels.yml` call `gh release create` (`git grep "gh release create" -- .github/workflows`).

**Gate.** `sh scripts/test-check-release-workflow.sh && just check-release-workflow && just lint-workflows`.

**Commit.** `ci: keep toolchain releases from becoming the latest release`

**Risks.** Low. See proposal P-1: the flag and the `packs.md` line must be on dev **before** the owner publishes `pack-docling-v0.2.0` at step 0, which comes long before step 5.

### F9: release workflow hardening (Low, M, W-D)

**Current behaviour.**

- (a) The trigger is `'**[0-9]+.[0-9]+.[0-9]+*'` plus dev's `'!toolchain-torch-*'` and `'!toolchain-wheels-*'` (dev `release.yml:52-56`). `pack-docling-v0.2.0` still matches, while `pack-docling.yml:20` listens on `pack-docling-v*`, so every pack tag also starts the CLI release (SDS 7; ci-justfile report finding 2).
- (b) `brew style` runs only in `prepare-homebrew-formula`, which `needs: host`, so it never runs in PR rehearsals (DIST 9).
- (c) `announce` checks `publish-homebrew-formula` and `custom-publish-crates` but not `prepare-homebrew-formula`. A failed preparation leaves publishing skipped, and `announce` passes (branch `release.yml:504-514`).
- (d) `verify-homebrew.py` checks the cargo-dist formula before `brew style --fix` rewrites it. The pushed formula is never re-verified (SDS 6).
- (e) Tap commits are authored as `axo bot <admin+bot@axo.dev>` (branch `release.yml:446-447`).
- (f) The `images.yml` `publish` job (`packages: write`, `id-token: write`) has no `environment:` (dev `images.yml:219-228`, SDS 4).
- (g) CLI archives have no build provenance (SDS 5).

**Change.**

1. (a) Replace the three tag lines with one: `      - 'v[0-9]+.[0-9]+.[0-9]+*'`. Add header comment line 27: "Release trigger: only vX.Y.Z tags; pack and toolchain tags have their own workflows." `allow-dirty = ["ci"]` (`dist-workspace.toml:16`) permits the hand edit.
2. (b) In `build-global-artifacts`, after "Verify Homebrew formula and archive checksums":
   ```yaml
         - name: Check Homebrew formula style
           run: |
             export PATH="/home/linuxbrew/.linuxbrew/bin:$PATH"
             cp target/distrib/ashift.rb "$RUNNER_TEMP/ashift.rb"
             # The tag run fixes correctable offenses; only offenses it cannot fix fail here.
             brew style --except-cops FormulaAudit/Homepage,FormulaAudit/Desc,FormulaAuditStrict --fix "$RUNNER_TEMP/ashift.rb"
   ```
3. (d) In `prepare-homebrew-formula`, between "Format formula with brew" and "Upload prepared formula":
   ```yaml
         - name: Verify formatted formula
           run: |
             cp homebrew-tap/Formula/ashift.rb target/distrib/ashift.rb
             python3 scripts/release/verify-homebrew.py target/distrib
   ```
   The job already checks out the source and downloads `artifacts-*` into `target/distrib/`.
4. (e) `publish-homebrew-formula` env: `GITHUB_USER: "github-actions[bot]"` and `GITHUB_EMAIL: "41898282+github-actions[bot]@users.noreply.github.com"`, unless the owner names another project identity (P-3).
5. (c) `announce.if`: insert `(needs.prepare-homebrew-formula.result == 'skipped' || needs.prepare-homebrew-formula.result == 'success') && ` right after `needs.host.result == 'success' && `.
6. (f) `images.yml` `publish`: add `environment: release` under `needs: build`. Manual runs skip `publish` (`if: … github.event_name == 'push'`), so they never enter the environment.
7. Guard (`check-release-workflow.sh`). Add `'Verify formatted formula'` and `'Check Homebrew formula style'` to `SECURITY_STEPS` (`:183`), and add after the existing formula checks (`:352-359`):
   ```python
   triggers = data.get('on', data.get(True))
   push = triggers.get('push') if isinstance(triggers, dict) else None
   if not isinstance(push, dict) or push.get('tags') != ['v[0-9]+.[0-9]+.[0-9]+*']:
       fail("The release workflow must run only on vX.Y.Z tags")

   prepare_job = jobs.get('prepare-homebrew-formula') or {}
   prepare_names = [s.get('name') for s in (prepare_job.get('steps') or []) if isinstance(s, dict)]
   order = [prepare_names.index(n) if n in prepare_names else -1
            for n in ('Format formula with brew', 'Verify formatted formula', 'Upload prepared formula')]
   if min(order) < 0 or order != sorted(order):
       fail("The formatted formula must be verified after formatting and before its upload")
   verify_formatted = get_step_by_name(prepare_job, 'Verify formatted formula') or {}
   if (verify_formatted.get('run') or '').strip() != (
           'cp homebrew-tap/Formula/ashift.rb target/distrib/ashift.rb\n'
           'python3 scripts/release/verify-homebrew.py target/distrib'):
       fail("Verify formatted formula must re-run verify-homebrew.py on the formatted formula")
   if not get_step_by_name(jobs.get('build-global-artifacts') or {}, 'Check Homebrew formula style'):
       fail("PR rehearsals must check the formula style")

   announce_if = str((jobs.get('announce') or {}).get('if') or '')
   for upstream in ('prepare-homebrew-formula', 'publish-homebrew-formula', 'custom-publish-crates'):
       if f"(needs.{upstream}.result == 'skipped' || needs.{upstream}.result == 'success')" not in announce_if:
           fail(f"announce must not pass when {upstream} fails")

   pub_env = pub_job.get('env') or {}
   if (pub_env.get('GITHUB_USER'), pub_env.get('GITHUB_EMAIL')) != (
           'github-actions[bot]', '41898282+github-actions[bot]@users.noreply.github.com'):
       fail("Tap commits must use the project's bot identity")
   ```
   PyYAML reads the `on:` key as `True`; the guard already accounts for that at `:549`, and the lookup above handles both spellings. `pub_job` is defined at `:133`.
8. (g) **Blocked**: build provenance needs artifact attestations, which private repositories get only on GitHub Enterprise Cloud, and whether the repository is public is OD-19(b). Once that is settled, add to `host` an `actions/attest-build-provenance` step with `subject-path: artifacts/*` before "Create GitHub Release". Pin it to the commit SHA of the latest stable release, looked up on GitHub Releases rather than from memory (AGENTS.md "Versions"). Add `id-token: write` and `attestations: write` to `host`, and change the guard's expected map at `:153`. W-F then documents `gh attestation verify`.

**Regression test.** Append to `scripts/test-check-release-workflow.sh` in the F8 section. Each mutation survives before the guard change and fails after:

```sh
echo ""
echo "=== Release Trigger, Formula and Gate ==="

apply_replace "release-trigger-any-version-tag" "Release runs on pack and toolchain tags" \
  "      - 'v[0-9]+.[0-9]+.[0-9]+*'" \
  "      - '**[0-9]+.[0-9]+.[0-9]+*'"

apply_replace "formatted-formula-not-verified" "Formatted formula is not re-verified" \
  '      - name: Verify formatted formula' \
  '      - name: Check formatted formula'

apply_replace "rehearsal-skips-formula-style" "PR rehearsal skips the formula style check" \
  '      - name: Check Homebrew formula style' \
  '      - name: Style'

apply_replace "announce-ignores-formula-preparation" "announce passes after a failed formula preparation" \
  "(needs.prepare-homebrew-formula.result == 'skipped' || needs.prepare-homebrew-formula.result == 'success') && " \
  ""

apply_replace "tap-commits-third-party-identity" "Tap commits use a third party's identity" \
  '      GITHUB_USER: "github-actions[bot]"' \
  '      GITHUB_USER: "axo bot"'
```

`apply_replace` rejects a replacement only when the content would stay identical (`test-check-release-workflow.sh:80-101`), so the empty replacement in the fourth case is accepted.

**Gate.** `sh scripts/test-check-release-workflow.sh && just check-release-workflow && just lint-workflows`. Then a PR rehearsal of `release.yml`, which F11 records.

**Commits.**

- `ci: run the release workflow only for app version tags`
- `ci: verify the Homebrew formula after formatting and in rehearsals`
- `ci: fail the release gate when formula preparation fails`
- `ci: publish container images through the release environment`

**Risks.**

- The new tag filter drops cargo-dist's per-crate tag forms. The project uses only `vX.Y.Z` (`docs/git-workflow.md:115`).
- `brew style` in rehearsals runs without `brew update`, so its cops can lag the tag run's.
- The build job in `images.yml` still holds `packages: write` and pushes by digest without the environment. Only `publish` tags and signs.

### F10: verifier and lockfile follow-ups (Low, S, W-E; docs W-F; lock W-A)

**Current behaviour.**

- `docs/install.md:177` says `minisign -Vm` "checks … the signed trusted comment". Minisign only authenticates the comment; it never compares it with the expected release or file. `install.md:132` implies that all release artifacts carry `.minisig`, but CLI archives have only `.sha256` sidecars (DIST 8).
- `verifyRelease` (`verify-release.mjs:38-75`) downloads updater artifacts by asset id through authenticated `gh api` with an octet-stream header, never through `entry.url` the way the app does. tauri-action 1.0.0 writes API asset URLs into `latest.json` (DIST 11). Whether the updater sends `Accept: application/octet-stream` is unverified, because the crate source is not in the local registry.
- Dev `fuzz/Cargo.lock:92-93,129-130,170-171` records `ariad-core`, `ariad-host` and `ariad-typst` at `0.1.0`. After the bump that is stale; `cargo fuzz run` without `--locked` hides it (V-DIST).

**Change.**

1. (W-A) After F0: `cargo metadata --manifest-path fuzz/Cargo.toml --format-version 1 >/dev/null`, then commit `fuzz/Cargo.lock`. Gate: `cargo metadata --locked --manifest-path fuzz/Cargo.toml --format-version 1 >/dev/null`.
2. (W-E) `apps/desktop/scripts/verify-release.mjs`:
   - In `releaseTargets`, return `{ platform, asset, signature: entry.signature, url: entry.url }`.
   - Add:
     ```js
     // Downloads an updater artifact the way the app does: from the latest.json URL, without
     // credentials, asking for raw bytes. A JSON reply means the URL serves asset metadata
     // instead of the file, and in-app updates would fail.
     export async function downloadLikeUpdater(url, { fetchImpl = fetch, token } = {}) {
       const request = (headers) =>
         fetchImpl(url, { headers: { Accept: "application/octet-stream", ...headers }, redirect: "follow" });
       let response = await request({});
       // Runner addresses share the anonymous API quota; retry once with the workflow token.
       if (
         (response.status === 403 || response.status === 429) &&
         token &&
         response.headers.get("x-ratelimit-remaining") === "0"
       ) {
         console.log(`::notice::Anonymous download of ${url} was rate limited; retrying with the workflow token.`);
         response = await request({ Authorization: `Bearer ${token}` });
       }
       if (!response.ok) throw new Error(`Downloading ${url} failed with HTTP ${response.status}`);
       if ((response.headers.get("content-type") ?? "").includes("json")) {
         throw new Error(`${url} returned JSON instead of the updater artifact`);
       }
       return Buffer.from(await response.arrayBuffer());
     }
     ```
   - Make `verifyRelease` `async`. Replace `writeFileSync(artifact, download(target.asset))` with `writeFileSync(artifact, await downloadLikeUpdater(target.url, { token: process.env.GH_TOKEN }))`. Keep the `gh`-based `latest.json` download. Change the entry point to `await verifyRelease(process.env.GITHUB_REPOSITORY, process.env.RELEASE_TAG);`. Node is `24.21.0` (`.node-version`), so `fetch` and `Response` are built in.
3. (W-F) `docs/install.md:177`: replace the trusted-comment clause with: "`minisign -Vm` proves that the project key signed the file and its trusted comment. It does not check which release the comment names, so confirm that the printed `Trusted comment:` line reads `desktop:AriadShift release:v<version> file:<that file>`." In `install.md:132`, add: "CLI archives ship SHA-256 checksums but no Minisign signatures."

**Regression test.** Append to `apps/desktop/scripts/verify-release.test.mjs` and change its import to `import { downloadLikeUpdater, releaseTargets } from "./verify-release.mjs";`. It fails before, because the export does not exist, and passes after. `test:tools` (`apps/desktop/package.json:11`) runs it in `desktop.yml`'s check job.

```js
test("updater artifacts are fetched from their latest.json URLs as raw bytes", async () => {
  const calls = [];
  const fetchImpl = async (url, init) => {
    calls.push({ url, init });
    return new Response("artifact", {
      status: 200,
      headers: { "content-type": "application/octet-stream" },
    });
  };
  const url = "https://api.github.com/repos/example/project/releases/assets/1";
  const bytes = await downloadLikeUpdater(url, { fetchImpl });
  assert.equal(bytes.toString(), "artifact");
  assert.equal(calls[0].url, url);
  assert.equal(calls[0].init.headers.Accept, "application/octet-stream");
  assert.equal(calls[0].init.headers.Authorization, undefined, "the app sends no credentials");
  const { metadata, assets } = fixture();
  assert.deepEqual(
    releaseTargets(metadata, assets, "v1.2.3").map((target) => target.url),
    assets.map((asset) => asset.url),
  );
});

test("a JSON reply or an HTTP error never passes as an updater artifact", async () => {
  const json = async () =>
    new Response('{"id":1}', {
      status: 200,
      headers: { "content-type": "application/json; charset=utf-8" },
    });
  await assert.rejects(
    downloadLikeUpdater("https://example.invalid/a", { fetchImpl: json }),
    /JSON instead of the updater artifact/,
  );
  const missing = async () => new Response("", { status: 404 });
  await assert.rejects(
    downloadLikeUpdater("https://example.invalid/a", { fetchImpl: missing }),
    /HTTP 404/,
  );
});

test("a rate-limited anonymous download retries once with the workflow token", async () => {
  const seen = [];
  const fetchImpl = async (_url, init) => {
    seen.push(init.headers.Authorization);
    return seen.length === 1
      ? new Response("", { status: 403, headers: { "x-ratelimit-remaining": "0" } })
      : new Response("artifact", { status: 200 });
  };
  const bytes = await downloadLikeUpdater("https://api.github.com/x", { fetchImpl, token: "t" });
  assert.equal(bytes.toString(), "artifact");
  assert.deepEqual(seen, [undefined, "Bearer t"]);
});
```

**Gate.** `pnpm --filter @ariadshift/desktop test:tools`, then `just desktop-ci`.

**Commits.** `ci: verify updater artifacts through their latest.json URLs` (W-E). `build: refresh the fuzz lockfile for 1.0.0` (W-A). The install docs go in W-F's commit.

**Risks.** If the updater does not actually send the octet-stream header, this check passes while in-app updates fail. Only a real update from an installed app proves it (U-4).

### F11: release rehearsal (Medium, M, W-C and W-D, then an operator)

**Current behaviour.**

- The PR rehearsal signs with an unencrypted `-W` key only (branch `desktop.yml:82-95`). The production path, an encrypted key with its password fed through stdin (`desktop-release-sign.sh:47-52`), first runs on the real tag.
- `collectRelease` has no test.
- The new guard rules for the tap flag, the conditional tap steps, the Homebrew verification step, the registry-token skip and the `Push formula` notice are only exercised in their passing state. `test-check-release-workflow.sh` contains no mutation for any of them; its only publish-crates mutation is at `:939-944`.
- No recorded `just desktop-build` with verification and no recorded formula dry run exist.

**Change.**

1. `collectRelease` stub test: delivered by F4's last test.
2. (W-C) Encrypted-key rehearsal in `desktop.yml`'s `check` job, after "Rehearse desktop checksums and Minisign signatures". It uses a throwaway key created on the runner and deleted with the work directory. It never touches `PACK_MINISIGN_KEY`.
   ```yaml
         - name: Rehearse signing with a password-protected key
           if: runner.os == 'Linux'
           run: |
             work=$(mktemp -d "$RUNNER_TEMP/desktop-encrypted.XXXXXX")
             trap 'rm -rf "$work"' EXIT
             password=$(openssl rand -hex 24)
             printf '%s\n%s\n' "$password" "$password" | minisign -G -p "$work/rehearsal.pub" -s "$work/rehearsal.key" >/dev/null
             node scripts/desktop-release-sums.mjs --from target/release/bundle "$work/release"
             # The step above signs with a whole key file; this one feeds only the base64 line.
             MINISIGN_SECRET_KEY="$(sed -n 2p "$work/rehearsal.key")"
             export MINISIGN_SECRET_KEY
             if bash scripts/desktop-release-sign.sh "$work/release" v0.0.0 "$work/rehearsal.pub"; then
               echo '::error::An encrypted key was used without its password.'
               exit 1
             fi
             if MINISIGN_PASSWORD=wrong bash scripts/desktop-release-sign.sh "$work/release" v0.0.0 "$work/rehearsal.pub"; then
               echo '::error::A wrong key password was accepted.'
               exit 1
             fi
             rm -f "$work"/release/*.minisig
             MINISIGN_PASSWORD="$password" bash scripts/desktop-release-sign.sh "$work/release" v0.0.0 "$work/rehearsal.pub"
   ```
3. (W-D) Mutations for the publishing-channel rules. Append next to the F8 and F9 sections:
   ```sh
   echo ""
   echo "=== Optional Channel Publishing ==="

   apply_replace "tap-flag-exposes-token" "Tap availability flag carries the token value" \
     "TAP_PUBLISHING_ENABLED: \${{ secrets.HOMEBREW_TAP_TOKEN != '' }}" \
     "TAP_PUBLISHING_ENABLED: \${{ secrets.HOMEBREW_TAP_TOKEN }}"

   apply_replace "tap-checkout-unconditional" "Tap checkout runs without a token" \
     "          repository: \"bavanchun/homebrew-tap\"
           if: env.TAP_PUBLISHING_ENABLED == 'true'" \
     "          repository: \"bavanchun/homebrew-tap\""

   apply_replace "formula-verification-masked" "Formula verification failure is masked" \
     '        run: python3 scripts/release/verify-homebrew.py target/distrib' \
     '        run: python3 scripts/release/verify-homebrew.py target/distrib || true'

   apply_replace "push-formula-skip-without-notice" "Missing tap token is skipped silently" \
     'echo "::notice::HOMEBREW_TAP_TOKEN is not set; skipping the Homebrew tap push.' \
     'echo "HOMEBREW_TAP_TOKEN is not set; skipping the Homebrew tap push.'

   apply_publish_crates_replace "registry-token-missing-reports-available" \
     "Missing registry token reports itself available" \
     '            echo "available=false" >> "$GITHUB_OUTPUT"' \
     '            echo "available=true" >> "$GITHUB_OUTPUT"'

   apply_publish_crates_replace "publish-crates-ignores-availability" \
     "Crate publishing runs without a registry token" \
     "      - name: Publish crates in dependency order
           if: steps.registry-token.outputs.available == 'true'" \
     "      - name: Publish crates in dependency order"
   ```
   Every `old_str` above exists verbatim in the branch files. The tap-checkout mutation replaces the first `if: env.TAP_PUBLISHING_ENABLED == 'true'`, which belongs to the tap checkout step (branch `release.yml:451-455`). Expected failures: guard lines `:138`, `:141-142`, `:353`, `:358`, `:532` and `:536`. The Python `str.replace` with count 1 also matches the multi-line `old_str` values because the indentation is copied from the files.
4. (Operator, no keys or tags needed)
   - **On a Mac**: run `just desktop-build`, then `node scripts/desktop-release-sums.mjs --from target/release/bundle "$TMPDIR/out"` and `node scripts/desktop-release-sums.mjs --record target/release/bundle "$TMPDIR/digests/macOS.sha256"`. Check that every digest in `macOS.sha256` appears in `$TMPDIR/out/SHA256SUMS`. Sign with a throwaway key (`minisign -G -W`, then `desktop-release-sign.sh "$TMPDIR/out" v1.0.0 <throwaway.pub>`). Record the output in a `plans/reports/` worker report.
   - **On CI**: a PR rehearsal of `release.yml` (which runs `verify-homebrew.py` and, after F9, the style check) and of `desktop.yml` (both rehearsal steps). Record the run URLs. Pushing the PR needs the owner's go-ahead (AGENTS.md "Git").

**Regression test.** Parts 1-3 above; each fails before its change. Part 4 is recorded evidence.

**Gate.** `node --test scripts/desktop-release-sums.test.mjs`, `sh scripts/test-check-release-workflow.sh`, `just lint-workflows`, then the recorded runs.

**Commits.**

- `ci: rehearse desktop signing with a password-protected key` (W-C)
- `test: cover the optional publishing rules of the release guard` (W-D)

**Risks.**

- `minisign -G` reading the password from piped stdin is unverified. The signing script already relies on the same reader for `-S`, and a failure is loud.
- None of the rehearsals exercises `tauri-action` uploading into a real release (U-3).

## 4. Owner decisions needed

| Ref | Needed for | Note |
|---|---|---|
| OD-1 | F0, F3, F7 | If v0.2.0 ships first, `release/v0.2.0--notes-fix` lands on dev and F0 gains a `Cargo.toml` conflict and an add/add `docs/release-notes.md` conflict (rules in F0). |
| OD-2, OD-3 | F3 | Publishing `pack-docling-v0.2.0` and the 1.0.0 conformance install. Blocked; owner only. |
| OD-4 | F4 step 3 | Draft-then-publish, or document the window (text in F4). |
| OD-5 | F5, F6 | Key custody. F6's secret names and public key follow it. |
| OD-13 | F7 | Confirms the Windows Docling deferral (#83) that F7's wording assumes. |
| OD-19(b) | F9 (g) | Repository visibility decides whether artifact attestations are available. |
| P-1 (proposal, new) | F8, F9 | Land three dev-safe edits on `dev` now, as one small PR, before the owner pushes any `pack-docling-v*` or `toolchain-wheels-v*` tag at step 0: `--latest=false` in `toolchain-wheels.yml`, the `docs/packs.md:214` step, and the `v[0-9]+.[0-9]+.[0-9]+*` trigger in `release.yml`. The distribution branch does not touch those hunks, so F0 stays clean. The guard rules and mutations stay with W-D at step 5. Without this, step 0 can mark the pack release as Latest, and it starts a red CLI release run. |
| P-2 (proposal, new) | F0 | Merge method for the distribution PR: squash, or a merge commit. "Rebase and merge" would drop the F0 conflict resolutions. |
| P-3 (proposal, new) | F9 (e) | Tap commit identity: `github-actions[bot]` or a project-owned bot. |

## 5. Unresolved questions

1. **U-1.** `docs/git-workflow.md:110` says "Rebase onto the base branch instead of merging it in", while the master plan says to merge the integrated dev into the branch (F0). Which applies to this phase branch? Rebasing 34 commits onto a dev that has absorbed three phases means re-resolving the same conflicts in many commits.
2. **U-2.** The exact F0 conflict set. `git merge-tree` needed approval in this session, and the step-5 dev does not exist yet.
3. **U-3.** Does `tauri-action` 1.0.0 upload bundle files byte for byte? F4 assumes so. If not, `sign-release` refuses every release, which fails closed but is first seen on the tag.
4. **U-4.** Does `tauri-plugin-updater` 2.14.0 send `Accept: application/octet-stream` when it downloads `entry.url`? F10's verifier assumes so. The crate source was not available locally.
5. **U-5.** Was any `pack-docling-v0.2.0` asset ever published with a `<0.3.0` manifest? (OD-2; needs network.)
6. **U-6.** How does GitHub's automatic Latest selection rank `pack-docling-v0.2.0` and `toolchain-wheels-v0.2.0` against `v0.1.0`? F8 removes the question by never letting them compete.
7. **U-7.** Does `dist host --steps=create --tag=pack-docling-v0.2.0` fail at plan, or does it get further? F9 (a) stops the run either way.
