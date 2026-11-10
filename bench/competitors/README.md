# Local document competitor benchmark

This tooling compares AriadShift and Stirling-PDF on the same original authored
PDF/DOCX corpus. Stirling runs only as local evaluation tooling. Its image,
layers, dependencies and converted documents are never distributed with
AriadShift. The harness uses the existing Python workspace without new
dependencies or lockfile changes.

## Reproduce

Run from the repository root with a provisioned `ashift` binary. These Python
commands do not acquire the shared compiler lock or start a nested build.

```sh
docker pull --platform linux/amd64 stirlingtools/stirling-pdf:3.3.0@sha256:b670779e1984ab3b7d2009d34b4fff63f6294aeb2325c97dc93f5f616bf4b9c7

timeout --signal=TERM --kill-after=10s 600s uv run --package ariad-bench python bench/competitors/run.py \
  --config bench/competitors/stirling.toml \
  --ashift /path/to/ashift --out /tmp/ariad-competitors --preflight-only

timeout --signal=TERM --kill-after=10s 600s uv run --package ariad-bench python bench/competitors/run.py \
  --config bench/competitors/stirling.toml \
  --ashift /path/to/ashift --out /tmp/ariad-competitors
```

Preflight writes `preflight.json`, checks the installed pinned image/platform,
inspects its actual OpenAPI and attempts both routes for both products using the
original `fixtures/heavy/en-map` PDF and DOCX. These probes are prerequisite
checks, not corpus measurements. Missing scorers, corpus, routes or binaries
produce exit code 2 and explicit gaps. Image pulls are explicit, bounded setup;
the harness never silently selects a different tag, image or backend.

The full run writes aggregate `results.json` and `runs/<run-id>/attempt-*.json`
records. The aggregate identifies the current artifact directory; reruns never
mix their detailed attempts with an earlier run. JSON files are replaced
atomically. An
unsuccessful prerequisite check still writes blocked evidence, which is not
accepted by the public report renderer. A successful corpus run requires the
accepted fidelity-v2 scorer and the integrated authored editor corpus. Do not
copy old engine measurements into this evidence or describe a preflight as a
passing competitive benchmark.

## Pin and local isolation

[`stirling.toml`](stirling.toml) records the verified stable upstream release,
the immutable Linux amd64 manifest digest, verification date, source/licence
links, conversion endpoints and explicit options. This pin was resolved from
the official GitHub release API and Docker registry on 2026-11-10. Other
architectures require a separately verified platform digest, never emulation
silently applied to measurements.

Stirling's root licence covers its core under MIT and lists separately licensed
directories. The complete image also contains proprietary and third-party
components; the root MIT licence is not a licence for every image byte. The
harness records actual image IDs, repository digests and licence labels. This
evaluation does not add a product dependency or licence exception.

Each run creates a unique labelled container and dedicated internal bridge
network. The container has exactly that one network and no published Docker
port, host filesystem mounts or Docker socket. Docker internal networks disable
published ports, so a task-owned host relay binds only `127.0.0.1:18711` and
forwards to the inspected container address. The internal network blocks
external egress; no egress-capable second network is attached. HTTP clients
ignore proxy settings and reject redirects. A busy loopback port is an error,
and the harness never stops another task's container or selects another port.

The container is limited to two CPUs, 3 GiB and 512 processes. HTTP requests and
child commands have bounded timeouts; the run budget is 570 seconds, reserving
time for cleanup within the 600-second outer command limit. Signals and child
timeouts terminate owned process groups. Exit paths close relay sockets and
remove resources only after verifying their ownership labels. Docker image
layers remain a reusable local cache and are not committed.

## Corpus and evaluation contract

Corpus discovery reads `fixtures/manifest.toml` without changing it. It selects
original Apache-2.0 PDF and DOCX entries under `fixtures/editor/`, pairs files
by their relative stem, and requires exactly one authored `.truth.json`
companion per source. Every source/truth hash must match the manifest. Missing
pairs or truth block the corpus rather than shrinking a denominator. Fixture
generation belongs to the corpus owner and is never launched by this command.

Both products receive identical source bytes for each route, with OCR disabled.
AriadShift uses the editable profile and the digital PDF reader. Stirling uses
`outputFormat=docx` for PDF->DOCX and the LibreOffice backend
(`useStirlingOfficeConvert=false`) for DOCX->PDF. Conversion algorithms differ;
the digital-document/OCR policy and authored expectations are shared. Every
fixture/route/product has exactly three attempts. Product ordering alternates
between repetitions; startup is outside conversion timing. These are repeated
requests to the same local service, not three cold container launches.

Input extraction, conversion and output read-back/evaluation have separate
timings. Both products use the same hashed `ashift __ir` read-back binary and
the same scorer; read-back limitations apply to both and remain explicit.
No auxiliary renderer is used. If DOCX read-back cannot observe layout or a
style, that component remains unavailable with its denominator and reason.
Invalid PDF/DOCX artifacts, process failures, timeouts and unsupported backends
are failed attempts, never successful zero-error observations. Raw scorer
reports, output hashes, input hashes, image/runtime details and binary/config
hashes are retained. Temporary converted documents and read-back files are
deleted on exit; no private documents should enter this authored corpus.
Published JSON receipts replace machine-specific paths with `${WORKTREE}`,
`${ASHIFT_BIN}` and `${HOME}` placeholders. Measurements, hashes, resource
identities and command outcomes are unchanged; the coordinator's handoff
records the actual execution worktree. Retained JUnit receipts omit hostnames.

### Scorer integration

The scorer is late-bound through `--scorer ariad_bench.module:function` or the
`scorer` config value. The integrated configuration uses
`ariad_bench.competitors:score`, a thin adapter over the accepted
`ariad_bench.fidelity.compare` implementation. It exposes table F1 from the
table structure component and aliases coverage fields without changing values.

The callable accepts `(authored_truth_json, output_ir_json)` and returns a JSON
mapping or dataclass containing:

- `metric_version` and the unchanged raw scorer report fields.
- `text_cer`, `table_f1`, `style_accuracy` and `layout_iou`.
- `coverage` keyed by each of those four component names, with integer
  `matched` and `reference` counts (`0 <= matched <= reference`). A numeric
  component requires a nonzero reference denominator.
- `unavailable_reasons` whenever a component is null.

An adapter may translate field names and accepted canonical observations. It
must call the accepted metric implementation and preserve its results and
coverage; it must never introduce another metric or use a legacy fallback.
The harness does not align independently imported BlockIds. Alignment, style
inheritance and tolerances belong to the accepted fidelity-v2 implementation.
Missing modules/APIs or incompatible score objects are explicit gaps. No
production code invokes this Python adapter.

## Report and acceptance

```sh
uv run --package ariad-bench python -m ariad_bench report \
  --competitors /tmp/ariad-competitors/results.json --out /tmp/bench-report.md
uv run --package ariad-bench python -m ariad_bench report \
  --competitors /tmp/ariad-competitors/results.json --out /tmp/bench-report.md --check
```

Without an explicit argument, the report consumes
`bench/results/competitors/stirling-v3.3.0.json` when present. If absent, existing
base/heavy report bytes are unchanged. An explicitly missing or invalid evidence
path fails rather than silently dropping the comparison. The existing
base/heavy `generated_at` consistency check remains mandatory. The public
report is regenerated only after the final metric, corpus and heavy/base
measurements are integrated; do not append competitive prose by hand.

The PDF->DOCX gate is CER(A) <= CER(S), table F1(A) >= table F1(S), and
style(A) >= 1.10 * style(S), plus layout IoU(A) >= layout IoU(S).
Both routes show relative style gain and
percentage-point difference. For S=0, the inequality remains A>=0 and relative
gain is undefined; a zero/zero tie is never labelled infinite improvement.
Missing components or attempts in either route prevent acceptance. Cleanup
must complete before evidence can be accepted or the process can exit 0.
Means over observed values
are diagnostic only, with full observed/expected coverage printed beside them.
Failed or impossible gates require a recorded owner decision, never a relaxed
threshold. The coordinator records gaps in `.run/coord/to-pm/p11-gate-result.md`.

## Tests

```sh
timeout --signal=TERM --kill-after=10s 600s uv run --package ariad-bench pytest \
  bench/tests/test_competitors.py -k 'not test_stirling_real_local_pdf_docx_roundtrips'

ARIAD_BENCH_REAL_COMPETITORS=1 ASHIFT_BIN=/path/to/ashift \
  timeout --signal=TERM --kill-after=10s 600s uv run --package ariad-bench pytest \
  bench/tests/test_competitors.py -k test_stirling_real_local_pdf_docx_roundtrips
```

Unit tests use original hand-computed data only for report/schema arithmetic.
The timeout test launches real processes and checks that unrelated processes
survive. The separately opted-in integration starts the actual pinned image
and attempts four real conversions, validating actual PDF/OOXML artifacts.
No fake HTTP server or stubbed process success is used. A skipped integration
in the ordinary unit suite is reported as skipped and never satisfies that
required gate. A passing round-trip probe does not satisfy corpus acceptance.

The integrated command is `uv run --directory bench python -m ariad_bench.competitors --repeat 3`; it supplies the pinned configuration, release binary and `stirling-v3.3.0.json` output name to this same harness. The thin `ariad_bench.competitors:score` adapter calls the accepted `fidelity.compare` API and exposes table F1 plus unchanged component coverage. Layout IoU is also a required non-regression gate. `ARIAD_RUN_STIRLING_BENCH=1` enables the real local integration alongside the original opt-in variable.
