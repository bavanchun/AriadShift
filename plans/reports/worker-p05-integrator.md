# Optimize and security integration gate

Status: gate-ready for the accepted phase scope. The original compression
acceptance target remains unmet: the delegated PM accepted 6/10 as a recorded
phase deviation, with [text-heavy parity tracked in issue #187](https://github.com/bavanchun/AriadShift/issues/187).
This is not a claim that the original 7/10 target passed. Distribution remains
subject to the separate owner-pending qpdf licence reviews and release controls.

## Integration and decisions

The fetched base was `origin/dev` `b3b76a80cefc655fcb81dedf26a6ebbaf65eb023`,
merged as `fbe54969547c10ebf18bfb8b1391d565626af3a9`. The initial phase head
was `d3af6777443645750afdf370168f29a72a25d901`; s3
`f6bcfb1245b4c2a1bcae82944cdc55ae6cce766a` was merged as
`6ccff7ff5eae42f7e533820c25a1f39f706584ce`. The other initial slice heads were
c0 `08b09fab87a69c806e81aaa7bb6dd9edd4a878c3`, s1
`95217d9f2a0d52cb80421cb001b1db3f39f0ae36`, s2
`6c3700728eee56cc95a747ab3c360dff3329d75b`, s4
`7d3ce010296be2a9d743dec94ac5b5a4cce9d8cc`, and s5
`2394bfd402c312593e35066d22913b1094aa90d4`. The merged encoder, font and
final investigation records are `worker-p05-s2b.md`, `worker-p05-s2c.md` and
`worker-p05-s2d.md`. The tested product revision was
`7486d50b55b9929781498b7b6e2a0b0cb5dc42c5`; the subsequent change before this
report only adds the `loca` and exact Type 1 byte tokens to the spelling
configuration. It does not alter product code or test behavior.

The Mac PM approved D1–D14, the v2-only D6 wire error, local single-use MCP
handles, shared `signed_input_rewritten` diagnostics, the source-built qpdf
approach for macOS, and the documented 6/10 deviation. The frozen v1 engine
and pack schemas match `origin/dev` byte for byte. qpdf distribution is still
blocked on owner-pending IJG and GNU runtime review; no new general licence
exception or unsigned pack index entry was added.

The integration registers the tools, schemas and dispatch, forwards coded
warnings and metrics to native and WASM surfaces, warns when rewriting signed
AcroForm input, implements recipe stdin and hidden terminal secret prompts,
and adds registered-tool CLI and MCP tests. The signed manifest remains the
executable authority; the host pack tests cover rogue executables, a modified
receipt and Linux sandbox inheritance. Test-only protocol probes now have an
explicit entrypoint, keeping synthetic requests separate from the real qpdf
adapter. Tool and engine snapshots, names, docs, fuzz cases and generated
notices were updated. Existing in-memory signature dictionary assertions remain.

## Linux gate evidence

Every heavy command below ran sequentially via
`/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh`, with the shared
`CARGO_TARGET_DIR=/home/vchun/.cache/ariadshift-target`. The environment used
`ASHIFT_PANDOC=/home/vchun/Codes/01-AriadShift/.tools/pandoc/bin/pandoc`,
`ARIAD_TEST_PDFIUM_DIR=/home/vchun/Codes/01-AriadShift/.run/coord/pdfium-pack/bin`,
`ASHIFT_PACK_PDFIUM=/home/vchun/Codes/01-AriadShift/.run/coord/pdfium-pack`,
`ARIAD_TEST_QPDF_DIR=/home/vchun/Codes/01-AriadShift/.run/coord/qpdf/12.4.2/linux-x64`,
and `ASHIFT_PACK_QPDF=/home/vchun/Codes/01-AriadShift/.run/coord/qpdf-p05-dev`.
`SOURCE_DATE_EPOCH` was unset for the suites, because the writer tests compare
an unset value with an explicit override. The comparison additionally used
`ARIAD_TEST_STIRLING_DIR=/home/vchun/Codes/01-AriadShift/.run/coord/p05/stirling-outputs`.
Metadata headroom was checked before heavy steps; the last check showed 9.03 GiB
used of 10.51 GiB allocated. No data or other session's process was removed.

Logs named below are in the main checkout under
`.run/coord/p05/integrator-logs/`. Durations exclude lock waiting.

| Gate | Command and result | Duration / log |
| --- | --- | --- |
| Pack | `env UV_NO_CONFIG=1 just pack-qpdf "$ARIAD_TEST_QPDF_DIR" <development-pack>`: PASS; manifest and notices verified with the newly built CLI. The development binary reports qpdf 12.4.2. | 23.3 s; `gate-pack.log` |
| Format | `cargo fmt --all --check`: PASS. | 2.6 s; `gate-format.log` |
| Clippy | `cargo clippy --locked -p ariad-core -p ariad-pdf -p ariad-host -p ariad-server -p ariad-cli --all-targets --features ariad-pdf/codecs,ariad-host/test-probe -- -D warnings`: PASS. | 18.9 s; `gate-clippy.log` |
| WASM | `cargo check --locked -p ariad-pdf -p ariad-wasm --target wasm32-unknown-unknown` and `cargo test --locked -p ariad-wasm`: PASS; 16 tests. | 0.4 s and 3.0 s; `gate-wasm-check.log`, `gate-wasm-test.log` |
| Pack tooling | `env UV_NO_CONFIG=1 just pack-tooling-test` and `bash scripts/test-pack-qpdf.sh` with `QPDF_TEST_PACK` set: PASS; 55 shared checks, qpdf checks passed. | 2.2 s and 0.7 s; `gate-pack-tooling.log`, `gate-qpdf-test.log` |
| Core/PDF | `cargo test --locked -p ariad-core -p ariad-pdf -p ariad-engine-io --features ariad-pdf/codecs`: PASS with the approved 6/10 baseline. Four tests were ignored: three schema generators and one release-only timing test. | 2m52s; `gate-core-pdf.log` |
| Host | `cargo test --locked -p ariad-host --features test-probe`: PASS with both real packs and no ignored host tests. The first run with Stirling comparison included exceeded ten minutes after 9/10 rows and was terminated; the full suite was rerun without that extra comparison, and the comparison was completed separately below. | 6m36s retry; `gate-host.log`, `gate-host-retry.log` |
| CLI | `cargo test --locked -p ariad-cli --features ariad-host/test-probe`: PASS with both real packs. Eleven unrelated Docling/AppImage tests were ignored; the real qpdf and PDFium checks were configured. | 4m34s; `gate-cli.log` |
| Server | `cargo test --locked -p ariad-server`: PASS. | 1m34s; `gate-server.log` |
| Measurement | `cargo test --locked -p ariad-host --features test-probe --test compress_similarity corpus_size_and_similarity_against_stirling -- --nocapture`: PASS against the approved baseline. Ten input rows, ten target results and ten finite Stirling SSIM values matched the supplied TSV in order; the same six files met both size and SSIM thresholds, with zero errors. Target sizes were within 5% for 0/10; the chosen closest outputs and warning are documented in the measurement report. | 6m37s; `gate-measurement.log` |
| Smoke | `bash .run/coord/p05/smoke.sh` using the newly built CLI and both packs: PASS; compress, encrypt, decrypt, repair, linearize, sanitize and doctor each exited zero. Six nonempty PDFs and JSON results are in `.run/coord/p05/smoke/`; doctor reported both engines ready. | 3.5 s; `gate-smoke.log` |
| Policy | `env UV_NO_CONFIG=1 just licenses-check`, `just deny`, `just spell`, and `bash scripts/test-ci-scope.sh`: PASS. CI scope: 93 checks. The first spell run failed on valid font tokens; the corrected dictionary passed. | 1m20s, 3.7 s, 0.4 s, 1.6 s; `gate-licenses.log`, `gate-deny.log`, `gate-spell-retry.log`, `gate-ci-scope.log` |
| Fuzz | In `fuzz/`, `cargo check --locked` and `cargo check --locked --features pdf-reader`: PASS, including the phase tool targets. | 7.9 s and 1.4 s; `gate-fuzz.log`, `gate-fuzz-pdf.log` |
| Commits | `sh scripts/check-commits.sh origin/dev..HEAD`: PASS before this report; repeat after the report commit. | 1.0 s; `gate-commits.log` |

An earlier core/PDF attempt incorrectly set `SOURCE_DATE_EPOCH` globally and
failed a writer test that deliberately compares the unset and set cases. The
successful gate above explicitly unsets it. Before final verification, an
outdated synthetic qpdf probe and CLI inventory snapshots failed and were fixed;
the affected runner tests, snapshots and full surfaces were rerun. No test was
weakened to hide those failures. The compression assertion is the explicit
accepted 6/10 phase baseline, not a claim that 7/10 was reached.

## Review and outstanding Mac work

The integration review examined the shared signature helper, secret channels,
manifest executable enforcement, sandbox path and relevant font code. It was a
scoped inline review, not an independent audit of the entire 153-file phase.
The s2c report records conservative font exclusions, including unsupported
simple-font encodings and raw CFF, which remain within issue #187's follow-up.
No Stirling output PDF or user document was committed.

On macOS, build a source qpdf development pack and run
`ARIAD_TEST_QPDF_DIR=<source-built-qpdf-dir> ASHIFT_PACK_QPDF=<pack-root> cargo test --locked -p ariad-host --features test-probe --test qpdf_adapter --test qpdf_secrets`,
including the Seatbelt child-confinement case. Complete the phase PR's macOS
build, signing and native checks on the Mac/CI. `apps/macos` and `ariad-ffi`
are not present in this tree, so their future bindings cannot be tested here.
These Mac checks do not block the Linux phase gate under the PM directive.
The unsigned Linux development qpdf pack is for local testing only; signed
release publication and its owner-pending licence reviews are separate.

No worker branch or worktree was removed. The smoke password file was temporary
and deleted; smoke artifacts and the development pack remain in coordination
storage for review. The local `.tools/bin/committed` points to the existing
pinned 1.1.11 tool in the main checkout. No process started by this integration
is left running.

## Merge of updated dev

Merged `origin/dev` at `da078685ffe8c2f4e665b69e7cdb3a5a2fb7f2ca` as
`94a15dfaf487ac3e64923a0db85ca12baf654635`. The sole conflict was in
`THIRD_PARTY_LICENSES`; Cargo resolved both lockfiles with zip 9.2.1, and
`just licenses` regenerated Rust, desktop and web notices. The web and desktop
outputs had no tracked changes. The updated lockfiles also contain Cargo's
resolved Windows dependency edges.

Using the shared build lock and both real qpdf and PDFium packs, fmt, the
five-package clippy command, core/PDF/engine-io tests, the complete host suite,
the CLI suite, `just licenses-check`, `just deny` and the commit check passed.
The host suite was run in two complementary parts: all tests except
`corpus_size_and_similarity_against_stirling`, then that exact test separately.
Two initial whole-suite runs hit the ten-minute execution limit during corpus
measurement after compilation; their logs are retained. The first two licence
checks failed because the mise pnpm shim lacked a selected version; the final
check passed with `MISE_PNPM_VERSION=12.9.1`. Detailed logs and results are in
`/tmp/agent-herdr/runs/p05int/` on the integration machine.

## Review fixes and CI portability

The seven review fixes are `483426e0` (additional actions), `1b93848a`
(inline-image memory), `10c63df4` (single-use handles), `be0ee1f1`
(argv prompts), `add8736c` (recipe prompts), `8213d838` (secret input
bounds), and `fa42a36c` (signature warnings). Their regression coverage was
inspected against the requested cases. The MCP control-byte assertions were
moved to an empty store because a full store could reject them for capacity.

The qpdf build allowlist accepts the musl libc soname in `6237f60c`; other
unlisted libraries remain rejected. The host pack test canonicalizes its
expected executable path in `43182419`, preserving the actual resolver check.
Shellcheck 0.11.0, shell syntax and positive/negative dependency allowlist
cases passed. A Linux run passed the pack resolution test; macOS, Windows and
musl source builds remain for PR CI to verify.

Using the shared build lock, the affected PDF `sanitize`, `color_rewrite` and
`compress` tests, core `tools_registry`, CLI `mcp_secrets` and
`secrets_surfaces`, and host secret-bag regression passed. The schema generator
in `tools_registry` was ignored as designed. Clippy passed for core, PDF, host
and CLI with codecs and the host test probe. An initial CLI unit-test filter
matched zero tests; it is not counted as validation.

The added regression `rejects_control_bytes_in_an_empty_store` passed in
`bd75e6fd`. The final `cargo fmt --all --check` passed. The focused commands
were `cargo test --locked -p ariad-pdf --features codecs --test sanitize --test
color_rewrite --test compress`, `cargo test --locked -p ariad-core --test
tools_registry`, `cargo test --locked -p ariad-cli --features
ariad-host/test-probe --test mcp_secrets --test secrets_surfaces`, the named
host pack and secret-bag tests, and the named CLI unit test. Clippy used
`cargo clippy --locked -p ariad-core -p ariad-pdf -p ariad-host -p ariad-cli
--all-targets --features ariad-pdf/codecs,ariad-host/test-probe -- -D warnings`.
All Cargo commands used the shared lock wrapper.
