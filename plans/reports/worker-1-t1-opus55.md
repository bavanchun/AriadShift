# Worker p1-t1 (Opus 5.5 High): PR #49 high finding H1

Worktree: `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-bench-freeze`, branch `feat/heavy-route-bench-and-freeze`.

## Result

H1 is fixed in the working tree. Typst is the default PDF writer again for Markdown, HTML, EPUB and IR JSON. **The changes are NOT committed**: a stale `index.lock` blocks git (see Concerns). Nothing was built or run except `rustfmt --check` and a pure-Python render of the bench report.

## Root cause

Costs under the default `editable` profile: Typst path `md→IR→pdf` = 0.732, Pandoc+LibreOffice chain `md→IR→docx→pdf` = 0.562. The LibreOffice `docx→pdf` score (0.533 fidelity, 0.75 editability) was measured on *authored* Office DOCX, but the planner applied it to Pandoc DOCX generated from Markdown. The same flaw also routed `xlsx→pdf` through Docling → IR → Pandoc DOCX → LibreOffice instead of LibreOffice's direct edge (also routes for png/jpeg/tiff/webp/epub/docling+json → pdf).

## Fix

- `crates/ariad-core/src/planner/mod.rs`: new `pub const SOURCE_ONLY_ENGINES: &[&str] = &["libreoffice"]` with the rationale.
- `crates/ariad-core/src/planner/graph.rs`: Dijkstra skips a source-only engine edge unless it is the route's first step. All surfaces (CLI convert/plan, desktop, MCP, server, wasm) share this planner, so they get the same rule.
- Simulated over every format pair and profile: only routes that used LibreOffice on an intermediate change. They now go IR → Typst, and xlsx → pdf goes directly through LibreOffice. Office sources (docx/pptx/odt/…→pdf, doc→docx→…) keep LibreOffice as their first step.
- Coordinator option "keep Pandoc+LibreOffice as a non-default route": **not kept**. Faithful scored it slightly better (0.591 vs 0.625), but only because it used the authored-Office measurements, which don't apply to Pandoc output. So the measurements don't justify it. The bench still records the `md→pdf · libreoffice` route row as a comparison.
- `plan_with_available_engines` (host) is **unchanged on purpose**. `commands_cli.rs::namespace_denial_reports_installed_engine_and_preserves_the_planned_route` and docs/cli.md:110 pin an existing decision: when LibreOffice is installed but cannot start, an Office input fails with `sandbox_unavailable` instead of silently falling back to a different-fidelity route. Default plans for Markdown/HTML/IR JSON no longer include LibreOffice, so they cannot hit that error. A new Linux test proves it.

## Files changed (all uncommitted)

- `crates/ariad-core/src/planner/mod.rs`, `crates/ariad-core/src/planner/graph.rs`: the rule.
- `crates/ariad-core/tests/planner.rs`: `source_only_engines_never_render_an_intermediate` (synthetic: the better-scored chain is refused in all 4 profiles, DOCX still starts with LibreOffice, and with Typst unavailable there is NoRoute rather than a chain), and `ir_family_inputs_write_pdf_through_typst_in_every_profile` (embedded caps: md/html/json/epub → pdf end in Typst, no LibreOffice in the primary route or alternatives; xlsx → pdf is a single LibreOffice step).
- `crates/ariad-cli/tests/capabilities_coverage.rs`: md → pdf expectation restored to `markdown-ariad-core->ariad-ir+json`, `ariad-ir+json-typst->pdf`. **Boss addition:** dedicated `default_markdown_html_and_ir_json_to_pdf_routes_use_typst` pins the exact `Profile::default()` routes for Markdown, HTML and IR JSON → PDF against the embedded capabilities. It fails on any bench or capabilities refresh that flips them, and its message explains why Typst is the default (one shared writer, reproducible under SOURCE_DATE_EPOCH, milliseconds, no LibreOffice namespace sandbox, source-only LibreOffice, ARCHITECTURE decision).
- `crates/ariad-cli/tests/pdf_routes.rs`: removed both `--profile fast` workarounds, so the tests exercise the default profile again. New Linux-only `default_markdown_to_pdf_never_needs_a_blocked_libreoffice`: a broken `soffice` plus `ARIAD_TEST_SANDBOX_SUPPORT=namespace-denied` and best-effort mode; md → pdf succeeds with route `["ariad-core","typst"]`.
- `ARCHITECTURE.md`: §6.3 "Source-only engines" bullet, and a Decision Log row "IR → PDF writer".
- `docs/cli.md` (LibreOffice paragraph), `docs/bench.md` (the md→LibreOffice row is a comparison only).
- `bench/src/ariad_bench/report.py` and `docs/bench-report.md`: the same new paragraph in "Interpretation and limits". I checked locally that `render(capabilities, evidence)` == the committed `docs/bench-report.md` (True).

The freeze is untouched: no change to `schemas/`, `protocol.rs`, `ir/` or `capabilities.json`. `docs/cli.md:67` ("writes PDF through Typst") and `:604` (reproducibility) are now accurate again.

## Commits

None yet. `git add` failed: `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/.git/worktrees/01-AriadShift-wt-bench-freeze/index.lock` exists. It is 0 bytes and dated 18:08. `lsof` shows no holder, no git process runs in this worktree, and it was still there after 12 minutes of waiting (18:08–18:20). Per the rules I did not delete it. Once the coordinator confirms it is stale and removes it, run:

```sh
cd /Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-bench-freeze
git add crates/ariad-core/src/planner/graph.rs crates/ariad-core/src/planner/mod.rs crates/ariad-core/tests/planner.rs
git commit -m "fix(core): keep LibreOffice as a source-only renderer in the planner" -m "Measured LibreOffice scores come from authored Office files, so the planner no longer chains LibreOffice after another engine. Markdown, HTML, EPUB and IR JSON write PDF through Typst again, and XLSX to PDF uses LibreOffice directly instead of a Docling and Pandoc detour."
git add crates/ariad-cli/tests/capabilities_coverage.rs crates/ariad-cli/tests/pdf_routes.rs
git commit -m "test(cli): pin the default Typst PDF route for IR-family inputs" -m "Restore default-profile PDF route assertions, drop the fast-profile workaround, and check that Markdown to PDF succeeds when sandboxed LibreOffice cannot start."
git add ARCHITECTURE.md docs/cli.md docs/bench.md docs/bench-report.md bench/src/ariad_bench/report.py
git commit -m "docs: record Typst as the IR-family PDF writer"
```

## Verify (coordinator)

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
cargo test -p ariad-core --test planner
cargo test -p ariad-cli --test capabilities_coverage --test pdf_routes --all-features
cargo test -p ariad-cli --test commands_cli --test office_routes   # unchanged blocked-LibreOffice behaviour for Office inputs
uv run --package ariad-bench pytest bench/tests/test_heavy.py -k report
just ci
```

## Concerns

- Stale worktree index lock (above). The commits are pending.
- Not run: the tests are written but not compiled (no-build rule). The likeliest nit is rustfmt/clippy, and `rustfmt --check` passes on all edited Rust files.
- `crates/ariad-host/src/convert/routing.rs` still returns `SandboxUnavailable` when the built-in PDF *reader* cannot start. That is a reader path, not the default writer, so I left it out of scope.

Status: DONE_WITH_CONCERNS
