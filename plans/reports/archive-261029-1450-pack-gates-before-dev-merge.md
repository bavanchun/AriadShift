# Pack gates: refuse payload-bearing directories and ambiguous export paths

The ZIP-directory export bypass is fixed. Directory names and directory flags
cannot exclude payloads from the native scanner. Any directory with a nonzero
declared/compressed size or readable bytes fails; only genuine empty directories
may skip content scanning. No push, tag, release, dispatch, GPL waiver or wheel
stripping occurred.

## Source and report heads

- Starting report head: `896a1748a963ec1a30c7bffe2ab1d43ff8994c91`.
- Tested source head: `4740b4d5a4be15d576deaccb16de042065707505`.
- Source commit: `fix(docling): reject ambiguous wheel paths and directory payloads`.
- Report closure is a Markdown-only successor of that exact tested source head.
- [Previous report](archive-261028-2305-pack-gates-native-export.md) preserves the
  native identity/notice receipts, GNU provenance, CI policy and release limits.
  Its omitted directory-payload coverage is supplied by this rework.

## Behavior and regression evidence

The former duplicate list and scan loop both skipped `ZipInfo.is_dir()` entries
based solely on a trailing slash. The checker now validates every member path
and checks directory sizes/readable bytes before excluding an empty directory.
Directory attributes are also checked. Nonempty entries still reach native
identification and the independent video-family denial, even when their path
is invalid or ends in `/`.

Paths must be canonical relative POSIX names. Backslashes, absolute/drive paths,
traversal, dot/empty segments, NUL/control characters, duplicate names, case
collisions and file/directory conflicts fail. Duplicates include directory
entries and implicit parent-directory conflicts. The installed-pack policy,
licence exceptions, native receipts, pins and lockfile are unchanged.
AGENTS.md, ARCHITECTURE.md and its Decision Log, docs/docling.md and the module
threat-model text describe the actual checks.

Tests were written first and run against unchanged source `896a174` in the
owned detached verification worktree. Implementation proceeded separately while
the frozen-baseline runner waited for its slot. The baseline produced **27
failed, 57 passed**; the reviewer's unchanged real-object diagnostic accepted
**all six** GPL directory payload variants and failed its assertion. Each of
the three original x264/x265/Rubber Band objects was SHA-256 verified against
the hash-verified locked macOS wheel, then checked under its original name and
`cv2/.dylibs/libcodec.dylib/`. Neither the objects nor the locked wheel changed.
Red evidence: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework11-red.log`.

After the repair, **84 export tests** and **260 adjacent pack/licence tests**
passed. The same real directory diagnostic refused all six variants; the
existing strict probe refused twelve ordinary GPL renames and accepted five
reviewed real BSD objects only with complete notices. Tests also cover plain
non-native directory payloads, empty genuine directories, compressed directory
bytes, both directory attribute forms, unknown native content, unsafe names and
duplicate/case/file-directory collisions. An empty directory cannot satisfy an
expected OpenMP replacement. The NUL fixture patches actual ZIP
names so it tests stored bytes rather than ZipInfo's pre-write truncation.
Focused log: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework11-focused.log`.

## Exact-head scoped pack gate

The runner ran detached through slot 2 at nice 19, initially with one
build/test/native-math worker. The continuation uses CARGO_BUILD_JOBS=2 and
NEXTEST_TEST_THREADS=2 under A-0240, with native math kept at one worker. It waited for the shared slot and load below 12, never nested locks and
never paused a slot holder. Baseline, focused checks, the explicit source commit
and the first gate attempt ran sequentially in one slot acquisition. The gate command was:

```sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 bash /tmp/t7-rework11-red.sh
# The runner invokes, without another lock:
bash /tmp/t7-rework11-final-pack-gate.sh
```

The first attempt stopped at `uv audit --frozen` after three failed OSV TLS
handshakes (`tls handshake eof`), after all pack tests, real probes, both Linux
inventories and the lock check had passed. That failed log is retained at
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework11-final-pack-gate-attempt1.log`.
The runner had exited before the resource-rule update; no queued job was moved.
The endpoint then accepted a verified TLS connection. The continuation retried
the audit and ran the remaining checks through slot 2 at the same source head,
reusing completed checks with unchanged source/inputs. The canonical log retains
the failed attempt and the clearly marked successful continuation.

```sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 bash /tmp/t7-rework11-resume.sh
```

The [first complete command sequence](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework11-final-pack-gate.sh)
and [continuation sequence](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework11-remaining-checks.sh)
are retained. Results at `4740b4d5a4be15d576deaccb16de042065707505`:

- Focused pack/licence tests: **344 passed**. Whole `engines/docling/tests`:
  **438 passed, 4 skipped**; the existing conditional native compiler/ABI skips
  remain. All new regressions and the real CLI manifest integration ran.
- Six real GPL directory variants refused, twelve ordinary GPL variants refused,
  five reviewed BSD objects accepted only with their complete reviewed notices.
- Three real pywin32 wheels/installations and forged RECORD/source negatives;
  CP1252 and C-locale probes passed.
- Source-built fixture: **256 native files = 54 named + 202 extensions**,
  unique licensed RECORD ownership, complete notices, **7 GNU hash/ELF checks**.
- ARM Torch: **146 native objects**; **148 ZIP payloads** in **216 hash-verified
  locked wheels** checked. Linux OpenCV: **31 native objects**, five declared
  LGPL FFmpeg objects per target and zero GPL/nonfree flags.
- Fresh Linux inventories: x86_64 **402 native / 200 named**, aarch64 **403
  native / 201 named**, zero global/per-file findings across **805 files**.
- `uv lock --check`, `uv audit --frozen` (**112 packages**, no vulnerabilities
  or adverse statuses), explicit encoding audit (**22 files**), Ruff check and
  format (**13 files**) passed.
- `just bench-test`: **75 passed**, including licence tests. All fixture
  generators ran twice; **135 files** matched each other, stored fixtures and
  manifest SHA-256 values.
- Both workflows passed actionlint and zizmor 1.30.1 with no unsuppressed
  findings. `typos`, `just lint-commits` and clean exact-head checks passed.
  A real current-head CLI was built; the full Rust matrix remains excluded
  locally under A-0352 and belongs to GitHub CI.

Gate log: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework11-final-pack-gate.log`.
Last line: `=== PACK GATE ALL GREEN HEAD=4740b4d5a4be15d576deaccb16de042065707505`.

Fresh [805-file census](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework11-native-census.json),
[platform summary](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework11-platform-summary.json),
[fixture inventory](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework11-total-native-inventory.json)
and [GNU ABI evidence](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework11-gnu-symbol-tags.log)
are retained separately from earlier evidence. Full notices are at
`/home/vchun/Codes/t7-native-inventory/notices-rework11`.
Report closure checks typos, commit lint, tested first parent, clean Git state,
local links and the byte-identical operations mirror; log:
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework11-report-closure.log`.
AgentWiki publish skipped; the journal is local.

## Remaining release prerequisites

The five BSD export receipts still approve individual reviewed objects, never
the containing GPL wheel or unreviewed new source-build outputs. New native
outputs require their own component/licence/hash/notice reviews before upload.
Native macOS compilation, installed-pack/conversion/smoke validation and the
actual video-disabled OpenCV release pin remain pending. PR macOS readiness
stays visibly skipped without that pin; both Linux targets remain mandatory.
Windows packs remain deferred. No release omission or publication is authorized.

Unresolved questions: None.

Status: DONE_WITH_CONCERNS
