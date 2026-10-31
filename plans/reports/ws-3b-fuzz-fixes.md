# Fuzz repairs and shared package stamping protection

Branch: `feat/v1-hardening--fuzz-fixes`. No push or merge.
Exact verified implementation head:
`e48504672400b60f87e7dfd82fb331df2c4250c2`.
The report is committed separately; the operations copy records the resulting
exact delivery head after that commit.

## Automatic regression gate wiring

The remaining automatic-gate gap is repaired at
`e48504672400b60f87e7dfd82fb331df2c4250c2`
(`ci(fuzz): run package oracle unit tests in normal gates`). The CI fuzz job now
runs `cargo test --locked --manifest-path fuzz/Cargo.toml --bin docx_meta` in a
separate unconditional step before its existing campaigns. The normal Linux
`just fuzz` route runs the same command before selecting targets, so the six
regressions also run for default or other-target invocations. Existing corpus,
seed, sanitizer, feature and campaign commands are preserved. `docs/ci.md`
documents this local and automatic route.

`just test-ci-scope`, already part of `just static`, now invokes
`scripts/test-fuzz-gate.py`. Its three guards check the CI command and ordering,
absence of per-step conditions/error masking, the local fail-fast command before
target selection, and the guard's static route. Two additional scope table cases
ensure fuzz-unit source and the fuzz manifest require the heavy CI/fuzz job.
This follow-up edits only workflow/recipe wiring, guard tests and owning docs;
the decoder, its six tests, seeds, manifests, libraries and dependencies are
unchanged from `8771c19`.

### Failing-first evidence and verification

All new logs are under `/home/vchun/Codes/ariadshift-logs/`.
`ws3b-fuzz-fixes-wiring-red.log` prints baseline head
`8771c190a6da9205ad6e7936348129a5b677aa0b` and records three failing new guards
before any gate wiring changed. This fast Python-only check ran directly with
`uv run --no-project --with pyyaml==6.0.3 python scripts/test-fuzz-gate.py` and
ends `=== ws3b-fuzz-fixes automatic gate expected-red ALL GREEN`.

All compilation and complete gate runs were detached through `gate-slot.sh 2`,
nice 19, `CARGO_BUILD_JOBS=2`, with the helper's load/memory admission checks
and the existing tool pins. No nested locks.
`ws3b-fuzz-fixes-wiring-precommit.log` verifies scope/guards, workflow lint,
`just fuzz docx_meta 1` including the six unit tests, spelling and staged paths,
then commits the five explicit implementation paths. It ends
`=== ws3b-fuzz-fixes automatic gate precommit ALL GREEN`.

`ws3b-fuzz-fixes-wiring-green.log` starts with committed exact head
`e48504672400b60f87e7dfd82fb331df2c4250c2` and a clean tree. It ends
`=== ws3b-fuzz-fixes automatic gate exact-head verification ALL GREEN`.
Commands from `/tmp/ws3b-wiring-green.zsh` on that committed head:

```sh
just test-ci-scope
just check-release-workflow
just lint-workflows
cargo test --locked -p ariad-cli --no-default-features --test fixture_manifest
# Parse ci.yml and execute its new unit step verbatim with bash -e -o pipefail:
cargo test --locked --manifest-path fuzz/Cargo.toml --bin docx_meta
for seed in fuzz/seeds/docx_meta/*.zip; do
  cargo fuzz run -s none --dev docx_meta "$seed" -- -runs=1
done
cargo fuzz run -s none --dev ir_json_versioned fuzz/seeds/ir_json_versioned/legacy-version-regression.json -- -runs=1
just fuzz docx_meta 60
just fuzz ir_json_versioned 60
just spell
just lint-commits 8771c190a6da9205ad6e7936348129a5b677aa0b..HEAD
```

The log explicitly names all six successful tests under the verbatim CI step
and again under each local recipe invocation:

```text
successful_stamps_cannot_skip_an_unreadable_input_directory
truncated_known_fields_are_rejected_without_modifying_the_input
raw_utf8_cp437_empty_names_and_tolerated_header_padding_have_fallbacks
malformed_optional_fields_never_hide_count_order_or_name_damage
every_admitted_regression_seed_runs_preservation_checks_with_the_real_stamper
every_version_crc_utf8_and_repeated_field_decision_retains_names
```

Results: 78 scope cases, three automatic-gate guards, nine credential-launcher
tests, two fixture manifest tests and each six-test oracle invocation passed.
Actionlint and canonical online zizmor passed, with no new suppression changes.
The existing release workflow guard recorded 78 PASS, two local SKIP because
`pwsh` is absent, and zero FAIL; PowerShell syntax execution is not claimed.
All 31 ZIP replays and the saved IR replay passed. Via the affected local
recipes, DOCX completed 70,348 runs in 61 seconds and IR completed 1,022,211 runs
in 63 seconds, both requested with a 60-second campaign parameter. Typos and
commit lint passed. The runner also verifies all 32 combined seed inventory
hashes, unchanged decoder/product inputs, unchanged existing CI fuzz steps and
five implementation file hashes.

Final report-only commit lint, spelling, source hashes, automatic unit-test log
names, preserved campaign bodies, seed provenance and clean-tree/process checks
are in `ws3b-fuzz-fixes-wiring-delivery.log`. This is local execution of the CI
step and affected recipes; no push or remote GitHub Actions run occurred. Full
monorepo CI and the complete 18-target campaign are not claimed.

## Earlier optional extra-field oracle repair

The decoder and all decision coverage below remain unchanged by the gate wiring.


The remaining bypass is repaired at
`e19320a74aed44d6bc316af6c35dac7e7a72f758`
(`test(fuzz): keep package preservation checks active`). The name helper now
returns a `String`: invalid optional Unicode replacements keep the previous
name, initially the raw entry name, and scanning continues. Tolerated trailing
padding retains the name selected so far. `stamp_case` now requires readable
input and output raw directories after every successful stamp, then always
compares physical count and name/order. The former `if let Some(before)` skip
is removed. No indexed-name substitution or new skip was introduced.

This follow-up changes `fuzz/fuzz_targets/docx_meta.rs`, its per-item unit file
`fuzz/fuzz_targets/docx_meta_tests.rs`, the DOCX target's `test` flag in
`fuzz/Cargo.toml`, 23 seeds and `fixtures/manifest.toml`. Conditional test hooks
let Cargo test the actual harness helpers; ordinary fuzz builds retain their
existing entry point. Production libraries, dependencies, lockfiles, IR target
and accepted name-encoding policy remain unchanged from `c35ef55`.

The new invalid-UTF-8 seed is byte-identical to the reviewer's
`cases/v2-invalid-utf8.zip`, SHA-256
`3a6e6a4e2f215d59262786057b45ca4f5cad1336d50306f57d0680d56a4e813d`.
Thirteen probes are copied byte-for-byte from the review artifacts; ten small
stored ZIPs cover additional framing decisions. All 23 have actual
`[[fuzz_seed]]` entries with hashes, synthetic project provenance, Apache-2.0
licensing and attribution. Existing fixture records and prior seed entries
are unchanged.

### Pinned parser decision table

Source inspected: ZIP 9.0.5 `src/extra_fields/extra_field.rs` (`parse`,
`apply_extra_fields`), `zipinfo_utf8.rs`, the ZIP64/timestamp/NTFS parsers,
`src/read/mod.rs`, and `src/types.rs::name`. These decisions describe the pinned
implementation, including its tolerated malformed input.

| Case | ZIP behavior and oracle expectation | Coverage |
| --- | --- | --- |
| No usable replacement | Original name decoded UTF-8-first, otherwise CP437; empty raw names remain empty | Raw UTF-8, CP437, ASCII and empty-name unit cases |
| Valid CRC and UTF-8, any version byte | Apply replacement; version byte is discarded | All 256 versions; existing version seeds |
| Wrong CRC | Ignore replacement and continue | All versions, CRC-mismatch seed and wrong-CRC-before/after-valid seeds |
| Invalid UTF-8 replacement | Ignore replacement and continue | All versions, reviewer seed and invalid-before/after-valid seeds |
| Empty valid replacement | Use the empty name | All versions and empty-name seed |
| Multiple replacements | Last usable field wins; every CRC uses the original raw name | All versions, two-valid seed, invalid/CRC mixtures and rebased-CRC negative case |
| Complete unknown fields, including zero length | Skip declared body, continue; embedded Unicode tags are data | Unknown-before-valid seed and unit cases |
| Unicode Comment field | Can change comment, never the entry name | Comment-before-valid seed and unit case |
| One trailing byte, or unknown ID with incomplete length | Tolerated padding; stop scanning and retain previous name | Raw padding unit cases and three trailing-padding seeds |
| Recognized ID with incomplete length | Archive rejected; failed stamp must preserve bytes | Both partial lengths for all seven recognized IDs; two known-header seeds |
| Unicode Path body lengths 0 through 4 | Archive rejected | Five short-field seeds and rejection tests |
| Truncated Unicode body | Archive rejected | Truncated-content seed and rejection test |
| Truncated unknown-field body | Archive rejected | Truncated-unknown-content seed and rejection test |
| Truncated discarded ZIP64 body without sentinels | Tolerated; retain previously selected name | Truncated-ignored-ZIP64 seed and unit case |

Six unit tests cover 2,560 version/field combinations, raw decoding/padding,
14 admitted seed packages through the real DOCX stamper, and nine rejected seed
packages with byte preservation. The DOCX fixtures also check unchanged bytes
on EPUB rejection because they contain no OPF. Test-only reference decoding
checks independently specified expected names against ZIP; the preservation
oracle itself still reads one name per physical record.

For every admitted seed, controlled closures separately drop a record, reverse
the order, and change a name: all 42 damaged outputs must trigger the specific
count or name/order assertion. This is deliberate oracle fault injection, not
a claim that the product stamper drops records. Another test proves a successful
rewrite cannot silently skip an unreadable input directory.

### Failing-first and exact-head verification

All new logs are under `/home/vchun/Codes/ariadshift-logs/`. Compilation, tests
and fuzzing ran detached through `gate-slot.sh 2`, nice 19,
`CARGO_BUILD_JOBS=2`, with the helper's admission checks for load below 12 and
at least 8 GiB available. Tool pins and the worker-private Cargo directory match
the shared-core verification section. No locks were nested or bypassed.

`ws3b-fuzz-fixes-extras-red.log` prints exact head
`c35ef559c6481560363b80c68aeaef8b8160b119`. The decoder, raw walker and stamp
assertions were byte-identical to that head; only unit-test wiring and fixtures
had been added. The source snapshot is `/tmp/ws3b-extras-red-source.rs`,
SHA-256 `c036f7252904ecdd6a77ddfc31cc3f6fabb07e8495b9295272c467dc86a4f270`.
`cargo test --manifest-path fuzz/Cargo.toml --bin docx_meta` compiled and ran:
**1 passed, 5 failed**, including the demonstrated acceptance of deliberately
removed records and the skipped input-directory guard. Its last line is
`=== ws3b-fuzz-fixes optional extras expected-red ALL GREEN`.
This explicitly expected failure establishes the regression.

`ws3b-fuzz-fixes-extras-precommit.log` runs the six repaired tests, formatting,
strict clippy including tests, spell check and staged-path validation, then
commits the 27 explicit implementation paths. It ends
`=== ws3b-fuzz-fixes optional extras precommit ALL GREEN`.

`ws3b-fuzz-fixes-extras-green.log` begins with committed exact head
`e19320a74aed44d6bc316af6c35dac7e7a72f758` and a clean tree. Its last line is
`=== ws3b-fuzz-fixes optional extras exact-head verification ALL GREEN`.
Commands from `/tmp/ws3b-extras-green.zsh` after that head was committed:

```sh
rustfmt --edition 2024 --check fuzz/fuzz_targets/docx_meta.rs fuzz/fuzz_targets/docx_meta_tests.rs fuzz/fuzz_targets/ir_json_versioned.rs
cargo test --locked --manifest-path fuzz/Cargo.toml --bin docx_meta
cargo clippy --locked --manifest-path fuzz/Cargo.toml --bin docx_meta --bin ir_json_versioned --all-targets -- -D warnings
for seed in fuzz/seeds/docx_meta/*.zip; do
  cargo fuzz run -s none --dev docx_meta "$seed" -- -runs=1
done
cargo fuzz run -s none --dev ir_json_versioned fuzz/seeds/ir_json_versioned/legacy-version-regression.json -- -runs=1
just fuzz docx_meta 60
just fuzz ir_json_versioned 60
just lint-commits c35ef559c6481560363b80c68aeaef8b8160b119..HEAD
just spell
```

Results: six tests passed, zero failed/ignored; 31 ZIP replays and the saved IR
replay passed. DOCX completed **84,558 runs in 61 seconds**; IR completed
**2,991,940 runs in 61 seconds**. Formatting, warnings-denied clippy, commit lint
and spell check passed. The runner also verifies all 26 manifest seed hashes,
unique paths, licensing, unchanged fixture records, no production/dependency/IR
diff, four source/manifest hashes and a clean tree. No full monorepo CI or longer
fuzz campaign is claimed.

Final report-only commit lint, spell check, exact-head source/hash and combined
seed inventory checks, copied-artifact equality and clean-tree/process checks
are recorded in `ws3b-fuzz-fixes-extras-delivery.log`.

## Earlier Unicode version-byte repair

`52006acd130023629f80936df0d1dcf2b2ef60a5` removed the unsupported version-1
filter and added versions 0, 2 and 255 as seeds; report head `c35ef55` preserves
that change's complete logs and verification. The current repair closes the
remaining invalid-UTF-8 and padding bypass found afterward. The earlier exact
head and delivery logs use the prefix `ws3b-fuzz-fixes-unicode-`; they are
historical evidence, not verification of the new unconditional checks.

## Earlier shared package stamping repair

The following production audit, policy and core-test evidence belong to the
shared count guard at `bcba42c0f02d553c8a305d23bf5d7d8b0eafc2b3`. That guard
and all production callers are unchanged by this oracle-only follow-up.

## Result and scope

The production audit found stamping paths without host archive preflight on the
same bytes, including WASM and generated native output. Therefore the lead's
library-repair condition applies. **This change modifies ariad-core.**
`crates/ariad-core/src/package_meta.rs` now counts physical central-directory
records independently of ZIP's name index and returns
`PackageMetaError::EntryCountMismatch { raw_count, indexed_count }` before
constructing the output writer when they differ. The reviewer's Unicode Path
alias package now returns the typed counts `3` and `2`, instead of succeeding
and losing one record.

The scanner uses ZIP's selected central-directory offset, fixed-size headers,
checked variable-length seeks bounded by the input length, and a checked count.
It does not allocate a second list of names. The archive is safely reopened
following the count pass; this adds metadata traversal rather than introducing
unsafe metadata reuse. No dependency, ZIP encoding policy or architecture change.
Native file wrappers and the WASM memory wrapper inherit the shared protection.
WASM exposes the failure through its existing invalid-input error mapping.

Before editing, the report declared the production owner as
`crates/ariad-core/src/package_meta.rs`. Per-item failing-first tests are in
`crates/ariad-core/tests/package_meta_record_count.rs`; an additional per-item
host regression is in `crates/ariad-host/tests/package_meta_record_count.rs`.
They check the exact typed counts, unchanged caller-owned output, unchanged
DOCX/EPUB source bytes and no temporary-file leftovers. `docs/cli.md` documents
the rejection contract. No other production source file changes.

## Harness diagnoses and preserved checks

The IR target's original failure was an incorrect expectation: the plain reader
retains `ariad-ir/0`, while the versioned reader intentionally upgrades it to the
current version, as required by ARCHITECTURE.md section 6.2 and existing migration
tests. The comparison normalizes only the supported version field, then requires
full content equality. Byte, depth and block limits remain intact. Its original
saved input is committed.

The original seven-`ff` ZIP failure compared lossy UTF-8 display text against ZIP's
UTF-8-first/CP437 decoding. The harness now independently applies the stamper's
display-name rule, including CRC-checked Unicode Path fields. This saved-input
oracle error is separate from the accepted consumer-visible re-encoding below.
The broader original claim that all CP437 names are preserved is withdrawn.

The fresh review correctly demonstrated that collecting indexed names hid record
loss: three raw records with Unicode aliases collapse into two indexed entries.
The repaired oracle parses one name per physical record, retaining duplicates,
and now requires readable raw directories after every successful stamp before
comparing count, order and decoded names. Input and output parsing must succeed;
neither can silently skip a check. It uses ZIP only for the selected
directory offset, never indexed names or counts, so decoy EOCDs do not cause it
to compare different embedded packages. The decoy reproducer is also retained.
Failure byte integrity, temporary cleanup and restamping checks remain intact.
No archive preflight or indexing-mismatch skip was added to the harness.

## Production caller audit

These references are at the verified implementation head. “No” means no host
`preflight_archive` / `EntryCountMismatch` check on the exact bytes subsequently
passed to stamping. The core guard above now protects every row that stamps.

| Production entry point or direct caller | Host preflight on the same bytes before stamping? |
| --- | --- |
| `crates/ariad-core/src/package_meta.rs:35` (`stamp_into`) | No host dependency; now performs the shared typed raw/index count check itself. |
| `crates/ariad-core/src/package_meta.rs:94`, `crates/ariad-core/src/package_meta.rs:104` (`stamp_bytes` to `stamp_into`) | No; memory callers have per-entry/aggregate size budgets and now the shared count check. |
| `crates/ariad-host/src/package_meta.rs:9`, `crates/ariad-host/src/package_meta.rs:13`, `crates/ariad-host/src/package_meta.rs:28` (`stamp_docx`, `stamp_epub`, private `stamp` to `stamp_into`) | No; atomic temporary-file rewrite delegates directly to core. |
| `crates/ariad-wasm/src/lib.rs:282`, `crates/ariad-wasm/src/lib.rs:283` (`stampPackage` to `stamp_bytes`) | No host preflight in WASM; the shared core count guard now rejects aliases before output writing. |
| `apps/web/src/converter/browser-engines.ts:55` | No; passes just-produced Pandoc WASM output directly to `stampPackage`. `apps/web/src/converter/pandoc.ts:108` returns those bytes without an archive preflight. |
| `crates/ariad-host/src/convert.rs:1118`, `crates/ariad-host/src/convert.rs:1120` (engine executor) | No; stamps a host-owned snapshot of generated engine output. Earlier source preflight checks different bytes. |
| `crates/ariad-cli/src/main.rs:522`, `crates/ariad-cli/src/main.rs:772` | No output preflight; `convert` / `write_from_ir` delegate to that host executor. |
| `crates/ariad-cli/src/mcp/tools.rs:423` (local MCP) | No output preflight; `convert_confined_assets_in` delegates to the same executor. |
| `apps/desktop/src-tauri/src/backend.rs:285` | No output preflight; `convert_in` delegates to that executor. |
| `crates/ariad-server/src/worker.rs:452`, `crates/ariad-server/src/main.rs:58`, `crates/ariad-host/src/convert/isolated.rs:312` | No output preflight; the isolated worker calls `convert_with_limits`, then that executor. |
| `crates/ariad-server/src/api/jobs.rs:97`, `crates/ariad-server/src/mcp/tools.rs:146` | REST and remote MCP enqueue work for that worker; neither directly stamps. |

Public host conversion wrappers also share this executor:
`crates/ariad-host/src/convert.rs:420` (`convert_custom`),
`crates/ariad-host/src/convert.rs:463` (`convert`),
`crates/ariad-host/src/convert.rs:486` (`convert_in`),
`crates/ariad-host/src/convert.rs:509` (`convert_with_limits`),
`crates/ariad-host/src/convert.rs:534` (`convert_confined`),
`crates/ariad-host/src/convert.rs:565` (`convert_confined_in`),
`crates/ariad-host/src/convert.rs:597` (`convert_confined_assets_in`), and
`crates/ariad-host/src/convert.rs:1459` (`write_from_ir`). None adds output archive
preflight before stamping. Source inspection across production Rust/TypeScript
found no other direct stamping call. Inspect/plan and input-to-IR-only paths do
not stamp outputs.

`crates/ariad-host/src/convert.rs:1748` (`copy_and_preflight_input`) and
`crates/ariad-host/src/engines/pandoc.rs:287` (Pandoc reader) run host preflight
on source files. Engine input calls are at
`crates/ariad-host/src/convert.rs:1031` and
`crates/ariad-host/src/convert.rs:1042`; the count/index comparison is at
`crates/ariad-host/src/archive.rs:435`. None proves that later generated package
bytes pass that preflight. Guarding core rather than filtering the harness is
therefore necessary, particularly for the browser path.

## Accepted v1.0 name policy

Observed behavior: an untouched flag-zero name with valid UTF-8 bytes
`c3 a9 2e 74 78 74` is written back with flag `0x800`, so a spec CP437 consumer
sees `├⌐.txt` before stamping and `é.txt` afterward.
The accepted rationale is that names in OOXML/EPUB packages are referenced from
UTF-8 XML, so the flagged form matches the package's own references and mainstream
readers already decode these names as UTF-8; preserving name bytes and flags
exactly is a v1.x item.

`crates/ariad-core/tests/package_meta_entry_names.rs` contains an independent
wire-record reader and fixture consumer decoder, without ZIP's index or decoder.
`changing_flagless_utf8_consumer_names_requires_a_deliberate_api_decision`
pins the exact original and stamped bytes, flags and consumer-visible names.
The second test distinguishes the valid non-UTF-8 CP437 case. Both pin current
behavior; neither changes the writer. The policy is also in `docs/cli.md`.

Paste into security residual-risk notes:

> In v1.0, package metadata stamping may change filename interpretation for ZIP
> consumers that honor flag-zero CP437 instead of UTF-8-first decoding: valid
> UTF-8 name bytes are written back with the UTF-8 flag. This compatibility
> decision matches OOXML/EPUB references in UTF-8 XML and mainstream readers'
> existing interpretation. Exact preservation of name bytes and flags is deferred
> to v1.x and is not a v1.0 guarantee.

## Earlier shared-core verification

All compilation, tests and fuzzing ran detached through
`/home/vchun/Codes/ariadshift-ops/gate-slot.sh 2`, nice 19,
`CARGO_BUILD_JOBS=2`, admission load below 12 and at least 8 GiB available.
No nested lock. Tool pins: `MISE_UV_VERSION=0.12.23`,
`MISE_PNPM_VERSION=12.9.1`, `MISE_NODE_VERSION=$(cat .node-version)`;
`CARGO_TARGET_DIR=$PWD/fuzz/target` is worker-private.
Logs are under `/home/vchun/Codes/ariadshift-logs/`.

Failing-first log: `ws3b-fuzz-fixes-count-red.log`, last line
`=== ws3b-fuzz-fixes typed count expected-red ALL GREEN`.
After declaring the new error variant but before implementing the runtime guard,
`cargo test -p ariad-core --no-default-features --test package_meta_record_count`
compiled and failed both tests (0 passed, 2 failed): stamping still succeeded.
The runner explicitly required that failure. This establishes the regression;
it is not counted as a passing product test.

Earlier core gate: `ws3b-fuzz-fixes-count-green.log`, last line
`=== ws3b-fuzz-fixes typed count verification ALL GREEN`.
Commands executed sequentially by `/tmp/ws3b-fuzz-fixes-count-green.zsh`:

```sh
git rev-parse HEAD
rustfmt --edition 2024 crates/ariad-core/src/package_meta.rs crates/ariad-core/tests/package_meta_record_count.rs crates/ariad-host/tests/package_meta_record_count.rs
cargo test -p ariad-core --no-default-features --test package_meta_record_count
cargo test -p ariad-host --no-default-features --test package_meta_record_count
cargo test -p ariad-core --no-default-features
cargo test -p ariad-host --no-default-features --lib package_meta::tests
cargo fmt --all --check
rustfmt --edition 2024 --check fuzz/fuzz_targets/docx_meta.rs fuzz/fuzz_targets/ir_json_versioned.rs
for seed in non-utf8-entry-name.zip unicode-alias.zip cp437-utf8-overlap.zip cp437-ff.zip decoy-eocd.zip; do
  cargo fuzz run -s none --dev docx_meta fuzz/seeds/docx_meta/$seed -- -runs=1
done
cargo fuzz run -s none --dev ir_json_versioned fuzz/seeds/ir_json_versioned/legacy-version-regression.json -- -runs=1
just fuzz docx_meta 60
just fuzz ir_json_versioned 60
cargo clippy -p ariad-core -p ariad-host --no-default-features --all-targets -- -D warnings
cargo clippy --manifest-path fuzz/Cargo.toml --bin docx_meta --bin ir_json_versioned -- -D warnings
cargo check -p ariad-wasm --target wasm32-unknown-unknown
just spell
sha256sum crates/ariad-core/src/package_meta.rs crates/ariad-core/tests/package_meta_record_count.rs crates/ariad-host/tests/package_meta_record_count.rs fuzz/fuzz_targets/docx_meta.rs fuzz/fuzz_targets/ir_json_versioned.rs
```

Results: focused core 2/2 passed; focused host 1/1 passed (both DOCX and EPUB);
full core suite 363 passed, zero failed/ignored, including both independent name
policy tests; host metadata unit tests 4/4 passed. All six saved seed replays
passed. Default `docx_meta` completed 121,473 runs in 61 seconds;
`ir_json_versioned` completed 3,241,110 runs in 61 seconds. Formatting, both
clippy invocations with warnings denied, WASM target compilation and spell check
passed. No full monorepo CI or browser runtime test is claimed.

The log begins at parent head `45928b5c62604ffde82d033a1ae1f704f1871409` with
the repair uncommitted. Its final SHA-256 checksums match the source/test files
committed at `bcba42c0f02d553c8a305d23bf5d7d8b0eafc2b3`; at that head the
harness files matched their already committed strict version. The earlier delivery
check verified those historical source hashes, seed provenance, commit lint,
spell check and clean worktree at report head `828c330` in
`ws3b-fuzz-fixes-count-delivery.log`. The new oracle has separate exact-head
verification above.

Historical reports at `dc043f4` and `45928b5` retain earlier failing/blocked gate
evidence. In particular, the earlier default DOCX failure was real raw record
loss; the shared core error now resolves that blocker without weakening the
oracle. Those superseded logs are not presented as final-head green runs.

## Seed provenance and manifest handoff

The first two seeds are byte-identical to the original supplied crashes. The IR
regression intentionally duplicates the original generated seed per the lead.
Three review packages are byte-identical to their synthetic review artifacts;
the decoy is original project fuzz data. No private document is included.
These six earlier entries remain a handoff to the sibling fuzz-packs inventory.
The 26 seeds from the Unicode Path follow-ups are already declared in
`fixtures/manifest.toml` (three version seeds and 23 decoder/framing seeds);
integrate the following earlier six alongside them.

```toml
[[fuzz_seed]]
id = "fuzz-docx-meta-non-utf8-entry-name-zip"
target = "docx_meta"
path = "fuzz/seeds/docx_meta/non-utf8-entry-name.zip"
sha256 = "d6e1a35df51b83fbcbf3d2dbeef66a7bc921e04a3fea64ba2104c4b53851692a"
source = "Saved project fixture fuzz mutation, derived from fixtures/office/vi-route-sheet.xlsx."
license = "Apache-2.0"
license_basis = "Original project fixture/test content and synthetic fuzz mutations under the repository license."
attribution = "AriadShift contributors"
```

```toml
[[fuzz_seed]]
id = "fuzz-ir-json-versioned-legacy-version-regression-json"
target = "ir_json_versioned"
path = "fuzz/seeds/ir_json_versioned/legacy-version-regression.json"
sha256 = "42b57ac3f5852d6212f5614fa71266bdc3b4c2155424e2ab37db68c7394d9e21"
source = "Saved original project IR, derived from fixtures/md/en-fenced-code.md."
license = "Apache-2.0"
license_basis = "Original project fixture/test content and synthetic fuzz mutations under the repository license."
attribution = "AriadShift contributors"
```

```toml
[[fuzz_seed]]
id = "fuzz-docx-meta-unicode-alias-zip"
target = "docx_meta"
path = "fuzz/seeds/docx_meta/unicode-alias.zip"
sha256 = "0d5ad9d485b7ced647b7bca0ca13e8ce0ab4d58dbdce818d9eb5cacf40dc7d59"
source = "Original synthetic Unicode Path alias ZIP from r-3b-fuzz-fixes-sol-artifacts."
license = "Apache-2.0"
license_basis = "Original project fixture/test content and synthetic fuzz mutations under the repository license."
attribution = "AriadShift contributors"
```

```toml
[[fuzz_seed]]
id = "fuzz-docx-meta-cp437-utf8-overlap-zip"
target = "docx_meta"
path = "fuzz/seeds/docx_meta/cp437-utf8-overlap.zip"
sha256 = "7a76ce51184e5d6e9da0c82ff408d36fac6d6078e749ce25f31553f573671e37"
source = "Original synthetic CP437/UTF-8 overlap ZIP from r-3b-fuzz-fixes-sol-artifacts."
license = "Apache-2.0"
license_basis = "Original project fixture/test content and synthetic fuzz mutations under the repository license."
attribution = "AriadShift contributors"
```

```toml
[[fuzz_seed]]
id = "fuzz-docx-meta-cp437-ff-zip"
target = "docx_meta"
path = "fuzz/seeds/docx_meta/cp437-ff.zip"
sha256 = "e3bda46fa955fbc4c8774e47e5b9b7177e9e837716c2b40a1ece22bb86cd7d51"
source = "Original synthetic CP437 ff ZIP from r-3b-fuzz-fixes-sol-artifacts."
license = "Apache-2.0"
license_basis = "Original project fixture/test content and synthetic fuzz mutations under the repository license."
attribution = "AriadShift contributors"
```

```toml
[[fuzz_seed]]
id = "fuzz-docx-meta-decoy-eocd-zip"
target = "docx_meta"
path = "fuzz/seeds/docx_meta/decoy-eocd.zip"
sha256 = "9e6d102883675d881cc27120b84445f95dd4caf00be0c59ef7039ead4a8bc6dc"
source = "LibFuzzer crossover of the project spreadsheet and synthetic CP437 probes; artifact crash-4fa23e8e102f63b3f05821280d8495889445287a."
license = "Apache-2.0"
license_basis = "Original project fixture/test content and synthetic fuzz mutations under the repository license."
attribution = "AriadShift contributors"
```

## Commits and integration notes

- `8abda6fb8cd6c21daa75a6c87e4144eac62fc303`: initial IR/display-name repairs,
  original two saved inputs, direct ZIP fuzz dependency and corresponding lock
  edges. The indexed-name oracle in that commit is superseded below.
- `dc043f44b1be1ee068648b38ddf0b70eb757fc83`: original report.
- `dc5e13b4d5f6735e5b5e9eb66905fee77fb1cce5`: independent raw record oracle,
  Unicode alias regression and decoy EOCD seed.
- `52c98eb3cc58929ec2da04a306478e259f85d3b4`: two independent consumer-level
  name policy tests, CP437 probes and documentation.
- `45928b5c62604ffde82d033a1ae1f704f1871409`: previous blocked status, retained
  as history.
- `bcba42c0f02d553c8a305d23bf5d7d8b0eafc2b3`: shared typed count rejection,
  failing-first per-item core tests, host atomicity test and rejection docs.
- `828c330fc934ed5e7016088676407659fdfe9e67`: production caller audit and
  shared-count verification report.
- `52006acd130023629f80936df0d1dcf2b2ef60a5`: Unicode Path version oracle
  correction and three licensed regression seeds with manifest entries.
- `c35ef559c6481560363b80c68aeaef8b8160b119`: version-byte verification report.
- `e19320a74aed44d6bc316af6c35dac7e7a72f758`: unconditional preservation
  checks, complete Unicode/framing decision tests and 23 licensed seeds.
- `8771c190a6da9205ad6e7936348129a5b677aa0b`: complete decoder decision and
  exact-head verification report.
- `e48504672400b60f87e7dfd82fb331df2c4250c2`: automatic CI/local unit gate,
  static guards, scope regression cases and CI documentation.

All commits use explicit paths. Earlier manifest changes add the Unicode Path
and framing seeds, and `fuzz/Cargo.toml` enables unit tests for this target. The
latest follow-up changes only the five gate/documentation files above; no decoder,
dependency, lockfile, host/WASM production code or ARCHITECTURE.md change was
needed for the gate repair.
Preserve the sibling fuzz-packs fixed timestamp and richer IR declared-version
checks when integrating this strict raw oracle, and fold in the six inventory
entries above. No integration merge or push occurred.

All requested implementation and verification work is complete. The fresh review
also measured 1.30–1.38 times the baseline native stamping time for synthetic
packages with many small entries on a shared machine; no latency budget was
supplied. That shared-count overhead
remains a handoff concern, and no optimization is claimed in this oracle repair.
The accepted v1.0 name re-encoding compatibility limitation remains documented
and pinned; exact byte/flag preservation is v1.x work, not a remaining blocker
for this task.

Status: DONE_WITH_CONCERNS
