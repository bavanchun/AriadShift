# Worker Report: Phase 1 — Task T5 (Permissive PDF Tools Test Corpus)

- **Worker**: `argon`
- **Task**: `T5` — Permissive PDF test corpus (`fixtures/pdf-tools/`)
- **Branch**: `docs/next-stage-contracts`
- **Worktree**: `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p01-contracts`

## Summary of Changes

1. **Dev/test dependency (`fixtures/gen/pyproject.toml`, `uv.lock`)**:
   - Added `pikepdf==10.17.0` under `[dependency-groups] dev` in `fixtures/gen/pyproject.toml` and marked the package with `classifiers = ["Private :: Do Not Upload"]` (`[tool.uv] package = false` was already set).
   - Locked `pikepdf==10.17.0` (MPL-2.0, dev/test-only, never distributed in product artifacts) via `uv lock`.

2. **Manifest tag allowlist (`crates/ariad-cli/tests/support/fixtures.rs`)**:
   - Extended `ALLOWED_TAGS` with 18 tags for the PDF tools corpus: `pdf-tools`, `multipage`, `outlines`, `page-labels`, `mixed-sizes`, `rotated`, `encrypted`, `annotations`, `attachments`, `incremental-update`, `broken-xref`, `object-streams`, `subset-font`, `watermark`, `blank-pages`, `javascript`, `tagged-pdf`, and `redaction`.

3. **Deterministic corpus generator (`fixtures/gen/src/ariad_fixture_gen/pdf_tools.py`)**:
   - Implemented deterministic builders for all 22 PDF tool test cases and their `*.truth.json` ground-truth companions using `static_id=True`, fixed timestamps (`D:20231114221320Z` / `1_700_000_000`), and normalized trailer `/ID` digests.
   - Provided test-time encryption helpers (`ENCRYPTED_FIXTURE_SPECS`, `generate_encrypted_fixture`, `materialize_encrypted_fixtures`, and CLI `--emit-encrypted <dir>`) that read the committed `.plaintext.pdf` files and emit AES-256 (R=6 user+owner), AES-256 (R=6 owner-only with print/extract disabled), and RC4-40 (R=2 legacy) encrypted PDFs on demand. Passwords live only in `ENCRYPTED_FIXTURE_SPECS`, never in `*.truth.json`.
   - Provided `generate_large_1000_pages()` both as a committed fixture builder (180,704 bytes with PDF 1.5 object streams, well below the 2 MiB manifest limit and 5 MB task limit) and as a test-time helper.

4. **Generated corpus (`fixtures/pdf-tools/**`) & manifest (`fixtures/manifest.toml`)**:
   - Materialized 44 files under `fixtures/pdf-tools/` (19 PDFs, 3 `.plaintext.pdf` sources for encrypted cases, and 22 `.truth.json` companions; total 1,142,987 bytes / 1.09 MiB).
   - Appended 22 `[[fixture]]` entries to `fixtures/manifest.toml` (`license = "Apache-2.0"`, `origin = "generated"`, `phase = "1"`, `routes = []`) with SHA-256 digests for every file and companion.

## Corpus Inventory (22 Cases)

| # | ID | Committed Path | Pages | Size (B) | Structural Coverage |
|---|---|---|---:|---:|---|
| 1 | `multipage-20` | `fixtures/pdf-tools/multipage-20.pdf` | 20 | 9,433 | 20 pages, 2-level outline tree (4 chapters, 2 nested sections), page labels (`i..ii`, `1..16`, `A-A..A-B`) |
| 2 | `mixed-sizes` | `fixtures/pdf-tools/mixed-sizes.pdf` | 3 | 1,818 | A4 portrait, US Letter portrait, A3 landscape |
| 3 | `rotated-pages` | `fixtures/pdf-tools/rotated-pages.pdf` | 4 | 2,160 | `/Rotate` 0°, 90°, 180°, 270° (page 2 inherits `/Rotate 90` from `/Pages` parent) |
| 4 | `encrypted-aes256` | `fixtures/pdf-tools/encrypted-aes256.plaintext.pdf` | 1 | 1,142 | Plaintext source committed; `generate_encrypted_fixture("encrypted-aes256")` emits AES-256 (R=6, V=5) with user + owner passwords |
| 5 | `encrypted-owner-only` | `fixtures/pdf-tools/encrypted-owner-only.plaintext.pdf` | 1 | 1,143 | Plaintext source committed; `generate_encrypted_fixture("encrypted-owner-only")` emits AES-256 (R=6, V=5) with empty user password, owner password, print/extract disabled |
| 6 | `encrypted-rc4-40` | `fixtures/pdf-tools/encrypted-rc4-40.plaintext.pdf` | 1 | 1,125 | Plaintext source committed; `generate_encrypted_fixture("encrypted-rc4-40")` emits legacy 40-bit RC4 (R=2, V=1) |
| 7 | `form-acroform` | `fixtures/pdf-tools/form-acroform.pdf` | 1 | 3,598 | AcroForm with text (`full_name`), checkbox (`subscribe_news`), radio group (`delivery_mode`), combo (`department`), and unsigned signature widget (`approver_signature`) |
| 8 | `annotations` | `fixtures/pdf-tools/annotations.pdf` | 1 | 2,065 | `/Highlight` (`QuadPoints`), `/Text` sticky note, `/Ink` stroke (`InkList`), `/Link` URI action |
| 9 | `with-attachments` | `fixtures/pdf-tools/with-attachments.pdf` | 1 | 2,110 | `/Names/EmbeddedFiles` + `/AF` with `inventory.csv` (`text/csv`) and `notes.txt` (`text/plain`) |
| 10 | `incremental-update` | `fixtures/pdf-tools/incremental-update.pdf` | 1 | 1,577 | Two-revision PDF with `/Prev` xref pointer and two `%%EOF` markers |
| 11 | `broken-xref` | `fixtures/pdf-tools/broken-xref.pdf` | 1 | 1,161 | Intentionally shifted xref byte offsets (`+47` bytes) and `startxref` pointer that `qpdf` / `pikepdf` repairs |
| 12 | `object-streams` | `fixtures/pdf-tools/object-streams.pdf` | 2 | 1,200 | PDF 1.5 `/ObjStm` compressed object streams and `/XRef` cross-reference stream |
| 13 | `vi-diacritics-subset-font` | `fixtures/pdf-tools/vi-diacritics-subset-font.pdf` | 1 | 20,862 | Vietnamese text with full tone/vowel diacritics rendered with a subset TrueType `/CIDFontType2` font (`DejaVuSansMono`) |
| 14 | `watermark-layer` | `fixtures/pdf-tools/watermark-layer.pdf` | 3 | 2,494 | Optional Content Group (`/OCG` named `AriadShift Watermark`) wrapped in `/OC /WM0 BDC ... EMC` |
| 15 | `watermark-artifact` | `fixtures/pdf-tools/watermark-artifact.pdf` | 3 | 2,305 | `/Artifact <</Subtype /Watermark /Type /Pagination>> BDC ... EMC` marked-content watermark |
| 16 | `watermark-repeated-xobject` | `fixtures/pdf-tools/watermark-repeated-xobject.pdf` | 3 | 2,358 | Shared `/Form` XObject (`/FmWM`) invoked across all 3 pages with no `/OCG` or `/Artifact` tag |
| 17 | `scanned-skewed-vi` | `fixtures/pdf-tools/scanned-skewed-vi.pdf` | 1 | 292,305 | Image-only Vietnamese page rasterized and skewed 3.0° with deterministic scan noise and zero text operators |
| 18 | `blank-separators` | `fixtures/pdf-tools/blank-separators.pdf` | 6 | 2,692 | 6-page document where pages 2 and 5 are blank separator sheets (page 5 carries faint scanner margin specks) |
| 19 | `javascript-action` | `fixtures/pdf-tools/javascript-action.pdf` | 1 | 1,416 | Catalog `/OpenAction`, `/AA`, and `/Names/JavaScript` actions for sanitization testing |
| 20 | `tagged-actualtext` | `fixtures/pdf-tools/tagged-actualtext.pdf` | 1 | 1,656 | `/StructTreeRoot` element carrying synthetic PII in `/ActualText`, `/Alt`, `/E`, and page content stream |
| 21 | `form-with-values` | `fixtures/pdf-tools/form-with-values.pdf` | 1 | 3,528 | AcroForm widgets carrying synthetic PII in `/V`, `/DV`, `/TU`, `/AP/N` appearance streams, and page content stream |
| 22 | `large-1000-pages` | `fixtures/pdf-tools/large-1000-pages.pdf` | 1000 | 180,704 | 1,000-page PDF 1.5 document with object streams (~176.5 KiB) |

## Verification

- **Determinism**: Ran `uv run --frozen --package ariad-fixture-gen python -m ariad_fixture_gen.pdf_tools` twice and confirmed zero byte or manifest drift across all 44 generated files.
- **Test-time encryption & secret hygiene**: Verified `materialize_encrypted_fixtures` opens cleanly in `pikepdf` with the expected user/owner passwords and `R=6` / `R=2` parameters, and verified no password strings appear in any `*.truth.json` file.
- **Manifest & tag validation**: `cargo test -p ariad-cli --test fixture_manifest --test fuzz_seed_manifest` passed (validating all file paths, SHA-256 digests, companion digests, `ALLOWED_TAGS`, per-file 2 MiB cap, and 25 MiB suite cap).
- **Formatting & spelling**: `just spell` and `rustfmt --edition 2024 --check crates/ariad-cli/tests/support/fixtures.rs` passed.

## Coordinator / Integration Notes

1. **`fixtures/gen/src/ariad_fixture_gen/__main__.py` (outside T5 `FILES YOU OWN`)**:
   - To have `just fixtures` (`uv run --package ariad-fixture-gen python -m ariad_fixture_gen`) run `pdf_tools.generate` automatically alongside `core`, `pdf_edge`, `scan`, and `heavy`, add `"pdf-tools": pdf_tools.generate` to `GENERATORS` in `fixtures/gen/src/ariad_fixture_gen/__main__.py`. Currently `fixtures/gen/src/ariad_fixture_gen/pdf_tools.py` is invocable directly via `uv run --frozen --package ariad-fixture-gen python -m ariad_fixture_gen.pdf_tools` and leaves existing `just fixtures` outputs untouched.
2. **`bench/tests/test_references.py` (outside T5 `FILES YOU OWN`)**:
   - `test_unscored_manifest_fixtures_are_explicitly_route_free` in `bench/tests/test_references.py` has a hardcoded `assert len(unscored) == 25` for fixtures with `routes = []`. Adding the 22 route-free `pdf-tools` fixtures increases that count from 25 to 47; update that assertion (or filter out `"pdf-tools"` tagged fixtures) when integrating Phase 1.
