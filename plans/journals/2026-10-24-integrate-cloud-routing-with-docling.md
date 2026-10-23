---
title: Integrate cloud routing with Docling
date: 2026-10-24
summary: Rebase cloud backend onto the merged Docling engine and preserve public format filtering.
---

# Integrate cloud routing with Docling

## Integration

Rebased `feat/cloud-backend` from `2bdd516` onto `origin/dev` at `00eee93` after PR #38 merged. The only conflict was `docs/ci.md`; the resolution preserves server integration, Docling fuzz coverage, and scope classifier documentation. Review approval for PR #37 remains the delivery context; its low findings are deferred by the maintainer.

## Regression evidence

Before the fix, three new routing tests showed that `docling+json` resolved as a public format, DoclingDocument uploads were accepted, and a PDF-to-PNG planning error advertised `docling+json`. The server now excludes it alongside the other internal interchange formats in name resolution, upload detection, and reachable-target errors. All seven routing tests pass after the fix, including content detection with misleading extensions. The existing API documentation already specifies this boundary.

## Remaining verification

Run the serialized `just ci` gate and commit lint, push with `--force-with-lease`, and watch CI on all three operating systems. Record final evidence in the requested coordinator report. The signed Docling pack publication and low findings remain outside this task.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
