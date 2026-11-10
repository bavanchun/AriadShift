---
title: Repair web CI path resolution
date: 2026-11-11
summary: Fix working-directory-dependent WASM loading and spelling false positives.
---

# Repair web CI path resolution

## Cause and repair

The locale sitemap test failed from the repository root because the server
catalogue resolved its WASM file relative to apps/web. The loader now checks
the repository-relative path before using the existing web-relative path.

The spelling gate also flagged a French interface label and the intentionally
truncated search query in existing tests. Exact string exceptions preserve
those test inputs without excluding their files from spelling checks.
The formatter also corrected one existing file-picker test assertion.

## Evidence

The original root-directory test reproduced ENOENT before the repair. The
locale and public-page suites now pass all 14 tests, covering root and web
working directories. The locale, JavaScript licence and spelling gates pass.
The full root-directory Vitest run passed 1,083 tests, with 33 tests skipped.
The production build generated all 7,566 static pages and passed TypeScript
and the bundle-based notices check. Biome passed across 372 files.
The staged feature tree passed the secrets scan.

## Delivery

Keep the incoming staged feature changes and the existing dev fixes intact.
Create one conventional commit on feat/web-foundation-2 and send the requested
coordinator report after the remaining verification completes.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
