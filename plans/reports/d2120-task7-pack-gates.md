# Pack gates: positive native identities before toolchain export

The export-boundary finding is fixed: every detected native payload must match
exactly one explicitly reviewed component SHA-256, an allowed SPDX licence and
all reviewed notices bundled with the artifact. Filename and marker checks are
additional refusals, never positive licence identities. No push, tag, release,
workflow dispatch, GPL waiver or stripping of an upstream distribution occurred.

## Heads and scope

- Starting report head: `13ea8b32b4d41598b005bf77f92834825cf74832`.
- Tested source head: `f63cca840b82f279d108c34db70e1fd6a2c283e2`.
- Source commit: `fix(docling): require reviewed identities for native wheel exports`.
- The report closure is a Markdown-only successor to that tested source head.
- [Previous report](archive-261028-1927-pack-gates-export-names.md) preserves
  the CI readiness policy, accepted Linux/native/GNU evidence and the unsent
  release request. Its filename-only export assurance is superseded here.

## What changed

`_scan_stream` hashes the complete original native bytes while scanning them.
The export checker identifies payloads through `export_objects` in the existing
native-component inventory. A neutral filename, image-codec filename, extension
suffix, caller-supplied OpenMP hash or generated output checksum cannot identify
an unknown object. Zero or multiple component matches fail. Unknown/GPL SPDX
expressions fail. Missing, empty, changed or escaping notice paths fail; duplicate
archive paths also fail. Existing Intel/video/GPL/configuration refusals remain.
The installed-pack LGPL/GNU policy and Linux wheel-input/RECORD bindings remain
separate and unchanged.

Detection covers ELF, PE, Mach-O, ar/thin archives and common COFF/LLVM bitcode
headers, including extensionless/neutral data names. The checker does not infer
licensing from a binary suffix. It requires the reviewed notice bytes inside
the wheel: original paths for wheel-scoped notices, `notices/<path>` for upstream
notices. Loose native sidecars require those files beside the artifact at the
reviewed relative paths. Notice identity is checked by SHA-256; this is not a
semantic licence-text parser.

Five initial export receipts bind already reviewed BSD-3-Clause OpenEXR/Imath
objects from the locked OpenCV 5.0.0.93 macOS wheel:
`libIex-3_3.32.3.3.4.dylib`, `libIlmThread-3_3.32.3.3.4.dylib`,
`libOpenEXR-3_3.32.3.3.4.dylib`, `libOpenEXRCore-3_3.32.3.3.4.dylib` and
`libImath-3_1.29.11.0.dylib`. Their exact hashes and complete notice receipts
are in `engines/docling/pack/native-components.toml`. These approve individual
BSD objects, never the containing GPL wheel or a new source-built wheel.

`test_exported_clean_image_libraries_remain_allowed` now requires an explicit
component/hash/licence/notice review. New cases cover unknown native content
under neutral names, altered/missing/ambiguous receipts, unknown/GPL licences,
notice absence/hash/path errors, loose artifacts and the OpenMP sidecar path.
AGENTS.md, ARCHITECTURE.md, its Decision Log and docs/docling.md describe the
boundary and the separate release prerequisites.

## Verification

Regressions were written first and run against the untouched accepted source
`149e4cb55dacc7fe1387a4d459bbd94c0a808129` in the owned verification worktree.
The baseline diagnostic is the reviewer's original script; it checks the three
unchanged SHA-256-verified real GPL dylibs under original and neutral names.
The baseline accepted all six neutral-name real GPL variants. The new test
module produced **49 failed, 5 passed**, confirming the missing positive identity
boundary; the log ends `=== EXPECTED RED REGRESSIONS rc=1`.
Red log: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework10-red.log`.

Focused checks: **272 passed, 4 skipped**; strict real-object export probe,
Ruff check/format, actionlint, typos and whitespace checks passed. Log:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework10-focused.log`.

The final scoped pack gate ran detached through slot 2/nice 19 with single-worker
build/native-math settings, pinned uv 0.12.23 and the real CLI:

```sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 bash /tmp/t7-rework10-final-pack-gate.sh
```

The complete command sequence is retained at
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework10-final-pack-gate.sh`.
Final results:

- CI scope fixtures and cloud selection classifier passed; a real current-head
  CLI was built and used by the manifest integration tests.
- Focused pack/licence suites: **314 passed**. Whole `engines/docling/tests`:
  **408 passed, 4 skipped**. All new regressions ran; the four existing
  conditional native compiler/ABI probes remain skipped.
- Three original pywin32 312 wheels and real uv installations pass; modified
  sources and forged installed RECORD files fail. CP1252 and C-locale probes pass.
- Source-built x86_64 fixture: **256 native files = 54 named + 202 extensions**,
  exactly one licensed RECORD owner per file, full notices and **7 GNU hash/ELF
  checks**, with readelf/objdump symbol evidence retained.
- ARM Torch archive: **146 native objects** accepted. **148 ZIP payloads** in
  **216 hash-verified locked wheels** checked.
- Fresh complete Linux input inventories: x86_64 **402 native / 200 named**;
  aarch64 **403 native / 201 named**. Both global and per-file checks have zero
  findings, covering all **805 native files**.
- Linux OpenCV: **31 native objects**; five declared LGPL FFmpeg objects per
  target and zero GPL/nonfree flags. The installed-pack LGPL rule is preserved.
- `uv lock --check`, `uv audit --frozen` (**112 packages**, no known
  vulnerabilities or adverse statuses), explicit encoding audit (**22 files**,
  zero implicit text encodings), Ruff check/format (**13 files**) passed.
- `just bench-test`: **75 passed**, including bench licence tests. All fixture
  generators ran twice: **135 files** byte-identical to each other, the stored
  fixtures and manifest SHA-256 values.
- `actionlint` and `zizmor@1.30.1` on both pack/toolchain workflows passed
  (zero unsuppressed findings); `typos`, `just lint-commits` and clean exact-head
  source verification passed. The full Rust matrix remains excluded locally
  under A-0352 and is covered by GitHub CI.

Gate log:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework10-final-pack-gate.log`.
Last line: `=== PACK GATE ALL GREEN HEAD=f63cca840b82f279d108c34db70e1fd6a2c283e2`.

The first detached full-gate launcher disappeared before runner entry and ran
no checks. Its stopped record is
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework10-launcher-stopped.log`.
After confirming no surviving duplicate, the same SHA was requeued in slot 2;
that completed run supplies every final result above. No slot holder was paused.

Fresh [805-file census](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework10-native-census.json)
SHA-256: `46dc3ebaba299f3a22975c002bdd234de124555490c5184d3a4a30ac020537e5`.
[Platform summary](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework10-platform-summary.json)
SHA-256: `a3ebe73423a8b717a42c7d2d79ea9b8fb9eecbe31b5e4281e09c0e1dae5ef74a`.
The census data matches the prior accepted bytes; both platform checks were
freshly executed at this source head. The
[source-built fixture inventory](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework10-total-native-inventory.json)
and [GNU ABI evidence](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework10-gnu-symbol-tags.log)
record this head. Full vendor notices are retained at
`/home/vchun/Codes/t7-native-inventory/notices-rework10`.

The strict real-object probe verifies the original locked wheel SHA-256, then
asserts refusal of x264, x265 and Rubber Band under each original name,
`libcodec.dylib`, `libavif.1.dylib` and `payload.data`: **12 refusals**. It also
asserts that all five reviewed real BSD objects fail when their full notices
are absent and pass with the complete hash-checked notices bundled. These ZIPs
are diagnostic fixtures only; the locked wheel is unchanged and no diagnostic
archive is exported or published.

Probe script:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework10-real-export-probe.py`.
Report closure log:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework10-report-closure.log`.
The closure checks commit lint, typos, tested first parent, clean Git state,
local evidence links and byte-identical canonical/operations reports.
AgentWiki publish skipped; the journal is local and no external write was made.

## Limits and remaining release prerequisites

Trusted review data records the component identity/licence and notice review;
candidate output checksums never generate approvals. A new or changed native
object, including a legitimate new source-build output, remains refused until
its identity, licence, complete bundled notices and exact bytes are reviewed and
recorded. The five BSD receipts are not a complete source-built OpenCV/PyTorch
wheel approval. Both toolchain publication callers scan every final export
file, including loose artifacts under neutral names, and propagate failures before
upload. Unreviewed source outputs stop there. No full source-built toolchain
export or native macOS validation is claimed by this result.

The earlier unsent tag request therefore also needs actual native output review
and committed export receipts before any authorized publication attempt.
Native macOS compilation, final installed-pack inventory/conversion/smoke and
an authoritative video-disabled OpenCV release pin remain pending. PR macOS
readiness stays visibly skipped while its pin is absent; both Linux pack targets
remain mandatory. Windows packs remain deferred. GitHub CI owns the full Rust
matrix, following the lead's scoped local-gate exclusion. No external action is
pending approval in this worker session; the release request remains unsent.

Status: DONE_WITH_CONCERNS
