---
title: Integrate native document engines with desktop and cloud
date: 2026-10-24
summary: "Preserve current Docling and cloud safeguards while sharing PDF, Typst and Office engines across native surfaces."
---

# Integrate native document engines with desktop and cloud

## What happened

The document-route branch was rebased first onto the merged desktop and cloud work, then onto the newer Docling follow-ups as the shared dev ref advanced. Five logical commits remain. Each passed compilation checks for host, CLI, server and desktop tests.

The shared dispatcher now serves PDF, Typst and LibreOffice alongside Pandoc. Desktop follows the common format registry and explicitly enables the built-in PDF engines. Worker sandbox failures can retry on another worker while retaining generic public messages. Current Tesseract grants, Windows runtime diagnostics and pack verification remain intact.

## Verification and corrections

The first full CI attempt rejected a raw cargo-about inventory because the newer generator normalizes whitespace. Regenerating with just licenses repaired the owning artifact. Full local CI then passed 919 native tests, 64 benchmark tests, four package verifications, Web and Desktop checks.

A real FUSE-mounted debug AppImage passed the required-sandbox Office conversion test. Its private temporary namespace preserved bundle libraries, and the produced PDF was parsed successfully. The temporary harness initially raced asynchronous FUSE cleanup; bounded cleanup waiting and a second successful run confirmed that its process and mount were gone.

## Next checks

Finish the real PostgreSQL/S3 service test, publish only the feature branch with the supplied lease, and verify the native, Desktop, Engine conformance and Docling pack workflows before the coordinator reviews it.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.

## Service verification

Later in the session, all 99 PostgreSQL/S3 integration tests passed without skips.
The feature publication and remote platform gates remain the next delivery checks.
