---
title: Integrate conversion benchmark preparation
date: 2026-11-10
summary: Integrated benchmark coverage, repaired spreadsheet readback and regenerated measured conversion data.
---

# Integrate conversion benchmark preparation

## Integration

Merged approved preparation commit `c5aac84e` into `feat/conversion-parity` as `32d84f09`. Removed pending-integration notes from the CLI reference and parity matrix. Matched accessibility-report engines and runtimes to the registry and linked URL/compliance regression tests.

## Measurement correction

A real workbook probe showed that Pandoc 3.12 drops inline-string cell contents. Readback now expresses those strings as shared strings in a temporary workbook copy, preserving the actual artifact and deriving content only from its cells. Added a regression test asserting cell text, table content and unchanged artifact bytes. The 26 conversion-edge tests passed, followed by all 103 benchmark tests.

Discarded an incomplete, unpublished calibration after discovering that readback defect. The completed full run used three cold repetitions and produced 110 heavy observations from 28 fixture inputs. Capabilities now measure 35 of 53 edges, adding CSV, XLSX, TXT, ODT, RTF and PDF-to-PPTX metrics. Existing measured quality and sample counts remained unchanged. Capabilities, route evidence and the generated benchmark report share timestamp `2026-11-09T20:58:52Z`.

## Verification

The initial release build stopped at its prescribed 600-second limit. The maintainer authorized continuation with a 20-minute build limit, direct benchmark/report execution and a 60-minute heavy limit. The resumed release build passed in 143.90 seconds; the final heavy run passed in 2569.29 seconds.

The prescribed locked checks passed for benchmark tests, capability invariants, Rust formatting, seven-package Clippy, and core planner/schema/registry tests. The core suite passed 35 tests, with two schema-generation helpers intentionally ignored. Updated two doctor snapshots from inspected CLI output to include the integrated veraPDF engine check; ten focused doctor tests then passed. The full ten-target CLI suite passed 133 tests. No route snapshot was changed beyond the approved preparation merge.

## Limits

The 64-level EPUB readback fixture reaches the existing conversion limit; its failed sample remains reflected in the measured EPUB score. TXT, CSV, EML, MSG, SVG and CBZ reader fixtures still lack independent truth and sufficient sample counts. Those edges remain unmeasured. Proprietary Office samples retain the documented local manual gate.

> Historical work record — not durable authority. Prefer docs, specifications and ADRs for current decisions.
