# Worker Report: `a-v02-notes-stale` (Gemini 4 Argon High)

## Summary

Worked in `/home/vchun/Codes/01-AriadShift-wt-v02-notes` on branch `release/v0.2.0--notes-fix` (cut from `origin/release/v0.2.0--notes` at `a160dcb`). Audited every `0.1.0` match across the repository, updated the three stale product-version references (`README.md`, `docs/server.md`, `crates/ariad-cli/tests/docling_pack.rs`) to match the `0.2.0` bump (`1c3d0c3`), and prepared the PR #51 entry under `### Changed` in `docs/release-notes.md` while keeping the `## 0.2.0 (unreleased)` heading and leaving `THIRD_PARTY_LICENSES` untouched for the gate worker.

## Commits

- `54d46fc` — `test(cli): expect 0.2.0 compatibility in the Docling pack install` (`crates/ariad-cli/tests/docling_pack.rs`)
- `c34babe` — `docs: update stale 0.1.0 references and the #51 release note entry` (`README.md`, `docs/server.md`, `docs/release-notes.md`)
- Plus the commit recording this report (`docs(plans): add the stale 0.1.0 and #51 release notes report`).

## Audit of `rg -n '0\.1\.0'` hits

| Location | Decision | Reason |
|---|---|---|
| `README.md:40` | **Changed** | Referred to the current release's package-channel publication status (`v0.1.0 has not reached them yet`); updated to `v0.2.0`. |
| `docs/server.md:38` | **Changed** | Stated `while this workspace is still 0.1.0` next to the Docling pack manifest's `>=0.2.0, <0.3.0` requirement. Updated to reflect that this workspace is now `0.2.0` and only signed pack asset publication remains gated before tagging. |
| `crates/ariad-cli/tests/docling_pack.rs:30,32` | **Changed** | Had a temporary workaround rewriting `manifest.requires_ashift` to `">=0.1.0, <0.3.0"` while the source tree was `0.1.0`. `engines/docling/pack/build.py:55` writes `REQUIRES_ASHIFT = ">=0.2.0, <0.3.0"` and `crates/ariad-host/src/packs/manifest.rs:291` compares against `env!("CARGO_PKG_VERSION")` (`0.2.0`), so the test now asserts `manifest.requires_ashift == ">=0.2.0, <0.3.0"` and signs the unmodified `pack.toml`. |
| `docs/release-notes.md:12,88,90,92` | **Left** | Historical references to the `0.1.0` release (EPUB identifier comparison with `0.1.0`, `0.1.0` package-channel note, and the `## 0.1.0 (2026-10-21)` section). |
| `AGENTS.md:7` | **Left** | Historical statement (`Roadmap 1a (v0.1.0) is shipped.`). `v0.2.0` is not tagged yet, and `AGENTS.md` is outside this task's file ownership. |
| `docs/cli.md:321,367,440` | **Left** | Multi-version engine pack examples showing active `0.2.0` alongside an older installed `0.1.0` pack version (`"versions": ["0.2.0", "0.1.0"]`, `"removed": ["0.2.0"], "active": "0.1.0"`). |
| `docs/ci.md:96` | **Left** | Explains why `just package-check` patches crates.io with local path dependencies rather than resolving the previously published `0.1.0` crates on crates.io. |
| `ARCHITECTURE.md:424` | **Left** | Protocol wire-format example (`"engine":"pandoc","version":"0.1.0"`), matching the unit test fixture in `crates/ariad-core/src/protocol.rs:248`. |
| `crates/ariad-core/data/capabilities.json:7` | **Left** | Benchmark provenance (`"ashift_version": "0.1.0"`), recording which binary produced the committed measurements. |
| `bench/tests/test_cli.py:261,343,445` | **Left** | Synthetic unit-test fixtures for the benchmark CLI. |
| `apps/web/test/converter-app.test.tsx:17` | **Left** | Mock worker `init` response in `FakeWorker`; not compared against the workspace version. |
| `scripts/winget-manifest.sh:31`, `scripts/test-winget-manifest.sh:24,33,52,67,77,78` | **Left** | Version format example in an error message (`e.g. 0.1.0`) and unit-test inputs for `winget-manifest.sh`. |
| `scripts/check-crypto-bigint.test.mjs:9,12,20,21,22,23` | **Left** | Hardcoded sample `cargo tree` strings in a unit test (`scripts/check-crypto-bigint.mjs` matches `v\S+`). |
| `crates/ariad-core/src/protocol.rs:248,345`, `crates/ariad-host/src/bin/engine-probe.rs:177,180`, `crates/ariad-host/src/convert.rs:2328`, `crates/ariad-server/tests/isolated.rs:334` | **Left** | Synthetic engine `version` strings in mock/probe capabilities JSON fixtures. |
| `crates/ariad-host/src/packs/mod.rs:524`, `crates/ariad-host/src/packs/manifest.rs:514`, `crates/ariad-host/tests/support/mod.rs:312`, `crates/ariad-host/tests/packs.rs:987`, `crates/ariad-host/tests/pack_schema.rs:59`, `crates/ariad-cli/tests/packs_cli.rs:65`, `crates/ariad-cli/tests/docling_routes.rs:242` | **Left** | Synthetic pack versions or lower-bound SemVer requirements (`requires_ashift = ">=0.1.0"`, which matches `0.2.0`) in unit and integration test fixtures. |
| `Cargo.lock:930,2359,3173,3220,4783,9137,9170,9182,9436,10323` | **Left** | Third-party Rust crates at version `0.1.0` (`byteorder-lite`, `fearless_simd_macros`, `hayro-cmap`, `hayro-postscript`, `num-cmp`, `unicode-math-class`, `unicode-vo`, `unscanny`, `vswhom`, `xmlwriter`). |
| `fuzz/Cargo.lock:93,130,171` (plus third-party crates at `340,983,1324,1371,4177,4210,4222,4872`) | **Left** | `fuzz/` has its own independent lockfile where `ariad-core`, `ariad-host` and `ariad-typst` are still `0.1.0`. Left untouched per file-ownership rules; noted below for the gate worker. |
| `THIRD_PARTY_LICENSES:14920-14926` (plus third-party crates at `10268,14951,14959,14963,15075,15076,19789,20180,20215`) | **Left** | Per brief: do not touch `THIRD_PARTY_LICENSES`; the gate worker runs `just licenses`. |
| `plans/**` | **Left** | Historical plan documents and earlier worker reports. |

## PR #51 line in `docs/release-notes.md` (**to be re-verified at merge**)

Replaced the temporary `### Docling pack without Intel components` section and its internal coordinator blockquote with a two-sentence bullet under `### Changed` (`docs/release-notes.md:55`), based on `gh pr view 51` and `git show origin/fix/docling-drop-intel-ipp:docs/docling.md`:

```markdown
- **Docling pack without proprietary Intel components.** x86_64 Docling packs exclude proprietary Intel IPP, oneMKL and Intel OpenMP binaries: OpenCV is built from source without IPP, x86_64 CPU PyTorch is built with static OpenBLAS and Apache-2.0 oneDNN, and ARM packs keep the audited official wheels. LLVM OpenMP is the only OpenMP runtime (with MSVC compatibility wrappers on Windows), Windows native builds use MSVC via Ninja, and a binary and notice gate rejects proprietary Intel artifacts on every target. (#51)
```

- **To be re-verified at merge:** PR #51 (`fix/docling-drop-intel-ipp`, `ef61b1c`) is still in CI and in draft state. This entry makes no claim about the Windows torch job passing or toolchain release wheels already being published. Re-verify the exact wording against the final merged state of #51 before tagging `v0.2.0`.

## Commands run and outcomes

This task is build-free (no `cargo`, `pnpm`, `uv`, `docker` or compiling `just` recipes were run; no gate log was produced):

- `rg -n '0\.1\.0'` and `rg -n '0\.1\.0' -g '!THIRD_PARTY_LICENSES' -g '!plans/**'` — enumerated all `0.1.0` occurrences across the repository.
- `gh pr view 51` and `git show origin/fix/docling-drop-intel-ipp:docs/docling.md` — inspected the PR #51 description and Docling pack documentation.
- `zsh -lic 'typos README.md docs/server.md docs/release-notes.md crates/ariad-cli/tests/docling_pack.rs plans/reports/a-v02-notes-stale-argon.md'` — exited `0` (no spelling errors).
- `for sha in $(git rev-list --no-merges origin/dev..HEAD); do ~/Codes/01-AriadShift/.tools/bin/committed --config committed.toml "$sha"; done` — exited `0` (all non-merge commits since `origin/dev` conform to `committed.toml`).

## Commands the gate worker must run

```bash
just licenses
cargo check --workspace --locked
cargo test -p ariad-server --test openapi_drift
just ci
just check-release-workflow
just package-check
just lint-commits
```

## Open concerns

1. **PR #51 release-note line (`docs/release-notes.md:55`) is to be re-verified at merge.** Confirm that the final merged changes in #51 match this bullet before tagging `v0.2.0`.
2. **`THIRD_PARTY_LICENSES` still lists `ariad-* 0.1.0` (`lines 14920–14926`).** Left untouched as instructed; `just licenses` in the gate pass will update it.
3. **`fuzz/Cargo.lock` still records `0.1.0` for `ariad-core`, `ariad-host` and `ariad-typst` (`lines 93, 130, 171`).** The gate worker or coordinator may refresh `fuzz/Cargo.lock` (`cargo check --manifest-path fuzz/Cargo.toml`) if lockfile parity in `fuzz/` is desired before tagging.

Status: DONE_WITH_CONCERNS
