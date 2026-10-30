---
title: Review locked PyTorch model archives
date: 2026-10-28
summary: Bind twelve bundled model archives to reviewed wheel bytes and RECORD ownership.
---

# Review locked PyTorch model archives

## What happened

The ZIP local-header gate refused torch/bin/script_module_v4.ptl in the locked
aarch64 wheel. An audit of 216 compatible locked wheels found 148 embedded ZIP
occurrences, including twelve regression models shared by both Linux wheels.

## Decision

Commit ea43f406da7ae5aa2602b47a49ebe9372cc11232 permits only the reviewed
model distribution, version, path and SHA-256 with unique verified RECORD
ownership. Synthetic negative cases and the complete aarch64 extraction pass;
unknown archives, JAR and zipapp payloads stay refused.

## Next steps

Record the full exact-head gate in the canonical pack report. The independent
macOS libIex native-library refusal remains for separate review. Nothing was
pushed. AgentWiki publish skipped.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
