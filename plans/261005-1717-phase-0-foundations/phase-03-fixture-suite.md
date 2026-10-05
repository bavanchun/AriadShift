---
phase: 3
title: "Fixture suite (60 documents + manifest)"
status: completed
priority: P1
effort: "12h"
dependencies: [2]
---

# Phase 3: Fixture suite (60 documents + manifest)

## Goal

Commit a license-clean fixture suite of 60 Vietnamese and English documents (minimum 50). Every file is described in `fixtures/manifest.toml`, and the manifest is checked in CI. About 28 Markdown files feed the Phase 0 MD → DOCX goldens. The other formats seed roadmap phases 1a and 1b, including scans with exact ground truth for the Vietnamese OCR choice.

## Context links

- [OCR and fixtures research](./research/researcher-02-ocr-fixtures-report.md) §4 (sources, manifest, target mix)
- [AGENTS.md](../../AGENTS.md) Licensing (fixture rules from phase 1)
- [ARCHITECTURE.md](../../ARCHITECTURE.md) §5, §14

## Key insights

- **Excluded sources:**
  - CC-BY-SA: a converted golden would be an adaptation and would have to be BY-SA too. This rules out Wikipedia and the CommonMark/GFM spec text.
  - GPL test suites (Pandoc's).
  - Research-only datasets (OmniDocBench).
  - arXiv's default `nonexclusive-distrib` license.
- **Public-domain rule for external works:** the work was **published before 1931** and its author died before 1976, or it is a US federal work (17 U.S.C. §105), or it is a Vietnamese legal or administrative document (IP Law Art. 15) downloaded from a government host. A modern reprint or annotated edition fails the rule, because editors and typesetters may hold new rights. Use the original edition, or drop the item.
- **Generator output must be deterministic.**
  - python-docx stamps wall-clock times into zip entries.
  - typst-py stamps `/CreationDate` and XMP dates (both verified 2026-10-06).
  - The generator therefore rewrites DOCX zips with a fixed `ZipInfo.date_time` and compiles Typst with `#set document(date: none)` (or a fixed date).
  - Pillow cannot read PDFs. Scans are rendered by Typst directly to PNG (`typst.compile(format="png", ppi=N)`), and Pillow only degrades the pixels.
- **Generated prose** is original text written for this repository, or quoted from the public-domain sources above. Never paste text of unknown origin.
- **This phase lands before phase 4,** because phase 4's IR snapshots cover these files. Running the two in parallel turns CI red at an intermediate commit.

## Requirements

Functional:
- ≥ 50 documents (target 60) in the mix below.
- Every document has a manifest entry. Supporting files (images referenced by a Markdown file, scan ground truth) are listed as `companions` of their document, and do not count as documents.
- Every listed path exists, and its SHA-256 matches.
- One command regenerates every `origin = "generated"` file **byte-identically**.

Non-functional:
- Short kebab-case paths, no spaces (Windows MAX_PATH).
- Bytes are kept exact across OSes (`fixtures/** -text`).
- Each file is ≤ 2 MB; the whole suite is ≤ 25 MB.

## Architecture

```text
fixtures/
├── manifest.toml            one [[fixture]] per document; single source of license data
├── md/                      28 Markdown (+ md/assets/ for image companions)
├── html/ (6)  docx/ (8)  pdf/ (8)  image/ (2)
├── scan/                    8 (+ scan/truth/*.txt companions)
├── golden/                  written by phases 4 and 6 (insta snapshots); not in the manifest
└── gen/                     uv workspace member "ariad-fixture-gen" (dev-only, Apache-2.0)
    ├── pyproject.toml
    └── src/ariad_fixture_gen/{__main__.py, markdown.py, html.py, docx.py, pdf.py, scan.py, text/…}
```

Manifest entry fields:

| Group | Fields |
|---|---|
| Identity | `id`, `path`, `format`, `media_type`, `languages` (BCP-47), `title` |
| Provenance | `origin` (`generated` \| `external`), `source_url`, `retrieved`, `generated_by` |
| License | `license` (SPDX or `LicenseRef-*`), `license_basis`, `attribution` |
| Integrity | `sha256`, `pages` |
| Usage | `tags` (closed vocabulary), `phase` (`0` \| `1a` \| `1b`), `routes` (e.g. `["md->docx"]`), `companions` (`[{ path, sha256 }]`) |

The checker is written in Rust and shared with phase 6's golden test: `crates/ariad-cli/tests/support/{mod.rs,fixtures.rs}` plus `crates/ariad-cli/tests/fixture_manifest.rs`. No new crate is needed.

## Target mix (60)

| Group | Count | Composition | Phase |
|---|---|---|---|
| Markdown, generated | 24 | 12 vi, 10 en, 2 mixed. **One GFM feature per file:** headings, nested lists, ordered with start, task list, table with alignment, vi table with tone-dense headers, footnotes, inline and display math, fenced code, blockquote, links and autolinks, emphasis/strike, hard breaks, raw inline HTML, local image (companion in `md/assets/`), emoji shortcodes, YAML front matter (title/author/lang/date). **Edge cases:** an NFD twin of an NFC file, a CRLF twin of an LF file, an RTL snippet, a CJK snippet, a long file (> 200 headings), a 64-level nested list (at the limit), and 3 kitchen-sink files | **0** |
| Markdown from PD texts | 4 | 2 vi from Wikisource PD (*Truyện Kiều* from an edition published before 1931; a PD prose excerpt), 2 en from Gutenberg (header and trademark removed) | **0** |
| HTML | 6 | 4 generated (vi/en), 2 PD (Wikisource page, Federal Register notice) | 1a |
| DOCX | 8 | 4 generated with python-docx (styles, numbering, tables, footnotes; vi + en), 2 VN government DOC/DOCX, 2 Docling synthetic edge cases (MIT, maintainer-authored only, checked one by one) | 1a |
| Digital PDF | 8 | 3 VN legal (chinhphu.vn), 2 arXiv CC-BY-4.0 (e.g. 2203.01017), 1 GAO report (pages without third-party figures), 2 Typst-generated vi/en with known text | 1b |
| Scans | 8 | 2 vi Commons PD scans (*Tục ngữ, cổ ngữ, gia ngôn*, 1897; one more vi edition published before 1931), 1 vi government scan from `congbao.chinhphu.vn` (named item), 2 en (LoC Chronicling America, Internet Archive PD), 3 Typst-rendered vi/en pages degraded at 300/200/150 ppi with exact ground truth | 1b |
| Images | 2 | PNG of a vi page and an en receipt-style form, both generated | 1b |

## Files

| Action | File | Purpose | Size |
|---|---|---|---|
| Create | `fixtures/gen/pyproject.toml` | Deps: python-docx (MIT), Pillow (`MIT-CMU`), typst-py (license recorded from its GitHub repo; PyPI has no license metadata). Verify each version for Python 3.14 | ~20 |
| Create | `fixtures/gen/src/ariad_fixture_gen/*.py` | `uv run --package ariad-fixture-gen python -m ariad_fixture_gen [--only md]`. Writes files deterministically and updates `sha256` for generated entries and companions | ~450 |
| Create | `fixtures/gen/src/ariad_fixture_gen/text/*.md` | Original vi/en source text | ~300 |
| Create | `fixtures/{md,html,docx,pdf,scan,image}/…` | 60 documents + companions | ≤ 25 MB |
| Create | `fixtures/manifest.toml` | 60 entries | ~1,100 |
| Modify | `pyproject.toml`, `uv.lock` | Add the `fixtures/gen` workspace member | — |
| Modify | `crates/ariad-cli/Cargo.toml`, `Cargo.lock` | Dev-deps: serde, toml, sha2, hex | — |
| Create | `crates/ariad-cli/tests/support/{mod.rs,fixtures.rs}` | Manifest types, loader, `fixtures_for(route, phase)`; `#![allow(dead_code)]` because each test binary uses a subset | ~130 |
| Create | `crates/ariad-cli/tests/fixture_manifest.rs` | Manifest checks | ~130 |

## Implementation steps

1. **Generator package.**
   - Add `fixtures/gen` (`ariad-fixture-gen`, `requires-python >= 3.14`) to the uv workspace.
   - Before pinning, verify each dependency on PyPI/GitHub: version, cp314 support, and license.
   - Run `uv lock`.
2. **Fonts.**
   - First check the fonts Typst bundles (Libertinus Serif, New Computer Modern): render every letter in U+1EA0–U+1EF9 to PNG and inspect the result.
   - If coverage fails, vendor Noto Serif and Be Vietnam Pro (OFL-1.1) with their license texts under `fixtures/gen/fonts/`. These are binary files under `.gitattributes`.
3. **Source texts.** Write the original vi/en texts once. Every format is composed from them, so a scan's ground truth is its source text.
4. **Markdown generators (24).**
   - NFD twin: `unicodedata.normalize("NFD", …)`, and assert it contains combining marks.
   - CRLF twin: write with `newline="\r\n"`.
   - Front-matter file: title, author list, `lang: vi`, a date.
   - Emoji file: GitHub shortcodes such as `:smile:` and `:tada:`.
   - Image file: references `assets/<name>.png` as a companion.
   - Tag every entry `phase = "0"`, `routes = ["md->docx"]`.
5. **PD Markdown (4).**
   - For each Wikisource item, record in `license_basis` the source edition, its year, and the editor's or transcriber's death date. Reject an item when any of these is after the PD rule.
   - Gutenberg items: remove the PG header and trademark text.
6. **HTML, DOCX, PDF, scan and image generators.**
   - python-docx output is re-zipped with fixed `date_time` and sorted entries.
   - Typst documents set `date: none`. Scans are rendered with `typst.compile(format="png", ppi=…)`, then degraded with Pillow (blur, noise, JPEG, small rotation) using a fixed seed. Write `scan/truth/<id>.txt` as companions.
7. **External documents.**
   - Download each one and check the rights statement on its page.
   - For GAO, check every kept page for third-party figures and record "pages checked" in `license_basis`.
   - Crop scans to 1–3 pages.
   - Record `source_url`, `retrieved`, `license`, `license_basis`, `attribution` and `sha256`.
   - Replace any candidate whose rights cannot be confirmed. Generated documents fill the gap.
8. **Checker (`fixture_manifest.rs`).** Fail when any of these holds:
   - An id is duplicated or not kebab-case.
   - A document or companion path is missing.
   - A file under `fixtures/` (excluding `gen/`, `golden/` and `manifest.toml`) is neither a document nor a companion.
   - A SHA-256 does not match.
   - A license is outside the allow list: `Apache-2.0`, `MIT`, `CC-BY-4.0`, `CC0-1.0`, `CC-PDM-1.0`, `LicenseRef-VN-IPL-Art15`, `CDLA-Permissive-1.0`, `ODC-By-1.0`.
   - A tag is outside the vocabulary.
   - An `external` entry lacks `source_url`, `retrieved` or `license_basis`, or a `generated` entry lacks `generated_by`.
   - There are fewer than 50 documents, or fewer than 20 with `routes ∋ "md->docx"`.
   - The NFD, CRLF, front-matter, emoji or image Markdown cases are missing.
9. **Determinism check (local).** Run the generator twice and require `git status --porcelain fixtures/` to be empty after the second run.
10. Run `just ci` locally, then push. CI runs the checker on all three OSes. The Windows job proves that `-text` keeps the bytes identical.

## Todo

- [x] Generator package + lock (versions and licenses verified)
- [x] Vietnamese font coverage confirmed (or OFL fonts vendored as binary)
- [x] Original vi/en source texts
- [x] 24 generated Markdown incl. NFD/CRLF twins, front matter, emoji, image, 64-level nesting
- [x] 4 PD Markdown with edition-level license basis
- [x] 6 HTML, 8 DOCX, 8 PDF, 8 scans (+ truth), 2 images
- [x] Every external item's rights checked and recorded (GAO pages checked)
- [x] `fixtures/manifest.toml` complete (60 entries, companions listed)
- [x] Rust checker + support module + dev-deps
- [x] Generator byte-identical on a second run
- [x] CI green on 3 OSes

## Test scenario matrix

| Priority | Scenario | Expected |
|---|---|---|
| Critical | Checker on Windows | SHA-256 values match (no CRLF rewrite) |
| Critical | A file added without an entry or companion | Fails and names the file |
| Critical | Generator re-run | No byte changes |
| High | One byte edited | SHA-256 mismatch |
| High | `CC-BY-SA-4.0` license | Fails |
| Medium | Fewer than 50 documents | Fails |

## Success criteria

- 60 documents (≥ 50), each licensed and hashed, with companions tracked.
- The checker is green on three OSes.
- The generator reproduces its outputs byte-identically.

## Risk assessment

| Risk | Mitigation |
|---|---|
| An external item is not redistributable | The PD rule names editions; generated documents dominate; replace the item |
| A library update breaks determinism | Generator versions are pinned in `uv.lock`; step 9 catches it |
| Ground truth drifts from the scan | Truth comes from the same run as the scan |
| The suite grows large | 2 MB per file; scans cropped |

## Security considerations

- Never commit a user's or private document (AGENTS.md).
- Fixtures are hostile inputs by design. Later fuzzing seeds from them.
- Government PDFs carry official signatures only, with no personal data.

## Next steps

Phase 4 snapshots the IR of every `md->docx` fixture. Phase 6 writes the DOCX goldens.
