# Pack gates: explicit macOS readiness and exported-wheel review

Rework 9 and its addendum are implemented together. Both Linux targets remain
mandatory; macOS remains wanted and requires native validation and an actual
video-disabled OpenCV release pin. Windows packs remain deferred. No push,
release/tag creation, workflow dispatch, GPL waiver or release omission was made.

## Source and report heads

- Starting report head: `0f9789f866097404b9618af1a5194a0f63b7ab4c`.
- Source commits and exact tested head: `149e4cb55dacc7fe1387a4d459bbd94c0a808129`,
  `fix(docling): guard pack readiness and native wheel exports`.
- The report closure commit is a successor to the tested source head and changes
  only report, plan-state and journal records.
- [Accepted preceding evidence](archive-261028-1731-pack-gates-linux-native.md)
  preserves the 805-file Linux review, all 14 macOS wheel refusals, cv2 necessity,
  GNU provenance and the implemented native macOS source route.

## CI behavior and release guard

Previously Scope always selected macOS for pack-builder changes and release
runs. With no authoritative macOS pin, `native_build.opencv()` attempted a native
source bootstrap; the pack job had no readiness skip. Manifest assembly also
unconditionally requested the macOS receipt. Thus a PR could not complete that
row before the separately authorized toolchain release supplied its wheel.

`ci_plan.py` now owns selection and assembly. Every run keeps Linux x86_64 and
Linux aarch64. A PR with no macOS pin omits that row and emits a warning plus a
`SKIPPED` summary with the exact reason:

> macOS pack requires the video-disabled OpenCV wheel from toolchain-wheels-v*; pin missing

There is no green macOS build claim. An existing but stale/malformed pin fails;
metadata validation is shared with the authoritative release downloader without
fetching or compiling during Scope. A current pin selects the macOS row.

Tag/manual runs fail before pack scheduling unless the missing macOS target is
explicitly declared. Tags read `[pack_release_skips]` for the exact
`pack-docling-v*` tag; manual runs use `allow_missing_targets`. Neither Linux
target, Windows nor unknown names may be listed. No omission is populated or
preapproved. Signing/publishing authorization remains separate.

Manifest assembly recomputes current policy and compares the Scope plan, then
requires exactly the selected build receipts, matching triples and component
files. It uses the existing real CLI manifest writer. `pack.toml` and
`SHA256SUMS` contain only completed selected targets/components; stale unselected
archives are excluded. `pack-ci-plan.json` accompanies the unsigned manifest.

## Exported-wheel audit and permissive input binding

The export audit refuses FFmpeg `libav*`/`libsw*`/`libpostproc*`, x264, x265,
Rubber Band, vid.stab and FDK library names even without licence markers, and
continues to refuse GPL/AGPL declarations and forbidden build configurations.
The image codec libavif remains accepted. This stricter name policy applies to
exported video-disabled toolchains; installed packs retain their reviewed LGPL
FFmpeg allowance.

The cached original macOS OpenCV 5.0.0.93 wheel is SHA-256 checked before the
three unchanged GPL dylibs are isolated into diagnostic ZIPs and passed through
`check_wheel_files`. All three must fail by name, independently of companions.
These diagnostic ZIPs are never exported as distribution wheels.

| Original object | SHA-256 |
| --- | --- |
| `cv2/.dylibs/libx264.164.dylib` | `7f02ead8b5987d7441c56521a80a3e894b44fa5f49a57f8dd515ac3b3f733564` |
| `cv2/.dylibs/libx265.215.dylib` | `860a3c77010264c498e781b599980f6bb8cdaab989b05ea1522844bab961bbac` |
| `cv2/.dylibs/librubberband.3.dylib` | `ac8aeb49e8c6a77c29086fb0696ee88faaa3f6a20363477a015c228fad5d5686` |

The following permissive entries now record the carrying locked-wheel name and
SHA-256 per applicable Linux target: `torch-native`, `torch-arm-compute`,
`torch-openblas`, `opencv-linux-aom`, `opencv-linux-avif`, `opencv-linux-png`,
`opencv-linux-vpx`, `opencv-linux-openssl`, `opencv-linux-drm`,
`opencv-linux-openblas`. These are reviewed wheel-input bindings plus installed
RECORD SHA-256/size checks, **not independent reviewed hashes of each permissive
object**. Same-version/path wheel hash changes, missing wheel entries or missing
target bindings fail and require renewed review. The parser cache is keyed by
actual lock bytes, so a same-process lock edit cannot reuse stale parsed data.
Source replacements retain their output hashes/probes; LGPL/GNU per-object
bindings are unchanged. AGENTS.md, ARCHITECTURE.md, the Decision Log, inventory
comments and the module threat-model text record these limits.

## Verification

Tests were added first. The initial red run ended with **17 failed, 19 passed,
69 deselected, 19 errors**: missing planner, name-only video acceptance and
missing lock-drift binding were reproduced. Marker-bearing cases already
rejected by the existing declaration check passed. Red log:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework9-red.log`.

Focused checks: **191 passed, 4 skipped**, Ruff, actionlint on both workflows,
zizmor 1.30.1 (zero findings), typos and whitespace checks passed. Log:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework9-focused.log`.

The exact final gate command is preserved in
[`d2120-t7-rework9-final-pack-gate.sh`](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework9-final-pack-gate.sh).
It was launched detached through:

```sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 bash /tmp/t7-rework9-final-pack-gate.sh
```

The runner uses nice 19, one build/test/native-math worker, pinned uv 0.12.23,
a clean detached verification worktree and the real cached CLI. It sets
`ASHIFT_BIN` before pytest, so the manifest integration test runs rather than
skipping. Gate results:

- Focused pack/licence regressions: **262 passed**, including all 23 CI-policy
  tests and the real CLI manifest/checksum integration.
- Whole `engines/docling/tests`: **356 passed, 4 skipped**.
- Three real cached GPL dylibs rejected individually by export family name;
  synthetic name-only, marker-bearing, nonfree and clean/libavif cases pass.
- All three original pywin32 312 wheels and real uv installations pass, with
  modified source/forged RECORD negatives refused; CP1252 and C-locale checks pass.
- Source-built x86_64 fixture: **256 native files = 54 named + 202 extensions**,
  unique licensed RECORD ownership, full notices, **7 GNU hash/ELF checks**.
- Full ARM Torch archive: **146 native objects** accepted; **148 ZIP payloads**
  in **216 hash-verified locked wheels** checked.
- Complete original Linux input inventories, freshly checked at this head:
  x86_64 **402 native / 200 named**, aarch64 **403 native / 201 named**, zero
  global or per-file findings; all 805 files have ownership/hash/notice reviews.
- Linux OpenCV: **31 native objects**, five declared LGPL FFmpeg objects per
  target, zero GPL/nonfree flags. Installed-pack LGPL allowance still passes.
- `uv lock --check`, `uv audit --frozen` (**112 packages**, no known
  vulnerabilities/adverse statuses), explicit text-encoding audit (**21 files,
  zero implicit encodings**), Ruff check/format (**12 files**).
- `just bench-test`: **75 passed**, including the bench licence tests. Every
  fixture generator ran twice: **135 files** byte-identical to each other,
  stored fixtures and manifest SHA-256 values.
- `actionlint` and `zizmor@1.30.1` on both pack/toolchain workflows: no findings;
  `typos`, `just lint-commits`, clean exact-head verification passed.

Current-head [805-file census](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework9-native-census.json)
SHA-256: `46dc3ebaba299f3a22975c002bdd234de124555490c5184d3a4a30ac020537e5`.
[Platform summary](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework9-platform-summary.json)
SHA-256: `a3ebe73423a8b717a42c7d2d79ea9b8fb9eecbe31b5e4281e09c0e1dae5ef74a`.
[GNU ABI evidence](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework9-gnu-symbol-tags.log)
and [source-built fixture inventory](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework9-total-native-inventory.json)
record this same source head. Legacy helper output names were copied to these
current-head paths, preserving the preceding report's evidence. Full notices
are also retained at `/home/vchun/Codes/t7-native-inventory/notices-rework9`.

Final log:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework9-final-pack-gate.log`.
Last line: `=== PACK GATE ALL GREEN HEAD=149e4cb55dacc7fe1387a4d459bbd94c0a808129`.

The four existing conditional native compiler/ABI probes remain skipped;
all new regressions, including real CLI manifest assembly, ran. Local review
found no remaining blocking issue in the requested delta. Report closure checks
cover typos, commit lint, clean Git state, tested first parent, and byte-identical
canonical/operations reports; closure log:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework9-report-closure.log`.
AgentWiki publish skipped; no external write was
made.

## Unsent master release request draft

Request GO to produce the authoritative macOS OpenCV pin from the reviewed
integration source. This text is prepared only; it has not been sent or run.

1. After review ACCEPT and explicit push/release authorization, integrate the
   tested source and record the exact integrated SHA. Confirm the local native
   source/cache key at that SHA; do not tag a report or an older toolchain source.
   Proposed exact tag: `toolchain-wheels-v0.2.0`. Read-only checks on 2026-10-28
   found neither this release nor its tag (release not found / ref HTTP 404).
   Recheck before creating it; do not reuse an occupied tag.
2. Optional native rehearsal after GO: dispatch `toolchain-wheels.yml` at the
   approved integration SHA/ref. **Inputs: none.** It builds artifacts but does
   not publish, even if dispatched at a tag: publish requires the tag-push event.
   For authoritative release assets, create the proposed tag at that exact
   approved SHA and push that tag only under explicit GO. That push triggers
   native Linux x86_64 and macOS arm64 builds plus ARM torch verification and
   checksum collation, then the GitHub Release publish job.
3. Expected artifacts: `toolchain-wheels-x86_64-unknown-linux-gnu`,
   `toolchain-wheels-aarch64-apple-darwin`, and the collated
   `toolchain-wheels-release`. The bundle contains the actual repaired macOS
   `opencv_python_headless-5.0.0.93-*.whl`, Linux replacement wheels, matching
   `opencv-build-<target>.json` / `torch-build-<target>.json` provenance and notice
   sidecars, `toolchain-pins.toml` and `SHA256SUMS`. The macOS build must prove
   cv2 import, IPP disabled, video I/O disabled, macOS 14 arm64 compatibility,
   the complete repaired-wheel export audit and actual output hashes. Native
   installed-pack inventory/notices and conversion/smoke remain required before
   this target is released; an export audit alone does not prove the final pack.
4. Download the collated artifact/release assets without altering their bytes.
   Run `sha256sum --check SHA256SUMS` in that directory. Render pins with:
   `uv run --no-project --python 3.14 python engines/docling/pack/licenses.py --render-toolchain-pins <dir> --release-tag toolchain-wheels-v0.2.0`.
   Review actual SHA-256, byte size, exact release URLs and source/options build
   key for every output. Commit the rendered `[toolchain]` target tables into
   `engines/docling/pack/pins.toml`; never invent a wheel hash/size or hand-edit
   `uv.lock`. Confirm the macOS build key matches current source/lock inputs,
   rerun selection and the native pack gate, and inspect all targets in the
   final manifest. Pin metadata alone does not change the OpenCV build key.
5. Rollback: on any build/audit/probe failure, create no authoritative pin and
   publish no Docling pack. The PR remains visibly macOS-skipped; a release
   remains blocked unless the master explicitly approves its omission. A failed
   or partial toolchain release must not supply pins. Keep any created tag
   immutable; fix source and request the next exact tag (`toolchain-wheels-v0.2.1`)
   instead of retagging or deleting assets without new GO. Quarantine partial
   published assets only under master authorization. No GPL wheel stripping or
   licence exception is a fallback.

## Remaining release prerequisites

Native macOS execution and actual output pinning remain pending outside this
Linux worker. The workflow changes make that state visible without claiming a
macOS pass or dropping a wanted release target. GitHub CI owns the full Rust
matrix; the scoped local gate follows the lead's explicit Rust exclusion.

Status: DONE_WITH_CONCERNS
