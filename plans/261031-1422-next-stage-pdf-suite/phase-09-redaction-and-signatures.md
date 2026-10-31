---
phase: 9
title: "True redaction with verification, and digital signatures (visual + PAdES certificate, timestamp, validate)"
status: pending
priority: P1
effort: "6 tasks, ~12 worker-days"
dependencies: [4, 5]
branch: feat/redact-sign
milestone: B (engine breadth)
risk_lane: high (data loss / security) — owner confirms scope at dispatch
---

# Phase 9: Redaction and signatures

> Deep-mode note: high-risk lane. The Mac PM confirms redaction scope with the owner (AskUserQuestion) before dispatch, and personally reviews every diff in this phase.

## Redaction (no permissive library solves it — stack report §2.7)

Design (normative):
1. Targets: text matches (exact, regex, presets: email, phone, Vietnamese CCCD/CMND, IBAN, credit card), user rectangles, whole pages.
2. Geometry from PDFium (`FPDFText_GetCharBox`, page objects).
3. Content removal in `ariad-pdf`: rewrite content streams (including Form XObjects and Type3 glyph procs) to drop text-showing operators for matched glyphs (split `TJ`/`Tj` arrays and re-position remaining glyphs), drop or clip vector paths in regions, **rewrite image XObjects** to black out pixels in regions (decode → paint → re-encode), remove annotations/links/form fields intersecting regions; then scrub matches everywhere else: Info + XMP, outlines, named destinations, link URIs, page labels, structure tree `/ActualText`, `/Alt`, `/E`, form field `/V` `/DV` `/TU`, page `/Thumb`, embedded-file names **and contents**, XFA (removed entirely when present).
4. Burn a black (configurable) box appearance into the content.
5. Full rewrite (no incremental save; previous revisions dropped) and garbage collection.
6. **Verification gates (all must pass or the tool fails, never "best effort")**: (a) re-extract text with PDFium and with `ariad-core`'s reader — no match; (b) render regions — fully covered; (c) no image XObject retains non-redacted pixels in regions; (d) **whole-object-graph scan**: walk every object reachable from the trailer (decoded streams included, all string encodings incl. UTF-16BE and hex) and fail on any target match; (e) result JSON `verification_report` listing every check. Tests additionally use an independent extractor (`pdftotext` from Poppler, test-only, never shipped) and `open-redact-pdf` as oracles.
7. Test oracle: `open-redact-pdf` 0.6.0 (MIT OR Apache-2.0) used **only in tests** to cross-check, never shipped as a dependency.

Tools: `pdf.redact { targets, color, overlay_text?, scope: {text, images, vectors, annotations, metadata, attachments} = all }`, `pdf.redact_verify` (standalone checker for any PDF + target list).

## Signatures

- Visual signatures (draw/type/image) placed as stamp annotations or flattened content: `pdf.sign_visual`.
- Certificate signing (PAdES B-B, B-T, B-LT, B-LTA) via **pyHanko 0.38.0 (MIT)** out of process in a `sign` pack (Python via uv with `uv.lock` hashes; full dependency tree listed and reviewed in the task report; licence gate = the Docling gate **generalised into a shared library** first (`engines/common/licenses/`), because `engines/docling/pack/licenses.py` hard-codes Docling distributions — note the gate detects licence drift, not malicious wheels): `pdf.sign { certificate: pkcs12 bytes (secret), password (secret), reason, location, contact, visible: {page, rect, appearance}, level: "B-B" \| "B-T" \| "B-LT" \| "B-LTA", tsa_url? }`. Incremental update always (keeps earlier signatures valid).
- PKCS#11 / hardware tokens and macOS Keychain identities: macOS app (phase 18) passes the signing operation through a Swift-side `SecKey` signer callback; desktop CLI supports PKCS#11 via pyHanko's extra only after its licence is verified (gate). Tier `pro` for hardware-token signing (proposed in parity matrix).
- `pdf.sign_validate` → JSON per signature: signer, time, integrity, coverage (whole document or partial), certificate chain status, timestamp, modifications since signing (pyHanko difference analysis). `pdf.sign_remove`.
- Network (AD-15): the `sign` engine never gets network. When a timestamp or revocation data is needed, the **host** fetches TSA/OCSP/CRL responses with the SSRF-safe client (https, public IPs only after DNS resolution, redirects re-checked, size/time caps) and passes them to pyHanko as files; in cloud this is disabled unless the operator configures allowed TSA URLs. URLs embedded in an attacker's certificate are never fetched automatically during `pdf.sign_validate` (offline validation by default; `--fetch-revocation` is explicit).

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Gemini 4 Argon high | Redaction core `crates/ariad-pdf/src/redact/**` (content stream rewriting, image pixel redaction, scrubbing) |
| T2 | Gemini 4 Argon high | PDFium geometry + verification gates `crates/ariad-pdfium-engine/src/redact/**`, `pdf.redact_verify` |
| T3 | Gemini 4 Argon high | Redaction test suite incl. adversarial cases (text in Form XObjects, Type3 fonts, text rendered as paths, ToUnicode tricks, overlapping images, hidden text `Tr 3`, text under images) |
| T4 | Gemini 4 Argon high | `sign` pack (pyHanko) `engines/sign/**` + licence gate + pack build |
| T5 | Gemini 4 Argon high | Signing tools + validation + visual signatures + secret handling (reuse phase-5 `x-ariad-secret`) |
| T6 | Gemini 4 Argon high | Signing tests: test CA and keys **generated at test time into a temp dir** (never committed; only a public test certificate may be committed if needed), validation in pyHanko and PDFium inspection, incremental edits after signing detected |
| T7 | Gemini 4 Argon high | Generalise the licence gate into `engines/common/licenses/` (Docling keeps its exceptions as data), used by `sign` now and `ai`/`inpaint` later |

## Acceptance

- [ ] Redaction adversarial suite: 100 % pass; any unsupported construct makes the tool fail with a clear error instead of producing an unverified output.
- [ ] Signed PDFs validate in pyHanko and show a valid signature in Acrobat Reader (manual gate check with the test CA trusted).
- [ ] No certificate, key or password reaches logs, temp files outside the job dir, or reports.
