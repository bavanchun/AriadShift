---
title: Native desktop link measurements
date: 2026-10-31
summary: Paired app links exposed an unnecessary ordinary-bin test build.
---

# Native desktop link measurements

## Evidence

Paired ordinary-app links changed the initial attribution. On macOS image
20261026.0455.2, dev already had 16,908,800 bytes of unwind data and the PR
16,929,056. Both exceeded 16 MiB. The old-image control also reproduced
this for dev and PR. The new integration-test target exposed the ordinary
app link; the prior dev test graph had not linked that target.

The largest live-symbol contributors were required Typst, desktop, core
and host code, with no accidental new dependency. Map totals describe all
live symbols, not unwind bytes per crate.

## Decision and checks

The narrow macOS binary lint allowance passed both native Verify and
Desktop, including packaging and a real PDF to IR to Typst PDF conversion.
The preferred cause-aligned alternative moves the two pure library tests
into the unit harness unchanged except for their crate import. Local
validation passed all 29 desktop tests with warnings denied. Native
Verify and Desktop validation of this alternative is still in progress.

The experiment is isolated on exp/macos-link-103. The PR remains at
65f20aa while native evidence is collected. No unwind-encoding or panic
strategy change is authorized or applied.

## Next steps

Complete native validation, preserve the measurements and image versions,
and make one normal push with the selected fix and report. Remove the
throwaway branch after collecting evidence. Keep the Ubuntu WebDriver
transport failure listed as a rerun candidate without adding retries.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
