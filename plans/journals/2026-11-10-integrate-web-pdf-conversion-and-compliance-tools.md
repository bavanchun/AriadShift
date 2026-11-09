---
title: Integrate web PDF conversion and compliance tools
date: 2026-11-10
summary: Retained 92 document tools and verified the merged conversion and compliance behavior.
---

# Integrate web PDF conversion and compliance tools

## What changed

Merged the reviewed web-to-PDF and compliance branches into `feat/conversion-parity`, retaining the existing comparison and validation tools. Resolved shared registration, dispatch, documentation and tool-count conflicts. Regenerated the combined 92-tool schema and CLI snapshot once after both merges.

Corrected the four spelling findings in the Unicode normalization test and visual comparison coordinates. Reconciled the veraPDF architecture descriptions and recorded AD-23: unmodified MPL-2.0 jars use system Java >= 11, with no bundled JRE/JDK.

## Verification

All requested formatting, Clippy, WASM, core, Typst, PDF/A, comparison, host and CLI/MCP checks passed through the shared build lock. The CLI suite exercised the real PDFium and veraPDF packs, including missing and old Java errors, compliant and non-compliant PDFs, and the zero-input web tool. Spelling passed, and the generated schema and CLI snapshot contain the same 92 unique tool descriptors.

The additional full-branch commit-message lint found two historical failures: commit `6e1357e` uses a disallowed type and `f25b825` has an overlong subject. Both precede the starting integration revision, as verified through their ancestry. Published history was preserved. Commit-message lint passes for the complete new integration range.

## Review

The integration review retained all tool IDs from the three registries and the existing comparison secret handling. Fixture manifest, workspace manifest, lockfile and parity matrix required no reconciliation. The feature branch is ready for its authorized final commit and push.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
