# Package metadata cap repair

Date: 2026-10-28 (Asia/Saigon).
Worktree: `/home/vchun/Codes/01-AriadShift-wt-i9-meta`.
Branch: `fix/core-package-meta-cap`.
Base: `47afeb445fee2692724e392f3ae1aad5fc7cd414`.
Implementation commit: `2d3e26f7f68a15cd5f94ec5c33c97e6b43785664`
(`fix(core): cap only rewritten package metadata entries`).

## Result

Moved the 64 MiB entry-size check after the raw-copy branch in
`crates/ariad-core/src/package_meta.rs`. Only rewritten DOCX core properties and
EPUB package XML are subject to that check. The bounded XML read remains intact.
The 256 MiB total uncompressed budget in `stamp_bytes` remains unchanged.

Added core module regressions for byte-identical copying of 65 MiB stored media
in both formats, rejection of oversized target XML, and acceptance at exactly
256 MiB with refusal one byte above it. The same aggregate sizes remain valid
through streaming `stamp_into`. Updated the host module's refusal fixtures to
oversize the properties XML instead of an unrelated entry.

Added `crates/ariad-cli/tests/package_meta_large_media.rs`. It uses the real host
conversion path and Pandoc to convert Markdown containing a 65 MiB data-URI PNG
under `Limits::local()`, then checks the embedded DOCX media byte for byte. It
derives its image from the existing licensed PNG fixture at runtime.

## Verification

Every compiling run used detached `setsid nohup zsh -lic` execution through
`/home/vchun/Codes/ariadshift-ops/gate-slot.sh 1`, which holds the build lock and
runs at nice 19. The scripts exported `CARGO_BUILD_JOBS=4`,
`MISE_UV_VERSION=0.12.23`, `MISE_PNPM_VERSION=12.9.1`,
`MISE_NODE_VERSION=$(cat .node-version)`, a worktree-local `CARGO_TARGET_DIR`, and
`ASHIFT_PANDOC=$PWD/.tools/pandoc/bin/pandoc`.

Before the production change, these commands ran against the base with the new
regressions present:

```sh
just pandoc
cargo nextest run -p ariad-core --lib -E 'test(package_meta::tests::)'
cargo nextest run -p ariad-cli --test package_meta_large_media
```

Pandoc installation passed. The core run had six passes and one expected
failure, `copies_large_stored_media_without_changing_bytes`, with
`EntryTooLarge`. The real conversion regression failed with `ConvertError::Failed`.
Both nextest commands returned 100. Log:
`/home/vchun/Codes/ariadshift-logs/i9-meta-base.log`.
Last line: `=== BASE REPRODUCED ALL GREEN`.

After the repair, both focused commands passed:

```sh
cargo nextest run -p ariad-core -p ariad-host --lib --test-threads 4 -E 'test(package_meta::tests::)'
cargo nextest run -p ariad-cli --test package_meta_large_media --test-threads 1
```

The module run passed all 11 tests. The conversion regression passed in 68
seconds. Log: `/home/vchun/Codes/ariadshift-logs/i9-meta-focused.log`.
Last line: `=== FOCUSED ALL GREEN`.

The final narrowed gate runs the following commands on the implementation commit:

```sh
cargo fmt --all --check
cargo clippy -p ariad-core -p ariad-host -p ariad-cli -p ariad-wasm --all-targets --features ariad-host/test-probe -- -D warnings
cargo nextest run -p ariad-core -p ariad-host -p ariad-cli -p ariad-wasm --features ariad-host/test-probe --build-jobs 1 --test-threads 4
typos crates/ariad-core/src/package_meta.rs crates/ariad-host/src/package_meta.rs crates/ariad-cli/tests/package_meta_large_media.rs
just lint-commits
git diff --check
```

Final gate outcome: all commands passed. Nextest passed all 870 selected tests
in 114.420 seconds (one slow test); 12 tests were skipped by the suite's normal
selection. Formatting, Clippy with warnings denied, spelling, commit lint, and
diff whitespace checks all passed.
Final gate log: `/home/vchun/Codes/ariadshift-logs/i9-meta-gate-final.log`.
Code gate completion line:
`=== 2d3e26f7f68a15cd5f94ec5c33c97e6b43785664 ALL GREEN`.

The first gate passed formatting and Clippy, but its concurrent test-binary
linking coincided with sustained host load above 24. Its owned process group
was terminated cleanly, verified absent, and the gate was queued again with
`--build-jobs 1`. No slot holder was suspended and no unrelated process was
stopped. The interrupted run is explicitly marked `STOPPED` in
`/home/vchun/Codes/ariadshift-logs/i9-meta-gate.log`.

## Review and limits

The scoped `ak-code-review` found no open correctness or scope findings. Public
signatures, cloud limits, dependencies, manifests, fixtures, and architecture
remain unchanged. No shared test file was edited. The regression restores the
documented local-limit behavior, so no evergreen documentation change is needed.
The requested four-crate gate is the validation boundary; full workspace,
desktop, browser end-to-end, and release gates are outside this brief.

The implementation commit above identifies the tested code. A report-only
commit follows, with its SHA recorded in the final handoff log marker after
report spelling and commit checks. Those final commands are
`typos plans/reports/d2120-i9-package-meta.md`, `just lint-commits`, and
`git diff --check`. Compiled checks are reused because source and build inputs
are unchanged. All task-owned background process groups have exited.
No push or pull request is authorized before
the lead's ACCEPT; neither has been performed.

Open questions: none.

Status: DONE
