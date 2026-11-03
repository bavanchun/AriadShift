# Worker report: phase 1, T3 (docs/parity-matrix.md)

Status: DONE_WITH_CONCERNS (content complete; open items below need the Mac PM).

## What changed

- Created `docs/parity-matrix.md` (about 325 lines). Nothing else edited.
- Contents: tier rule (a-d), parity matrix by group (108 rows), "Beyond parity" (10 rows), platform capabilities (7), "Not adopted" (14 entries with reasons), gaps against the phase files (the `†` rows), counts, and a Stirling index (S01-S52 dispositions).
- Every Pro row is marked `proposed`. The approval line placeholder is present: "Pro list: pending approval by the Mac PM on the owner's delegation. Approved on: ____".
- Pro rows: `pdf.sign_token` (b), `edit.page_objects` with `edit.apply` (d), `recipe run` and `recipe run --watch` above a proposed cap of 25 files or 250 MB per run (c). Cloud execution and AriadShift-hosted AI are Pro as runtimes (a). Nothing else is Pro.

## Counts checked

- Stirling report §2 feature table: 52 rows, numbered S01-S52 in report order. All 52 appear (scripted check). Six are Not adopted entirely, five are platform capabilities, five are partly Not adopted, 36 map to tool or route rows.
- Bento report §2 inventory: 124 names enumerated. The report itself says 125 tool pages and 124 README names and groups some names, so I could not reconcile one to one. All 124 are accounted for: 114 in matrix rows, 3 in the platform table, 7 Not adopted (scripted check on exact name strings).
- Plan tools: all 19 phase 3 tools and the tools listed in phases 2, 4-10, 12 and 13 are present.

## Commands run

- Python coverage script (scratchpad): Stirling ids, Bento names, row counts. Result: no missing ids; two Bento names (MOBI, FB2) were only in a combined cell, fixed to explicit names; rerun clean.
- `build-lock.sh just spell` (waited about 4.5 min for the lock): no findings. `typos docs/parity-matrix.md`: exit 0.
- `just fmt-check` not run: it is `cargo fmt`, no Rust file was touched, and it was not part of acceptance for a docs file.
- `git status` also shows `fixtures/gen/pyproject.toml` and `uv.lock` modified by another worker (T5). Not staged by me.

## Decisions made without asking

- Mapped competitor capabilities to existing plan tool ids first. Where no phase task covers a competitor capability, I added a proposed id marked `†` (14 rows, listed in "Gaps against the phase files") instead of silently dropping it.
- Split `pdf.sign_token` from `pdf.sign`, because AD-9 puts a tier on a tool spec; phase 9 already calls hardware-token signing Pro.
- Cloud eligibility (`cloud` runtime) marked on tools that need no hardware, local filesystem or secrets handle; `pdf.sign`, `web.to_pdf`, watch folders and Page-mode edit are local or wasm only. A guess, because the phase 13 server task is not specified per tool.
- Not adopted: PDF/X, MOBI/FB2/XPS, PSD, XML, PDF to SVG, Font to Outline, XFA, CBR, S3/FTP ingestion, extra Stirling AI agents. Reasons come from the two reports; PDF/X and PDF to SVG rest on the permissive stack report naming no engine (I grepped it, no mention).

## Open questions for the Mac PM

1. AD-9 defines `allowed(tier, plan)` with no size input, so the batch cap in rule (c) cannot be expressed by tier alone. Does the host enforce the cap as a plan limit, or does `allowed` take a usage argument?
2. Are the proposed cap numbers (25 files, 250 MB) acceptable as starting values?
3. Keychain software identities are Free under `pdf.sign`; only PKCS#11 and smart-card tokens are Pro. Is that the intended reading of "hardware tokens"?
4. Browser encrypt/decrypt: qpdf is out of process only, so web users get a hand-off where Bento encrypts in the browser. Accept, or plan a qpdf WASM build?
5. The 14 `†` gaps need phase owners to add tasks or move rows to Not adopted. Free-angle rotate, divide, posterize, adjust colours, background, scanner effect, TOC page and layers are small; `pdf.portfolio` and `pdf.timestamp` need a decision.
6. `pdf.overlay` is filed under group `organize`, following the phase 3 text, although the group list also has `overlay`.
