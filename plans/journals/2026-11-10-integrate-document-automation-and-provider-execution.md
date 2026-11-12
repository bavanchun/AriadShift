---
title: Integrate document automation and provider execution
date: 2026-11-10
summary: "Completed host wiring, batched watch execution, cloud runtime exposure and end-to-end verification."
---

# Integrate document automation and provider execution

## Outcome

Connected the offline document providers and consent-gated cloud broker to shared host execution. Added CLI watch batches, per-input artifact provenance, output-directory-aware idempotency, conditional handling for tools without inputs, and descriptive inpaint preview publication. Updated eligible Cloud runtimes, generated contracts, snapshots and user documentation.

## Verification

The required formatter, Clippy, core/host/WASM, CLI/MCP, server/OpenAPI, Python and spelling gates passed. The bilingual recipe evaluation passed all 50 semantic cases. Development packs were exercised with real generated documents; cloud tests used local HTTP endpoints.

## Execution notes

The shared Cargo cache contained metadata from another worktree and a removed Tauri plugin path. Scoped cache resets and holding the coordinator build lock across the verification sequence restored the correct artifacts. Set ASHIFT_PANDOC explicitly. The final test run used RUST_TEST_THREADS=1 after a parallel sandbox fixture hit ETXTBSY. The watcher debounce test now synchronizes after the first snapshot instead of assuming a writer thread runs within ten milliseconds.

## Release gate

Implementation is complete. Engine preview publication still requires the owner's explicit GO.

AgentWiki publish skipped.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
