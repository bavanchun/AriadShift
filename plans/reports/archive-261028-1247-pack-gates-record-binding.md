# Docling pack gates: complete pywin32 payload binding

The pywin32 exception now binds every original wheel payload file, including
`win32/lib/win32con.py`, to the reviewed wheel RECORD. Generic licence rules,
approved notices/source pointers, dependency locks and fixtures are unchanged.

- Tested code head: `f34a7574bc33d9eb89e5b1ff8c8d33cedb19b7ea`.
- Code commit: `fix(docling): bind complete reviewed wheel payloads`.
- Gate: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework4-final-gate.log`.
- Last line: `=== PACK GATE ALL GREEN HEAD=f34a7574bc33d9eb89e5b1ff8c8d33cedb19b7ea`.
- A report-only successor records this result; no previous commit was rewritten.
- [Previous Windows/bench repair](/home/vchun/Codes/01-AriadShift-wt-t7-pack/plans/reports/archive-261028-1213-pack-gates-rework3.md)
  preserves the accepted fixture, UTF-8, licence and earlier gate evidence.
- [Execution record](/home/vchun/Codes/01-AriadShift-wt-t7-pack/plans/reports/execution-261028-1213-pywin32-record-binding.md).

## Binding and installation handling

Each original RECORD is committed unchanged under
`engines/docling/pack/reviewed-records/` and SHA-256-bound in the existing named
review entry. Every installed RECORD payload entry must match its actual
SHA-256 and byte size, using the same helper as the total native inventory.
The self-referential RECORD row remains unhashed, as in the original wheel.

The check reconstructs original wheel RECORD rows from the installed payload,
restoring the receipt row order and CSV serialization, then requires their
SHA-256 to equal the reviewed original digest.
Changing another source file fails its installed hash check; editing RECORD to
hide the change still changes the reconstructed digest and fails. Missing rows,
duplicate rows, missing files and unreviewed payload entries fail. The check
also refuses unlisted files inside the distribution's owned directories.

Raw installed RECORD bytes differ legitimately from the wheel: uv moves the two
`.data/scripts` files, generates launchers beside them and adds installer
metadata. Only those two script names at known script destinations, their
launchers, and `INSTALLER`, `REQUESTED`, `direct_url.json`, `uv_cache.json` are
recognized. Their installed hashes/sizes are still verified. Installer-created
metadata/launchers are not asserted to be original wheel bytes. Derived
`__pycache__/*.cpython-314[.opt-N].pyc` files are tolerated only when their source
is in the reviewed payload; bytecode for an unlisted source fails. Root
site-packages and shared bin/Scripts directories also hold other distributions,
so the pywin32-owned tree census does not claim to own them wholesale.

This is the deliberate installation normalization; comparing a raw installed
RECORD digest directly would reject ordinary uv installations. No arbitrary
wheel byte replacement or licensing override is normalized away. Existing
metadata, eight-notice and adodbapi source bindings remain in place. AGENTS
Licensing, the owning architecture paragraph and its Decision Log row describe
the strengthened scope. A type-specific typos rule skips URL-safe digest tokens
in the unchanged upstream RECORD receipts, while still checking their paths.

## Three locked wheel records

uv.lock contains exactly these three pywin32 312 wheels, all CPython 3.14;
there are no CPython 3.12/3.13 pywin32 wheels in this lock.

| Wheel suffix | Original RECORD SHA-256 |
| --- | --- |
| `cp314-cp314-win32.whl` | `7af03355b008f5487ef350e846aa68c989505cf9ee8afd975e8f8b71ab66c07a` |
| `cp314-cp314-win_amd64.whl` | `65762feb4074c3a4f4f1b7eeca7db60ad1c741f40dca1bf7efcdc7f13217019c` |
| `cp314-cp314-win_arm64.whl` | `c2b5405aef6630c27ac46625bf227671d386538a7a9e545b6e8b74419411230d` |

Each original wheel has 611 RECORD entries. The inspected direct-wheel uv
installations have 617 entries after the known transformations. At the final
head, the verification helper checks the wheel SHA-256 against uv.lock, proves
that the committed receipt equals the original ZIP member byte-for-byte, checks
each complete extraction and uv installation, and refuses a modified outside
source both before and after rewriting its installed RECORD hash/size.

## Tests and exact-head gate

The new regression suite failed on the old implementation: **10 failed,
11 passed**, including the modified outside source, rewritten RECORD,
missing/extra file, wrong hash/size, duplicate row, unknown cache, changed
receipt and altered installer file. All focused tests pass with the repair.
The unchanged fixture and intentional installation transformations remain valid.

| Final check | Result |
| --- | --- |
| Focused licence/inventory suites | 172 passed |
| Whole engines/docling/tests | 263 passed, 4 skipped |
| Three locked complete wheel and uv installations | Passed; changed outside source and rewritten installed RECORD refused |
| CP1252 / C-locale probes | 5 passed each |
| Real native inventory | 256 files pass: 52 named objects and 204 extensions |
| uv lock --check | 113 packages; lock unchanged |
| uv audit --frozen | 109 packages; no known vulnerabilities or adverse statuses |
| UTF-8 audit | 19 files, no implicit encodings |
| Ruff check / format --check | Passed |
| just bench-test | 75 passed |
| Fixture generators | 135 files match stored bytes/manifests in two runs |
| typos / just lint-commits / tracked diff | Passed |


The full gate runs detached through `gate-slot.sh 1`, nice 19, with one Cargo,
test, BLAS and Rayon job. It runs the five focused licence/inventory test files,
the whole engine suite, all three complete wheel/uv-install checks, both encoding
probes, the real 256-file Linux native inventory, `uv lock --check`,
`uv audit --frozen`, UTF-8 audit, Ruff check/format, `just bench-test`, all fixture
generators twice, typos, `just lint-commits` and `git diff --exit-code`.
The cached current-head CLI build supplies the actual bench integration binary;
Cargo metadata selects its output directory for PATH. No full Rust matrix or
`just ci` was added to the scoped gate.

The exact command is `zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 1 /bin/bash /tmp/t7-rework4-final-pack-gate.sh`, launched detached. The full commands are in the [preserved gate script](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework4-final-pack-gate.sh).

Evidence in `/home/vchun/Codes/ariadshift-logs/`:

- `d2120-t7-rework4-red.log`: old implementation, 10 failed/11 passed.
- `d2120-t7-rework4-install-probe.log`: exact uv RECORD transformations.
- `d2120-t7-rework4-focused.log` and `...-focused-2.log`: passing focused suites
  and actual wheels; the second adds the direct reconstructed RECORD digest check.
- `d2120-t7-rework4-wheel-record-review.json`: exact wheel/RECORD digests and counts.
- `d2120-t7-rework4-final-gate.log`: complete final gate at the tested code head.
- The gate script, wheel checker, native inventory JSON and other source helpers
  are preserved beside that log.

## Remaining limits

Windows runtime behavior and a complete Windows native pack still require the
Windows runner and the separately deferred pack work. The local cross-platform
uv installations exercise real wheel content and relocation on Linux, without
executing Windows binaries. The previously documented broad Windows RECORD
refresh, non-semantic default notice discovery and limited archive recognition
remain unchanged. The accepted image-equivalent fixture serialization and
original measured benchmark inputs remain as recorded in the previous report.

No approval is pending, no further review round is required by the brief, and
nothing was pushed. The result is ready for the lead's gitsync.

Status: DONE_WITH_CONCERNS
