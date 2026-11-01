# Image output and scan coverage changes

Status: complete for the bounded investigation. The observed native PDFium
allocation failure remains generic because it provides no reliable marker to
the host; see the reproduction below.

- Image extraction previously accumulated encoded payloads and duplicated the
  original JPEG. The tool now writes each payload to scratch, charges the same
  cumulative byte budget as rendering, and emits artifacts only after extraction
  completes. The regression is
  `extraction_streams_and_rejects_cumulative_output_without_artifacts`.
- Removed the unused ZIP helper; render's existing ZIP tests cover its current
  streaming implementation.
- Scan detection previously rejected the 2,001st rectangle. It now marks the
  page `saturated` and uses the first 2,000 rectangles, while still accounting
  for traversal and sweep work. Regressions:
  `many_overlapping_images_report_saturated_coverage` and
  `image_object_work_budget_still_fails`.
- The watchdog returns `MemoryLimitExceeded` when it detects an RSS excess, and
  Linux maps the Rust allocator's explicit failure message. The native PDFium
  failure reproduced below returns a generic engine error without an OOM marker.
  No host mapping was added.

## Checks

All Cargo commands used `build-lock.sh` and the configured PDFium and Pandoc
paths where needed.

- `cargo fmt --all --check`: passed.
- `cargo clippy --locked -p ariad-pdfium-engine -p ariad-host --features ariad-host/test-probe --all-targets -- -D warnings`: passed after correcting a lint.
- `cargo test --locked -p ariad-pdfium-engine`: passed; one corpus scan ignored.
- `cargo test --locked -p ariad-host --features test-probe --test tool_runner_limits`: 11 passed.
- `cargo test --locked -p ariad-host --features test-probe --lib watchdog`: 2 passed.
- `cargo test --locked -p ariad-host --features test-probe --lib runner`: 7 passed.
- `git diff --check`: passed.

Assumptions: the page flag is true only when at least one image rectangle is
omitted, and the retained rectangles give a lower-bound coverage estimate. No
new fixture files were committed; the regressions construct small PDFs in code.
The public extraction helper retains its collecting interface for existing
callers; the tool execution path streams.


## Native PDFium allocation investigation

On Linux 7.2.3-arch1-3 x86_64, the pack under
`/home/vchun/Codes/01-AriadShift/.run/coord/pdfium-pack/bin` contained engine
SHA-256 `c83469f965a3500c60a50be2e2cc4e6e32e3ae66f7978b1d8612a0a204270514`
and libpdfium SHA-256
`1c251efce460d67437a4c76d4be9033ae2f17bf5603f5bff1af10209b94ca723`.
This is the assembled pack, not the binary compiled by this worktree.

The probe generated a small PDF 1.4 with one blank page, a catalog and Pages
tree, `/MediaBox [0 0 940 940]`, content `q Q`, and a valid xref. It sent an
`ariad-engine/2` `run_tool` request for `pdf.render`, `format=png`, `dpi=600`,
and `limits.max_memory_mb=256` to the pack engine. A Python parent set the
child's hard and soft `RLIMIT_AS` to 256 MiB and `RLIMIT_CORE` to zero before
`exec`, captured both streams, and used `wait4` for status and `ru_maxrss`.
The parent and child ran within `systemd-run --user --scope --quiet
-p MemoryMax=2G -p MemorySwapMax=0`. The temporary probe is
`/tmp/ariad-pdfium-oom-probe.py`; it is not a repository artifact.

Three runs returned exit status 1, no signal, empty stderr, and peak RSS of
14,960, 15,064, and 15,032 KiB respectively. Each emitted a failed result with
`code: engine_failure` and message `PDFium failed to render page 1:
PdfiumLibraryInternalError(\n    Unknown,\n)`. No PartitionAlloc or
out-of-memory message appeared. The same input and request with both the
request memory limit and `RLIMIT_AS` raised to 1,024 MiB succeeded, exited 0,
and reached 494,588 KiB peak RSS. A 612 pt page at 72 dpi under 256 MiB also
succeeded.

A separate GDB run of the same 256 MiB case, with
`set exec-wrapper prlimit --as=268435456 --core=0 --`, broke at
`FPDFBitmap_CreateEx` and used `finish` to observe a null return; its caller
was `pdfium-render`'s `PdfBitmap::empty`, called by `render_with_config`.
Inspecting the thread's errno immediately after return yielded 12 (`ENOMEM`)
in that run. This is diagnostic evidence of the failed native allocation,
but the generic PDFium API does not contractually identify the cause of a
null return through errno, and the host receives neither that value nor a
distinct error. Treating every generic PDFium error as OOM would misclassify
other failures. No reliable host-side signal or stderr marker was observed.

For comparison, a 930 pt page at 600 dpi and 256 MiB exited via `SIGABRT`,
with peak RSS 249,800 KiB and stderr `memory allocation of 240250000 bytes
failed` followed by the Rust backtrace hint. This is the Rust allocation
path already recognized by the runner, not evidence of a native PDFium abort.
The attempted rendering cases did not reproduce a native PDFium abort.

No source mapping was changed. The existing host behavior is unchanged,
including for the generic native failure; the macOS watchdog was not exercised
on this Linux host. Mac PM can decide whether the generic failure warrants a
separate engine-side contract or instrumentation.
