---
phase: 13
title: "Automation and AI: recipes at scale, watch folders, natural-language recipes, offline AI pack, watermark removal level 2"
status: completed
priority: P2
effort: "6 tasks, ~12 worker-days"
dependencies: [12, 6, 7]
branch: feat/automation-ai
milestone: C (the gem) — ends with engine preview 0.5.0
---

# Phase 13: Automation and AI

> Deep-mode note: scout-and-detail pass by the Mac PM before dispatch; every model/weight licence must be verified (CDLA-Permissive-2.0 or OSI) before pinning.

## Goal

Make AriadShift programmable and intelligent without giving up privacy: recipes everywhere, watch folders, natural-language → recipe, offline AI (summarise/ask/translate/extract), and raster watermark removal.

## Requirements

1. **Recipes v1 complete** (untrusted-recipe rules AD-18 apply; the watcher here is the only watcher implementation — macOS uses it through `ariad-ffi`): conditional steps (`if page_count > N`), per-file naming templates (`{stem}-{date}`), fan-out (split → per-part steps), error policy (stop/skip/continue); CLI `ashift recipe run --watch <dir>` (cross-platform watcher, debounced, idempotent via content hash ledger).
2. **Cloud tool jobs + remote MCP**: new server task (T7) — migration for tool jobs (`job_inputs` table for N inputs, nullable `target_format`, `tool_id`, `params` JSON **without secrets** — secrets only via short-lived handles never persisted), `pack` CHECK + `Pack` enum + queues/roles SQL (`infra/server-roles.sql`, `crates/ariad-server/src/queue.rs:157`, `tests/schema.rs:222`), per-engine worker images, `api/jobs.rs` + DTOs + additive OpenAPI `/v1` change + SDK regeneration, plan→tool access mapping (`plan_entitlements` extension). Only then do specs add `Cloud` to `runtimes`. Remote MCP tools `list_doc_tools`, `run_doc_tool`, `run_recipe` (update `crates/ariad-server/tests/mcp_remote.rs:152-155` exhaustive list).
3. **AI engine pack `ai`** (out of process, opt-in download): a small instruction model via llama.cpp (MIT, already an approved engine pattern in ARCHITECTURE §15) with weights under OSI/CDLA licence only (candidates verified at dispatch); tools `ai.summarize`, `ai.ask` (returns answer + page citations), `ai.translate` (IR in → IR out, structure preserved), `ai.extract_tables` (→ CSV/XLSX), `ai.recipe_from_text` (→ `ariad-recipe/1`, validated against the registry; never executes without user confirmation).
4. **Cloud AI (Pro, optional)**: provider adapter (Anthropic/OpenAI-compatible) used only when the user configures a key or Pro; document text sent only after explicit consent; never default.
5. **Watermark removal level 2 pack** `inpaint` (out of process): Florence-2 (MIT) detection + OpenCV Telea/NS (Apache-2.0) baseline inpainting; LaMa only after its weight + Places365 licence review passes (owner gate). Tool `pdf.watermark_remove_raster { pages, mask?: "auto" \| user rectangles }` with before/after preview images.

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Gemini 4 Argon high | Recipe v1 features + watcher `crates/ariad-core/src/tools/recipe.rs`, `crates/ariad-host/src/tools/recipe.rs`, `crates/ariad-host/src/watch.rs` |
| T2 | Gemini 4 Argon high | Remote MCP tools (`crates/ariad-server/src/mcp/**`, `tests/mcp_remote.rs`) on top of T7; starts after T7 merges |
| T3 | Gemini 4 Argon high | `ai` pack: `engines/ai/**` (adapter speaking `ariad-engine/2`), pack manifest, licence notices, model pinning |
| T4 | Gemini 4 Argon high | `ai.*` tool specs + host wiring + cloud provider adapter behind consent `crates/ariad-host/src/tools/ai.rs` |
| T5 | Gemini 4 Argon high | `inpaint` pack + `pdf.watermark_remove_raster` `engines/inpaint/**` |
| T7 | Gemini 4 Argon high | Cloud tool jobs (requirement 2): migration, server code, OpenAPI/SDK, worker images, access mapping, tests |
| T6 | Gemini 4 Argon high | Tests/evals: recipe golden runs, NL→recipe eval set (50 prompts EN/VI, ≥ 90 % valid recipes), ai.ask citation accuracy eval, watermark L2 visual checks |

## Acceptance

- [x] Watch folder processes 100 files idempotently (re-run does nothing).
- [x] NL→recipe eval ≥ 90 % valid and correct on the 50-prompt set (EN + VI).
- [x] No network access by `ai`/`inpaint` engines (sandbox verified); cloud path only after consent.
- [ ] Engine preview 0.5.0 (owner GO).

## Integration verification

Implementation is complete. The engine preview publication checkbox remains an owner release gate; this integration does not grant release approval.

- The CLI processes 100 PDFs in one watch batch, then skips all 100 reintroduced inputs without another recipe run. Ledger entries retain each input's artifact provenance and respect the selected output directory.
- Local AI and inpaint development packs run through the shared sandboxed engine runner. Cloud requests are brokered by the host only after consent and Pro-or-higher access or a user key. Recipe drafts are checked against registered tools and parameter schemas before publication.
- The 50-prompt English/Vietnamese semantic evaluation passes 50/50 cases. All 140 AI/inpaint Python tests pass.
- Formatter, workspace Clippy, core/host/WASM tests (1,438 passed; five existing ignored tests), CLI/MCP tests (74 passed), standalone watcher tests (20 passed), and server/OpenAPI tests (110 passed) pass. Tool schema and CLI snapshots match the 102-tool registry.
- Cargo checks use the coordinator's build lock with an explicit `ASHIFT_PANDOC`. The final tests use `RUST_TEST_THREADS=1` to avoid parallel executable-copy races in sandbox fixtures. Cloud provider tests use loopback HTTP; development pack tests do not download model weights or use a paid provider.
