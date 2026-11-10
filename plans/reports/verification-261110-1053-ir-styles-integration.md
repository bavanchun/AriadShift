# IR styles integration verification

Status: implementation and verification complete; competitive phase acceptance pending the owner's Condition 2 decision.

## Source and immutable boundaries

The seven accepted slices were merged in the prescribed order onto `feat/ir-styles-fidelity`, ending at merge `af72c227`. Integration repairs were measured from the dirty worktree after the final production edit; measurement metadata retains that actual source revision rather than claiming a later delivery commit.

The final release binary SHA-256 is `f019dbab67842f3388dcc1399e3f6401d2f75ce37ac4a0fcefb63dd3891ed6ed`. Release compilation passed in 2m37s after the final Rust production repair. All final measurements use this binary. Generated calibration data is refreshed by those measurements; the final workspace batch builds against the refreshed data. The subsequent web-only response-race repair does not affect this binary, benchmark inputs or results.

- Frozen `/1` schema: `92d3ef590f3f8dde533c938e14d5c7695b877978bc9cb246fc547aed976d96b5`.
- Published `/2` schema: `366d4141ccff677c58c6837733483b498449dc8de71e99b1df490b575ecfdd89`.
- Fixture manifest: `acc840496a72844c4da81cf25d27b49ded73945a7d5d433a5f9776b30bd7f5f4`.
- Authored editor fixtures and truth are unchanged from the accepted slices. Native PDF paths require zero recovered tables against one authored table; DOCX-only paths require the authored table.
- Frozen `/0`, `/1` and legacy goldens are unchanged from the integration base. Current reader and route snapshots use `fixtures/golden/v2`.
- Architecture decision IDs are unique; AD-24 through AD-27 record the integrated contracts. All 88 local documentation links checked before delivery resolve.

## Focused verification after the final repair

All heavy commands use `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh`.

| Check | Result |
|---|---|
| Actual PDF with unsupported Lab colour, absent and present redaction targets | Pass; style loss cannot disable text verification |
| Core PDF style tests | 10 passed |
| Core PDF reader tests | 18 passed, 1 existing ignore |
| Real PDFium redaction pipeline, including Vietnamese CID fonts | 27 passed |
| CLI PDF warning snapshot | 1 passed; only current warning classification changed |
| DOCX text-box parser boundaries and XML limits | 3 passed |
| Real host/memory DOCX styles, including text boxes and repeated image identities | 9 passed |
| Strict editor corpus, including all unconditional `/2` assertions | 6 passed |
| Actual captured Stirling DOCX read-back | Passed; one heading matches Pandoc semantic output, omitted text-box content is explicitly warned |
| Workspace clippy, all targets and warnings denied | Passed |
| Complete CLI suite | 316 top-level tests passed, 0 failed, 11 existing ignores; 2 additional sandbox child probes passed |
| Format | Passed |

The [independent text-box review](code-reviewer-261110-1238-docx-textbox-import.md) confirms the story boundary and conservative `unsupported_node` warning. The actual Pandoc AST contains only one heading, so complete text-box content preservation is not claimed. The final actual-artifact read-back succeeds without synthesizing missing text.

The [independent warning review](code-reviewer-261110-1053-color-warning-verification.md) confirms that Unicode mapping loss, content truncation and fail-closed publication gates remain unchanged. Its stale fixture assertion finding was repaired and the complete PDF reader suite passed.

Earlier successful checks include codec-enabled PDF coverage (570 passed, including all three QR tests), real host DOCX round trips (8 passed), memory-package tests (6 passed), repeated reader goldens (3 passed), sandbox runner/pack probes (58 passed), and the veraPDF resolver unit. The final clean workspace results below supersede earlier default-suite counts.

## Fresh measurements

Base `repeat=3` completed: 889 attempts, 705 versioned samples, 606 measured and 99 unavailable. The generator directly computes 91 eligible scoring fixtures; partial preservation-only Markdown/HTML companions remain explicitly unscored. Deep-list EPUB exceeds the nesting limit, and mixed-document CSV independent read-back exits 64; both actual failures remain in the evidence.

Heavy `repeat=3` completed in 34m19s: 28 fixtures, 110/110 measured samples, both OCR modes. The two heavy reports are identical. All 53 baseline engine-edge tuples remain; `bench-check` passed. Base and Stirling reports carry the same final binary hash. The metadata-only eligibility check leaves the generated fixture count at 91 and verifies unchanged edge metrics.

Stirling preflight is ready, with no gaps and 4/4 valid artifact probes. The opted-in real Docker suite passed 24 tests with no skips. One authored paired PDF/DOCX produced 12/12 scored attempts across two products, two directions and three repetitions. The corpus exits 2 (`incomplete`); report generation and report check pass.

| Direction | Product | CER | Table F1 | Style accuracy | Layout IoU |
|---|---|---:|---:|---:|---:|
| pdf->docx | ariadshift | 0.3728813559322034 | 0.0 | 0.68 | N/A |
| pdf->docx | stirling | 0.9067796610169492 | 0.0 | 0.0 | N/A |
| docx->pdf | ariadshift | 0.3728813559322034 | 0.0 | 0.4 | N/A |
| docx->pdf | stirling | 0.3728813559322034 | 0.0 | 0.64 | N/A |

All numeric CER/table/style observations have coverage 3/3. Layout coverage is 0/3: authored page geometry is not a reference block box, and the immutable truth has zero such boxes.

- PDF->DOCX passes the accepted CER, table and style inequalities. Style delta is +68 percentage points. Stirling's measured style baseline is zero, so relative percentage gain is undefined and is not claimed.
- DOCX->PDF ties CER and table F1. AriadShift style 0.4 versus Stirling 0.64 is -24 percentage points and -37.5% relative; the unchanged minimum is 0.704, so the style gate fails.
- Both layout gates remain unavailable. No threshold or truth was changed. Formal phase acceptance needs the owner's recorded decision under Condition 2; the exact question is in the coordinator's `to-pm/p11-gate-result.md`.

All benchmark-labelled Docker containers and networks were removed; port 18711 is free. Cached image layers and shared build caches remain. Final attempt receipts are under `bench/results/competitors/runs/5196da072d1c4a5ab082e466ebc193ff`. Superseded receipt directories were hash-checked into the runtime archive and removed from the untracked delivery set.

Earlier complete measurements are archived under `/tmp/p11-int-evidence/pre-textbox-measurements`. Their clean workspace batch was intentionally stopped with exit 143 after the text-box regression was diagnosed.

## Final workspace verification

The required batch started with the five-package `cargo clean` and held the shared build lock throughout all 23 commands. Twenty-two commands passed; the first web run found a real asynchronous response race. The original exit 1 remains in `workspace-gates.json`. A later held-lock closure reran the affected checks after the verified repair; `workspace-web-closure.json` records their success.

| Check | Final result |
|---|---|
| Format, workspace clippy with warnings denied | Passed |
| Minimal core and wasm32 checks | Passed |
| Core | 758 passed, 4 existing ignores |
| PDF default | 374 passed |
| Typst | 4 passed |
| Host | 615 top-level tests passed |
| CLI | 316 top-level tests passed, 11 existing ignores; 2 additional child probes passed |
| Full Python suite | 166 passed, 1 opt-in Docker skip; the separate opted-in suite passed all 24 tests |
| Editor corpus Python tests and immutable corpus checker | 4 passed; checker passed |
| Frozen JavaScript install and actual WASM bindings build | Passed |
| Focused account tests and complete web suite after repair | 23 and 308 passed |
| Biome on all five modified TypeScript files | Passed |
| Dependency licence gate, spelling, benchmark consistency and generated report check | Passed |

Deleting an account job aborts an outputs request, but the promise continuation previously still stored a late response and displayed its download after deletion. The regression test first reproduced that failure deterministically, then passed with the original focus and download-removal assertions intact. The component now ignores aborted responses and renders outputs only for successful, undeleted jobs. The [independent review](code-reviewer-261110-1355-job-output-race.md) reports no findings. This isolated UI repair requires no new Rust or benchmark measurements.

The four core ignores are existing large/manual schema cases; CLI ignores include the nine optional Docling cases. No strict editor assertion is ignored or conditional. Earlier progress counting of 318 CLI successes included the two child probes; the table above distinguishes them from top-level tests.

## Logs and delivery

The brief's `just check-commits` recipe does not exist. The documented replacement, `just lint-commits`, ran and failed on the already published accepted T7 commit `28adc2a2a923d486fb2c5715e9e0a776f36bfc9d`: one body line is 183 characters, exceeding 72. That commit is reachable from `origin/p11/t7`. The same checker passes for all new integration commits (`af72c227..HEAD`). No published history was rewritten and no lint exception was added; the full-range commit-metadata gate remains an explicit delivery concern for the coordinator.

Runtime receipts and command logs are retained under `/tmp/p11-int-evidence`. The coordinator result will record the final commits, ordinary feature-branch push and notification receipt. No merge into dev, release or public deployment is part of this delivery.
