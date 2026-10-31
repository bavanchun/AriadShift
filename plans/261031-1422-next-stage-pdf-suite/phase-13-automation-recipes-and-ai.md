---
phase: 13
title: "Automation and AI: recipes at scale, watch folders, natural-language recipes, offline AI pack, watermark removal level 2"
status: pending
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
| T4 | Gemini 4 Argon high | `ai.*` tool specs + host wiring + cloud provider adapter behind consent `crates/ariad-host/src/ai/**` |
| T5 | Gemini 4 Argon high | `inpaint` pack + `pdf.watermark_remove_raster` `engines/inpaint/**` |
| T7 | Gemini 4 Argon high | Cloud tool jobs (requirement 2): migration, server code, OpenAPI/SDK, worker images, access mapping, tests |
| T6 | Gemini 4 Argon high | Tests/evals: recipe golden runs, NL→recipe eval set (50 prompts EN/VI, ≥ 90 % valid recipes), ai.ask citation accuracy eval, watermark L2 visual checks |

## Acceptance

- [ ] Watch folder processes 100 files idempotently (re-run does nothing).
- [ ] NL→recipe eval ≥ 90 % valid and correct on the 50-prompt set (EN + VI).
- [ ] No network access by `ai`/`inpaint` engines (sandbox verified); cloud path only after consent.
- [ ] Engine preview 0.5.0 (owner GO).
