---
title: DOCX to PDF style preservation
status: completed
---

# DOCX to PDF style preservation

## Outcome and constraints

Preserve DOCX styles through PDF export and native read-back so the unchanged
authored corpus reaches style accuracy >= 0.704 against Stirling-PDF 3.3.0
under the existing repeat=3 protocol. Keep all metric thresholds and authored
text/style expectations unchanged. Add reference geometry only from verifiable
original authoring. No conversion route or dependency changes are planned.

The brief's historical `fixtures/golden/stirling/paired.ir.json` path does not
exist. The manifest and harness own the current pair under `fixtures/editor/`.

## Steps

- [x] Trace style loss through DOCX import, the actual default LibreOffice route
      and PDF read-back: the isolated Office profile lacked bundled fonts.
- [x] Fix the evidenced cause and add focused regression coverage.
- [x] Verify whether original authoring provides legitimate block geometry;
      populate reproducibly or document the exact limitation.
- [x] Run required Rust and benchmark checks; apply the coordinator's narrowed
      Rust rerun authorization after the unrelated PDFium registry drift.
- [x] Run the pinned competitive benchmark three times per product/direction,
      regenerate its public report and verify resource cleanup.
- [x] Review the implementation and prepare the verified branch for publication.

## Verified result

The benchmark tested production revision `b262a75e95bc7681cce802c64ee608b2b9cc7202`
with binary SHA-256 `71a03f8806c51b248876b64202144771a471501251b8f81825782d94f15f6a9f`.
All 12 paired attempts succeeded with three observations per product and route.
Every component comparison passed independently for both routes.

| Route | Product | CER | Table F1 | Style | Layout IoU |
|---|---|---:|---:|---:|---:|
| DOCX to PDF | AriadShift | 0.3728813559322034 | 0 | 1.00 | 0 |
| DOCX to PDF | Stirling | 0.3728813559322034 | 0 | 0.64 | 0 |
| PDF to DOCX | AriadShift | 0.3728813559322034 | 0 | 1.00 | 0 |
| PDF to DOCX | Stirling | 0.9067796610169492 | 0 | 0.00 | 0 |

Style retains 25 reference observations. Geometry adds nine body boxes and two
furniture boxes from the unchanged authored PDF, checked against independent
PDFium glyph bounds. PDF-to-DOCX read-back has no output boxes; DOCX-to-PDF
matches eight body boxes but their coordinates do not overlap the authored PDF
layout. Both zero-IoU ties pass the inequality without proving visual preservation.
Table F1 remains zero for both products under the existing native table coverage.

Validation: formatting and workspace Clippy passed. The four-crate Rust run
recorded 1,528 passes before the pre-existing PDFium tool-list mismatch; the
coordinator authorized its repair and a narrow rerun instead of repeating that
suite. The repaired PDFium target passed all 11 tests. The real LibreOffice
regression passed with style 1.00. Python benchmark tests passed 167 with one
separate Docker test skipped, then the opted-in competitor target passed all 24
including real Docker. Generator tests passed 11 and its 50 files passed drift
checking. The generated report matches the committed measurements.

The first preflight converted every probe but exceeded Docker cleanup's fixed
budget. All resources disappeared; an unchanged rerun passed preflight, corpus
and integration with completed cleanup. No protocol timeout or metric changed.

Evidence: [competitive measurements](../../bench/results/competitors/stirling-v3.3.0.json),
[generated report](../../docs/bench-report.md),
[integration receipt](../../bench/results/competitors/integration.xml).
Final branch publication and coordinator notification are recorded in the
coordinator's requested `p11-style-result.md` after push.

## Acceptance

DOCX to PDF style(A) >= 1.10 * style(S), CER(A) <= CER(S), and table F1(A) >=
table F1(S). If reference geometry is legitimately available, measure both
directions and require layout IoU(A) >= layout IoU(S). Record all coverage and
limitations without changing denominators. Required verification commands come
from the task brief and execute through the shared build lock.

## Risk and rollback

PDF metadata and inferred style evidence must remain conservative for unrelated
fonts. Reference geometry must come from the original PDF, never a converter's
hypothesis. Revert scoped commits to restore prior behavior.
