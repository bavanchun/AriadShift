---
title: Native desktop link measurements
date: 2026-10-31
summary: Paired native links exposed an unnecessary app build during tests.
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
Verify and Desktop also passed on image 20261026.0455.2, including 1,255
workspace tests, installer/signature checks and packaged PDF conversion.

The experiment stayed isolated on exp/macos-link-103 while the PR remained
at 65f20aa. The selected repair keeps every warning denied, preserves the
public assertions and changes no production runtime code. No unwind
encoding or panic strategy changed.

## Delivery and remaining work

The source gate at d5c986a passed desktop tests, Clippy, static checks and
commit lint. The report links the final delivery and single-push receipts,
per-run images and saved experiment source. The Ubuntu WebDriver transport
failure remains a rerun candidate; no retry policy changed. Reducing the
already oversized unoptimized app is recorded as a v1.x follow-up in the
owning desktop documentation.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
