# Overlay and watermark integration

Status: in progress. The branch integrates the approved D1-D10 contracts and `origin/dev` at `ff8603f6`. The final gate remains open.

The registry and generated schema now include the overlay tools. The host stages a PDF artifact while retaining a structured removal report; WASM exposes the same report in `data`. Focused host, WASM, CLI and MCP tests passed. Fuzz wiring adds `pdf_draw_params` without changing the existing `pdf_tools_structural` nightly and just recipes; the existing CI omission of `pdf_tools_structural` is unchanged.

The PM-dispatched Argon review at `.run/coord/to-mac/p06-argon-review.md` found H1-H3, M1-M3 and L1-L2. The host suffix build failure was fixed in `8cb427ef`. The worktree addresses bounded Form copies and hashing, shared indirect optional-content arrays, removable mark isolation, image caching and unsupported-image reporting, nested Form identity and candidate overlap, resource cleanup, and consistent content-stream resolution. Focused watermark tests passed (26 integration tests and two internal regressions). PDFium confirmed exact pixels after both basic watermark removal and stamp removal following a partial colour rewrite. A final independent delta review and the full gate are still required.

The notices generator passed after installing the locked JavaScript dependencies from the existing offline store. An initial attempt failed because workspace JavaScript dependencies were absent; its truncated desktop notice file was restored before regeneration. Platform-specific viewer and phone QR checks remain pending.
