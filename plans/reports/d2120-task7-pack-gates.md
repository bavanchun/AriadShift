# Pack gates after merging dev

Merged fetched `origin/dev` into `fix/pack-prebuild-gates` without conflicts,
rebasing or pushing. The complete scoped pack gate passed at the merge head.

- Previous branch head: `edc6d0bd5e6c7a889c4ba5bd8d6bca8482a7c8d4`.
- Fetched dev: `c687774cdd487b9ba899b41766e77ecec747dd74` (#101 and #102 included).
- Tested merge head: `d5a4446882914d1d89649e4fc6d036bbc345df8e`.
- Merge commit: `chore: merge dev into pack prebuild gates`.
- The report commit is a Markdown-only successor of this tested merge head.

Pillow's exact 13.0.0 wheel entries, benchmark/fixture declarations and regenerated
scan fixtures match dev. The lock additionally retains this branch's existing
macOS toolchain packages `altgraph`, `delocate` and `macholib`. No lockfile was
hand-edited or regenerated. Pack source and both pack/toolchain workflows are
unchanged from the accepted branch head. The committed tree equals Git's
automatic merge tree; the [merge proof](/home/vchun/Codes/ariadshift-logs/d2120-t7-sync-dev-261029-merge-proof.json) records
that comparison. History was preserved, including the duplicate Pillow commits.

## Verification

The gate ran detached through slot 2 at nice 19, with CARGO_BUILD_JOBS=2 and
NEXTEST_TEST_THREADS=2; native math workers remained at one. No extra dev server,
container, nested lock or paused process was created. The command was:

```sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 bash /home/vchun/Codes/ariadshift-logs/d2120-t7-sync-dev-261029-gate.sh
```

The [complete command sequence](/home/vchun/Codes/ariadshift-logs/d2120-t7-sync-dev-261029-gate.sh) and its persistent helper
scripts are retained beside the log. Earlier temporary helpers had disappeared;
they were restored from the saved scripts and point to fresh evidence outputs.
All results below were rerun at `d5a4446882914d1d89649e4fc6d036bbc345df8e`:

- Focused pack/licence tests: **344 passed**. Whole `engines/docling/tests`:
  **438 passed, 4 existing conditional native compiler/ABI skips**.
- Real export probes refused six GPL directory payload variants and twelve
  ordinary GPL renames. Five reviewed BSD objects passed with complete notices;
  their five missing-notice variants failed as required.
- Three locked pywin32 wheels and their uv installations passed, including
  modified-source and forged-RECORD negatives. CP1252 and C-locale probes passed.
- Source-built fixture: **256 native files, 54 named and 202 extensions**, one
  licensed RECORD owner each, full notices and **7 GNU hash/ELF checks**.
- ARM Torch: **146 native objects**. **148 ZIP payloads** in **216 wheels** passed;
  all wheel receipts were rehashed and required to occur in the current lock.
- Linux OpenCV: **31 native objects**, five LGPL FFmpeg objects per target,
  zero GPL/nonfree flags. Fresh complete inventories covered **805 files**:
  x86_64 **402 native / 200 named**, aarch64 **403 native / 201 named**,
  with zero global or per-file findings.
- `uv lock --check`, `uv audit --frozen` (**112 packages**, no known
  vulnerabilities or adverse statuses), explicit text-encoding audit,
  Ruff check/format, actionlint, zizmor, `typos` and `just lint-commits` passed.
- `just bench-test`: **75 passed**. All fixture generators ran twice;
  **135 files** matched stored bytes and manifest SHA-256 values.
- The actual merged-head CLI built and CI-scope checks passed. The full Rust
  matrix remains outside this local gate under A-0352 and belongs to GitHub CI.

Gate log: `/home/vchun/Codes/ariadshift-logs/d2120-t7-sync-dev-261029-gate.log`.
Last line: `=== PACK GATE ALL GREEN HEAD=d5a4446882914d1d89649e4fc6d036bbc345df8e`.

Fresh [native census](/home/vchun/Codes/ariadshift-logs/d2120-t7-sync-dev-261029-native-census.json),
[platform results](/home/vchun/Codes/ariadshift-logs/d2120-t7-sync-dev-261029-platform-summary.json),
[fixture inventory](/home/vchun/Codes/ariadshift-logs/d2120-t7-sync-dev-261029-total-native-inventory.json) and
[GNU ABI evidence](/home/vchun/Codes/ariadshift-logs/d2120-t7-sync-dev-261029-gnu-symbol-tags.log) are retained.
Full fixture notices: `/home/vchun/Codes/t7-native-inventory/notices-sync-dev-261029`.

## Retained limits

[The previous report](archive-261029-1450-pack-gates-before-dev-merge.md) preserves
all native identity, notice and provenance decisions. Native macOS build,
installed-pack/conversion/smoke validation and the actual video-disabled OpenCV
artifact pin remain release prerequisites; new native outputs still require
individual reviews. Windows packs remain deferred. No publication is authorized.

The Git helper could not start because its configured model was unsupported;
the merge was completed directly. The incoming, already published task-10 report
has seven trailing-whitespace lines reported by `git diff --check`; its dev
content was preserved. The requested pack, spelling and commit gates passed.
The report is copied byte-for-byte to the operations reports directory.
Markdown-only closure checks are logged at
`/home/vchun/Codes/ariadshift-logs/d2120-t7-sync-dev-261029-report-closure.log`.

Unresolved questions: None.

Status: DONE_WITH_CONCERNS
