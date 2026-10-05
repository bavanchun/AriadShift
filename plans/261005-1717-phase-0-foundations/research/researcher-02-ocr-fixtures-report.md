# Research: Vietnamese + English OCR stack and fixture sources

Date: 2026-10-06 · Scope: planning only · Inputs: ARCHITECTURE.md §2.4, §6.4, §7.1, §14, §15, §20; AGENTS.md licensing rules.

## 1. Recommendations

The current architecture would ship an OCR path that cannot write Vietnamese. RapidOCR is the only OCR in the `docling` pack, and both RapidOCR and Docling route `vi` to PP-OCRv6. The recognition dictionaries for PP-OCRv5 and PP-OCRv6 contain only 2 of the 90 letters in Unicode block U+1EA0–U+1EF9 (ạ ả ấ ầ ẩ ẫ ậ … ự ỳ ỵ ỷ ỹ), and they contain no combining marks. The model therefore has no output class for most tone-marked letters and drops them silently. A local smoke test confirmed this ("Cộng hòa Xã hội" came back as "Cng hòa Xã hi", CER 0.13 on clean text). Section 3 has the details.

Ranked choice for the vi+en OCR path:

| Rank | Role | Choice | Why |
|---|---|---|---|
| 1 | **Default OCR for any job whose `languages` contains `vi`** | **Tesseract 5.5.3 + `tessdata_best` `vie`+`eng`**, called through Docling's `TesseractCliOcrOptions` (out of process) | Apache-2.0 for both code and data, and CPU-only. It covers the full Vietnamese alphabet and handles mixed vi+en in one pass. It scored CER 0.000 on clean renders in the local test. It is weak on degraded scans: confusions between stacked marks such as Cộng→Cống and Chủ→Chú pushed CER to 0.20–0.37 at 9–12 px x-height |
| 2 | **Default OCR for non-Vietnamese jobs (en, zh, ja, other Latin)** | **RapidOCR 3.9.2 + PP-OCRv6 (small) on ONNX Runtime 1.30.0**, as today | Apache-2.0. It scored CER 0.000 on English and is faster and stronger on noisy Latin and CJK text. Keep it, but never route `vi` to it |
| 3 | **Optional high-accuracy VLM path** (opt-in, per page or whole document, CPU-capable) | **PaddleOCR-VL-1.6 (0.96B, Apache-2.0)** running through **llama.cpp (MIT)** from the official GGUF (about 1.8 GB). Benchmark **SenOCR-Vi** (an Apache-2.0 Vietnamese fine-tune of the same model) as a drop-in | It has the best Vietnamese score among permissive models that are CPU-feasible and vendor-maintained (MDPBench VI 80.9). Docling 2.133 already ships an adapter that converts PaddleOCR-VL 1.6 JSON into `DoclingDocument`. PaddleOCR documents x64 CPU as a supported target |
| Watch | VLM alternative | MonkeyOCRv2-B-Parsing (0.88B, declared Apache-2.0, VI 83.2, CPU support since 2026-08) | Its Vietnamese score is higher, but the repo is young, the LICENSE file is only a one-paragraph statement (GitHub reports NOASSERTION), and it needs a torch stack. Re-evaluate it in `bench/` before adopting |

Route rule for the engine to implement in 1b: `options.languages ∋ "vi"` → Tesseract `vie+eng`. Otherwise use RapidOCR with the first language. Use the VLM path only when `options.ocr = "accurate"`, or when mean Tesseract word confidence on a page falls below a threshold that `bench/` will calibrate.

### Changes ARCHITECTURE.md should take (in the same change as the Decision Log row)

- **§2.4**
  - RapidOCR row: change the version to `3.9.2 + 1.30.0` and the role to "OCR for PP-OCRv6 languages (en, zh, ja, most Latin). Not Vietnamese: PP-OCRv5/v6 dictionaries lack 88–90 of 134 Vietnamese letter forms."
  - Tesseract row: change to `5.5.3` + `tessdata_best` (`vie`, `eng`) with the role "Default OCR for Vietnamese and mixed vi+en."
  - Add a row: "PaddleOCR-VL | 1.6 (0.96B) | Apache-2.0 | Optional VLM OCR (via llama.cpp, MIT)."
  - Granite-Docling row: add "English (ja/ar/zh experimental); not used for Vietnamese."
- **§6.4** PDF (scan) row: change to "Docling + Tesseract (vi) / RapidOCR (other languages); optional PaddleOCR-VL."
- **§7.1** There are two options:
  - (a) Move Tesseract plus `vie`/`eng`/`osd` `tessdata_best` (about 30 MB) into the `docling` pack, because Vietnamese is a priority language. Keep other Tesseract languages in `ocr-extra`.
  - (b) Keep the packs as they are and have `ariad engines install docling` also pull `ocr-extra` when the locale is `vi`. Option (a) is simpler.
  - Either way, add an opt-in `ocr-vlm` pack containing the llama.cpp binary and the PaddleOCR-VL-1.6 GGUF, or fold these into `ocr-extra`.
- **§15** Add rows for Tesseract + tessdata (Apache-2.0, process), llama.cpp (MIT, process) and PaddleOCR-VL weights (Apache-2.0). Extend the Excluded bullet with model weights under OpenRAIL-M or custom terms, or with no license (Section 2 lists them).
- **§18** OCR row: "Chosen: Tesseract (vi), RapidOCR (others), PaddleOCR-VL opt-in. Rejected: EasyOCR, Surya/Chandra (OpenRAIL-M weights). Rationale: PP-OCR dictionaries cannot emit Vietnamese tone letters."
- **§20** Q3: mark it resolved as vi + en.

## 2. OCR evidence

Versions were verified on 2026-10-06 against PyPI, GitHub and the Hugging Face API. "VI" is the MDPBench Vietnamese column, a document-parsing composite score (higher is better), taken from the [official leaderboard](https://github.com/Yuliang-Liu/MultimodalOCR/blob/main/MDPBench/README.md). It is not raw CER.

### 2.1 Classic OCR engines

| Name | Version | Code license | Weights license | Vietnamese quality evidence | CPU | Verdict |
|---|---|---|---|---|---|---|
| [Tesseract](https://github.com/tesseract-ocr/tesseract) + [tessdata_best `vie`](https://github.com/tesseract-ocr/tessdata_best) | 5.5.3 (2026-07-24); `vie` model unchanged since 2017 | Apache-2.0 | Apache-2.0 | Local test: CER 0.000 on clean Noto Serif/Sans renders, 0.20–0.23 at about 12 px x-height with noise, 0.34–0.37 at about 9 px. Known confusion of acute and hook-above marks on top of circumflex ([langdata#66](https://github.com/tesseract-ocr/langdata/issues/66)) | Yes (fast) | **Default for vi** |
| [RapidOCR](https://github.com/RapidAI/RapidOCR) + PP-OCRv6 | 3.9.2 (2026-07-21) | Apache-2.0 | Apache-2.0 ([HF](https://huggingface.co/PaddlePaddle/PP-OCRv6_medium_rec)) | `ppocrv6_dict.txt` lacks 88 Vietnamese letters, and the HF `inference.yml` confirms the same gap. Local test: CER 0.132 on clean vi text with tone letters deleted; CER 0.000 on English. RapidOCR lists `vi` in `PP_OCRV6_LANGS` anyway | Yes | **Default for non-vi only** |
| PP-OCRv5 `latin_PP-OCRv5_mobile_rec` | via PaddleOCR 3.7.0 / RapidOCR | Apache-2.0 | Apache-2.0 | Dictionary lacks 90 Vietnamese letters (HF copy: 836 classes). On real exam papers, [0 of 1,629 ground-truth letters in U+1EA0–1EF9 were emitted](https://huggingface.co/Vphuc/PP-OCRv5-mobile-rec-vi). Local CER 0.19–0.50 | Yes | Excluded for vi |
| [PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR) 3.x pipeline | 3.7.0 (2026-06-11) | Apache-2.0 | Apache-2.0 | Same dictionaries as above. `lang="vi"` now routes to PP-OCRv6_medium. PP-StructureV3 scores VI 52.7 | Yes, but PaddlePaddle 3.3.1 has **no cp314 wheel** | Not needed; RapidOCR already covers PP-OCR |
| [VietOCR](https://github.com/pbcquoc/vietocr) | 0.3.13 on PyPI (2024-03); last GitHub release 2021; last push 2025-01 | Apache-2.0 on GitHub, but PyPI classifier says MIT (inconsistent) | Not stated; weights hosted on `vocr.vn` | Full Vietnamese vocabulary. README reports full-sequence precision 0.88. Recognition only, so it needs a detector | Yes (torch; community ONNX ports are days old and unvetted) | Fallback candidate only if bench shows a gap; low maintenance |
| [EasyOCR](https://github.com/JaidedAI/EasyOCR) | 1.7.2 (2024-09) | Apache-2.0 | Apache-2.0 | `latin_g2` charset covers all Vietnamese letters. No published vi CER | Slow; PyTorch | Keep rejected (§18); stale |
| [OnnxTR](https://github.com/felixdittrich92/OnnxTR) / docTR | 0.9.0 (2026-08) | Apache-2.0 | Apache-2.0 | It has a `vietnamese` vocab, but the shipped `parseq-multilingual-v1` vocab lacks 102 Vietnamese letters | Yes | Excluded (no vi model) |
| [nemotron-ocr-v2](https://huggingface.co/nvidia/nemotron-ocr-v2) (Docling engine) | — | — | NVIDIA Open Model License (non-OSI) | NVIDIA validates no Vietnamese | GPU-oriented | Excluded (license) |
| ocrmac | — | MIT | Apple Vision (OS) | macOS only | — | Not a cross-platform default |

### 2.2 VLM and document-parsing models

| Name | Size | Code license | Weights license | VI (MDPBench) | CPU-feasible | Verdict |
|---|---|---|---|---|---|---|
| [PaddleOCR-VL-1.6](https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6) | 0.96B; [official GGUF](https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6-GGUF) 0.94 + 0.88 GB | Apache-2.0 | Apache-2.0 | 80.9 (109 languages incl. vi) | Yes: llama.cpp ≥ b8110; x64 CPU documented | **Recommended VLM** |
| [SenOCR-Vi](https://huggingface.co/VietAlphaLabs/SenOCR-Vi) | 0.96B (LoRA-merged PaddleOCR-VL-1.6) | — | Apache-2.0 | Self-reported 82.83 on the corrected 160-page vi slice (not on the leaderboard) | Same architecture as above; no official GGUF | Benchmark as a drop-in; small org |
| [MonkeyOCRv2-B-Parsing](https://huggingface.co/zenosai/MonkeyOCRv2-B-Parsing) | 0.88B | Declared Apache-2.0, non-standard LICENSE file | Apache-2.0 | 83.2 | Yes (official CPU guide, torch); no PyMuPDF (uses pypdfium2) | Watch |
| [olmOCR-2](https://huggingface.co/allenai/olmOCR-2-7B-1025) | 8.3B | Apache-2.0 | Apache-2.0 | 84.0 | No (GPU) | GPU-only option later |
| [dots.mocr](https://huggingface.co/dots-studio/dots.mocr) / dots.ocr | 3.0B | MIT | MIT | 79.9 / 79.1 | Marginal | Lower VI than PaddleOCR-VL at 3× the size |
| [LightOnOCR-2-1B](https://huggingface.co/lightonai/LightOnOCR-2-1B) | 1.0B | Apache-2.0 | Apache-2.0 | 74.9 | Yes | Weaker on vi |
| [GLM-OCR](https://huggingface.co/zai-org/GLM-OCR) | 1.3B | MIT | MIT | 69.2 | Yes | Weaker on vi |
| [Falcon-OCR](https://huggingface.co/tiiuae/Falcon-OCR) | 0.27B | Apache-2.0 | Apache-2.0 | 67.8 | Yes | Weaker on vi |
| [DeepSeek-OCR](https://huggingface.co/deepseek-ai/DeepSeek-OCR) / -2 | 3.3B | MIT | MIT / Apache-2.0 | 54.1 (v1) | Marginal | Weak on vi |
| [Granite-Docling-258M](https://huggingface.co/ibm-granite/granite-docling-258M) | 0.26B | Apache-2.0 | Apache-2.0 | Not evaluated. Card lists English, with ja/ar/zh experimental | Yes | Keep for English complex layouts only |
| [Qwen3-VL-2B/4B/8B](https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct) | 2–8.8B | Apache-2.0 | Apache-2.0 | 73.4 (8B) | 2B yes | General VLM; worse than PaddleOCR-VL on vi |
| [Chandra 2](https://huggingface.co/datalab-to/chandra-ocr-2) / [Surya](https://github.com/datalab-to/surya) | 5.3B / — | Apache-2.0 (code) | **Modified OpenRAIL-M** (revenue caps; "cannot be used competitively") | 85.6 | — | **Excluded** |
| [HunyuanOCR-1.5](https://huggingface.co/tencent/HunyuanOCR) | 1.1B | — | Tencent custom (`other`) | 82.4 | — | **Excluded** |
| [Nanonets-OCR2-3B](https://huggingface.co/nanonets/Nanonets-OCR2-3B) | 3.75B | — | **None declared**; base Qwen2.5-VL-3B is Qwen Research License (non-commercial) | 66.0 | — | **Excluded** |
| [Nemotron-Parse-2.0](https://huggingface.co/nvidia/NVIDIA-Nemotron-Parse-2.0) | 0.9B | — | OpenMDW-1.1 (not OSI) | n/a | — | Excluded pending legal review |
| MinerU 2.5 | 1.2B | AGPL / custom | — | 74.2 | — | **Excluded** (AGENTS.md) |

### 2.3 How this fits Docling and the engine packs

- Docling 2.133.0 (2026-10-03) offers OCR kinds `auto`, `rapidocr`, `easyocr`, `tesseract` (CLI), `tesserocr`, `ocrmac`, `nemotron-ocr` and `kserve_v2_ocr`. `auto` probes engines in order and skips any engine that cannot serve the requested language. It does not know that PP-OCRv6 "supports" `vi` only nominally, so **the AriadShift engine must pick the engine explicitly rather than use `auto`**.
- `RapidOcrOptions` accepts `rec_model_path` and `rec_keys_path`. A future PP-OCR recognizer fine-tuned for Vietnamese could replace Tesseract without any change to Docling. The [Vphuc fine-tune](https://huggingface.co/Vphuc/PP-OCRv5-mobile-rec-vi) widened the head from 838 to 908 classes, but it is gated and has 2 downloads, so it is not usable now.
- There is no Docling stage that runs a VLM as a per-region OCR. The VLM path should run as its own engine using one of PaddleOCR's documented CPU combinations ("PaddlePaddle + llama.cpp" or "Transformers + llama.cpp"). It would then convert the result with Docling's `docling/utils/paddleocr_vl_utils.py` (targets PaddleOCR-VL 1.6 / PaddleX 3.7.2) into `DoclingDocument`, then into IR.
- Two constraints follow:
  - PaddlePaddle has no Python 3.14 wheel. The VLM engine therefore needs either the Transformers layout backend (torch 2.14.1 ships cp314) or its own uv-managed Python 3.13 environment.
  - `llama-server` listens on TCP. The engine protocol forbids network connections, so the protocol needs an explicit loopback-to-own-child exception, or the engine must use an in-process binding.

## 3. Local verification (reproducible)

Run in the scratchpad: Arch Linux, 12 cores, rapidocr 3.9.2, onnxruntime 1.30.0, Tesseract 5.5.3 with `tessdata_best` from GitHub. The input was 6 Vietnamese lines (legal header, diacritic-dense sentences) and 3 English lines, rendered at 40 px in Noto Serif and Noto Sans. CER was computed after NFC normalization.

| Engine | vi clean | en clean | vi degraded (12 px / 9 px / 7 px x-height) |
|---|---|---|---|
| RapidOCR PP-OCRv6 small / medium (`vi`) | 0.132 (tone letters deleted) | 0.000 | not run (structurally unable) |
| RapidOCR PP-OCRv5 latin mobile | 0.188–0.500 | 0.029–0.184 | — |
| Tesseract `vie+eng` (`--psm 6`) | 0.000 | 0.000 | 0.20–0.23 / 0.34–0.37 / 0.74–0.86 |

This is a smoke test, not a benchmark: the corpus is tiny and the noise is synthetic (blur, salt-and-pepper, JPEG, rotation). It proves the dictionary defect and shows that Tesseract degrades sharply on poor scans, which is why the VLM path matters. It did not run VietOCR, EasyOCR or any VLM, because torch and llama.cpp are not installed locally.

## 4. Fixture sources

### 4.1 Licensing rules for the fixtures folder

- **Vietnamese legal documents are not copyrightable.** Under IP Law 50/2005/QH11 Art. 15 (as amended), legal normative documents, administrative documents, judicial documents and their **official** translations are excluded from protection ([WIPO Lex](https://www.wipo.int/wipolex/en/legislation/details/12011)). Unofficial English translations, for example from commercial legal portals, are copyrighted and must not be used. Commercial portals (thuvienphapluat, luatvietnam) also add formatting and annotations, so download only from government hosts.
- **Vietnamese copyright term** is generally the author's life + 50 years; anonymous and photographic works get 75 years from publication (Art. 27, [Commons summary](https://commons.wikimedia.org/wiki/Commons:Copyright_rules_by_territory/Vietnam)). For worldwide safety, require that the author died before 1976 (public domain in VN) **and** that the work was published before 1931 (public domain in the US as of 2026).
- **Avoid CC BY-SA, at least at first.** Mixing it into an Apache-2.0 repo is legal as aggregation. The catch is the golden outputs: a DOCX converted from a BY-SA Wikipedia article is an adaptation and must itself be BY-SA. That complicates `fixtures/` and any `bench/` artifact published on the website. Exclude BY-SA from Phase 0. If it is needed later, isolate it under `fixtures/cc-by-sa/` with per-file licenses that cover the goldens too. This also excludes the CommonMark and GFM spec text (CC-BY-SA-4.0) and Wikipedia prose. Vietnamese Wikisource is fine **only** for pages tagged public domain, because the transcription adds no new copyright.
- **Do not copy GPL test suites.** Pandoc's tests fall under the repo-wide GPL-2.0-or-later ([COPYRIGHT](https://github.com/jgm/pandoc/blob/main/COPYRIGHT)), so do not commit them. If wanted, fetch them at test time instead.
- **arXiv's default license is not redistributable.** It is `nonexclusive-distrib/1.0`. Filter for CC-BY-4.0 and CC0-1.0 using the license field in arXiv's metadata. For example, Docling's own test PDF 2206.01062 is nonexclusive and must not be copied, whereas 2203.01017 is CC-BY-4.0.

### 4.2 Source table

| # | Source | Language | Formats | License basis (SPDX in manifest) | Use for | Notes |
|---|---|---|---|---|---|---|
| 1 | **Self-generated** (script under `fixtures/gen/`, Apache-2.0) | vi, en, mixed | MD, HTML, DOCX (python-docx 1.2.0 MIT, or Pandoc output), PDF (Typst), PNG scans (Pillow degradation) | `Apache-2.0` (own work) | Bulk of Phase 0 | Fonts: Noto / Be Vietnam Pro (OFL-1.1). Hand-write the text, or take it from rows 2–4. Avoid LLM-generated prose of unclear provenance |
| 2 | [vanban.chinhphu.vn](https://vanban.chinhphu.vn) → `datafiles.chinhphu.vn/cpp/files/vbpq/...pdf`; [congbao.chinhphu.vn](https://congbao.chinhphu.vn) (Official Gazette) | vi | Digital PDF (signed, embedded Times New Roman), some DOC/DOCX; older items are scans | `LicenseRef-VN-IPL-Art15` | Real vi PDFs with tables, numbered Chương/Điều/Khoản, seals | Verified today: e.g. `1922_qd-ttg_04102026` is a 2-page digital PDF with a text layer. `vbpl.vn` returned 403 to automated fetches |
| 3 | [Vietnamese Wikisource](https://vi.wikisource.org) public-domain pages, e.g. *Truyện Kiều* (Nguyễn Du †1820) | vi | HTML → MD | `CC-PDM-1.0` + basis note | Diacritic-dense prose and verse | Check the PD template on each page; skip BY-SA editorial content |
| 4 | [Wikimedia Commons](https://commons.wikimedia.org) PD Vietnamese scans: [*Việt Nam sử lược*](https://commons.wikimedia.org/wiki/File:Viet_Nam_Su_Luoc.djvu) (Trần Trọng Kim †1953; 1919 text, 1974 reprint, 408 pp), [*Tục ngữ, cổ ngữ, gia ngôn*](https://commons.wikimedia.org/wiki/File:T%E1%BB%A5c_ng%E1%BB%AF,_c%E1%BB%95_ng%E1%BB%AF,_gia_ng%C3%B4n.djvu) (Huỳnh Tịnh Của, 1897), [*Ư tình lục*](https://commons.wikimedia.org/wiki/File:UTinhLuc.djvu) (Hồ Biểu Chánh †1958) | vi | DjVu → extract 1–3 page PNG/PDF | `CC-PDM-1.0` (Commons-tagged PD) | Real vi scans for OCR in 1b | The 1897 typography is a hard OCR case; the 1974 reprint is modern print |
| 5 | arXiv CC-BY / CC0 papers ([license help](https://info.arxiv.org/help/license/index.html)), e.g. [2203.01017](https://arxiv.org/abs/2203.01017) (CC-BY-4.0) | en | PDF + LaTeX source | `CC-BY-4.0` / `CC0-1.0` | Math, tables, two-column layout | Record authors as the attribution |
| 6 | US federal works (17 U.S.C. §105): GAO reports, Federal Register via govinfo.gov, IRS forms | en | Digital PDF, HTML, XML | `CC-PDM-1.0` + basis "17 USC 105" | Tables, forms, footnotes | Avoid pages with third-party copyrighted figures. NIST may reserve rights abroad, so prefer GAO and the Federal Register |
| 7 | [Project Gutenberg](https://www.gutenberg.org/policy/license.html) (authors †<1956) | en | HTML, EPUB, TXT | `CC-PDM-1.0`; strip the PG header and trademark | Long text, chapters, footnotes | PG says the stripped text is unrestricted in the US and "most of the world" |
| 8 | Library of Congress [Chronicling America](https://chroniclingamerica.loc.gov) (pre-1931 newspapers), Internet Archive PD books | en | Scanned JP2/PDF + OCR text | `CC-PDM-1.0` | Real English scans, multi-column | Check each item's rights statement |
| 9 | PMC Open Access subset (per-article license in the file list) | en | JATS XML + PDF | `CC-BY-4.0` / `CC0-1.0` only | Scholarly tables, references | Skip NC/ND items |
| 10 | [Upstage DP-Bench](https://huggingface.co/datasets/upstage/dp-bench) | en | Page images + reference HTML | `MIT` (sources: LoC, OER, Upstage) | Layout/table goldens in 1b | 200 pages; spot-check OER items |
| 11 | [DocLayNet](https://github.com/DS4SD/DocLayNet) | en (mostly) | PNG pages + COCO JSON | `CDLA-Permissive-1.0` | Layout eval subset in `bench/` (download, do not commit) | Too large for fixtures |
| 12 | [olmOCR-bench](https://huggingface.co/datasets/allenai/olmOCR-bench) | en | PDF pages + unit tests | `ODC-By-1.0` | `bench/` only | Attribution required |
| 13 | Docling `tests/data` (MIT repo) | en | DOCX edge cases (OMML, SDT, lists) | `MIT` **only** for files authored by Docling maintainers | DOCX reader edge cases | There is no per-file provenance, and it includes `elsevier-00.pdf` and arXiv nonexclusive PDFs. Copy only synthetic DOCX after checking each one |
| ✗ | OmniDocBench ("research purposes only, not commercial"), FUNSD, UIT Vietnamese datasets, `5CD-AI/Viet-Handwriting-OCR-v2` (gated), Wikipedia (BY-SA), Pandoc tests (GPL), CommonMark spec (BY-SA) | — | — | Excluded | — | — |

### 4.3 Manifest schema

Use one human-edited file, `fixtures/manifest.toml`, validated in CI against `schemas/fixture-manifest.v1.json` so it sits alongside the engine-protocol schema. CI also fails on any file in `fixtures/` that has no entry, and on any SHA-256 mismatch.

```toml
[[fixture]]
id = "vi-legal-qd-1922-2026"             # stable, kebab-case, unique
path = "fixtures/pdf/vi-legal-qd-1922-2026.pdf"
format = "pdf"                           # ariad format id (matches §6.4)
media_type = "application/pdf"
languages = ["vi"]                       # BCP-47
title = "Quyết định 1922/QĐ-TTg"
origin = "external"                      # external | generated
source_url = "https://datafiles.chinhphu.vn/cpp/files/vbpq/2026/10/1922_qd-ttg_04102026_1-signed.signed.pdf"
retrieved = 2026-10-06
license = "LicenseRef-VN-IPL-Art15"      # SPDX expression
license_basis = "VN IP Law 50/2005/QH11 Art. 15: legal normative documents are not copyright-protected"
attribution = "Thủ tướng Chính phủ"
generated_by = ""                        # script path + args when origin = generated
sha256 = "…"
pages = 2
tags = ["digital-pdf", "tables", "numbered-articles", "seal", "signed"]
phase = "1b"                             # earliest roadmap phase that consumes it
routes = ["pdf->md", "pdf->docx"]
golden = ["fixtures/golden/vi-legal-qd-1922-2026.md"]  # optional
```

The tag vocabulary is closed and validated by the schema: `headings, lists-nested, tables, tables-merged, footnotes, math, code, images, links, blockquote, task-list, html-inline, emoji, rtl, cjk, mixed-script, nfd-text, long, multi-column, digital-pdf, scan, low-dpi, skew, handwriting, forms, seal, signed, numbered-articles, verse`.

### 4.4 Target mix (60 documents, at least 50 required)

| Group | Count | Composition | Phase |
|---|---|---|---|
| Markdown (generated) | 24 | 12 vi / 10 en / 2 mixed. Cover every GFM feature, one feature per small file plus 3 "kitchen sink" files. Include an **NFD-encoded Vietnamese** file (macOS paste case), a vi table with tone-dense headers, an RTL snippet, a CJK snippet, emoji, and a long file (>200 headings) | **0** (MD→DOCX golden) |
| Markdown from PD texts | 4 | 2 vi (Wikisource PD verse and prose), 2 en (Gutenberg excerpts) | 0 |
| HTML | 6 | 4 generated (vi/en), 2 PD (Wikisource, Federal Register) | 1a |
| DOCX | 8 | 4 generated (python-docx: styles, numbering, tables, footnotes; vi + en), 2 VN government DOC/DOCX, 2 vetted Docling synthetic edge cases | 1a |
| Digital PDF | 8 | 3 VN legal (chinhphu.vn), 2 arXiv CC-BY, 1 GAO, 2 Typst-generated vi/en with known ground truth | 1b |
| Scans | 8 | 2 vi Commons (1897, 1974), 1 vi old government scan, 2 en (LoC, IA), 3 generated degradations of row-4 Typst PDFs at 300/200/150 dpi with exact ground truth (vi-heavy) | 1b |
| Images | 2 | PNG of a vi page and an en receipt-style form (generated) | 1b |

This gives 28 Phase-0-ready Markdown files, which is far more than the single MD→DOCX golden needs. Every later route already has seeds, and generated scans provide exact CER ground truth for the Vietnamese OCR choice in Section 1.

## 5. Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Vietnamese OCR silently loses diacritics if anyone re-enables `auto` or routes `vi` to RapidOCR | High | High | Add a regression fixture: generated vi scan, CER < 0.02 on clean pages, asserting characters in U+1EA0–1EF9 are present |
| Tesseract quality on low-DPI or noisy vi scans (CER 0.2–0.4 in the smoke test) | High | Medium | Upsample to 300 dpi before OCR; confidence-triggered VLM path; bench on fixtures |
| PaddleOCR-VL CPU latency on llama.cpp is unmeasured (estimated tens of seconds per page) | Medium | Medium | Opt-in only; measure in 1b `bench/` |
| Python 3.14 pin conflicts with PaddlePaddle (no cp314) | Certain if Paddle is used | Medium | Transformers backend or a separate 3.13 env for the VLM engine |
| A young model, MonkeyOCRv2 or SenOCR-Vi, changes license or disappears | Medium | Low | Pin by commit hash and SHA-256 in the pack manifest; keep PaddleOCR-VL as the baseline |
| VietOCR weights lack an explicit license | — | — | Do not ship them unless clarified with the author |
| Embedded commercial fonts (Times New Roman subsets) inside government PDFs | Low | Low | Redistributing a document that embeds a font subset is normal practice; note it in the manifest |
| MDPBench scores are a parsing composite, not OCR CER, and the private split is unknown | — | Medium | Treat them as a ranking signal only; decide on our own vi CER bench |

## 6. Limitations

- No VLM, VietOCR or EasyOCR was run locally because torch and llama.cpp are not installed. Their quality claims rest on MDPBench, which is third-party but run by HUST and the maintainers of MultimodalOCR, and on vendor cards.
- No published head-to-head Vietnamese CER benchmark covers Tesseract against PP-OCR, VietOCR and the VLMs. The survey [arXiv 2506.05061](https://arxiv.org/abs/2506.05061) confirms that the gap exists.
- Legal points (Art. 15, BY-SA adaptation, US §105) are a technical reading, not legal advice.

## Unresolved questions

1. Should Tesseract plus `vie`/`eng` move into the `docling` pack (recommended), or should `ocr-extra` be auto-installed for vi users?
2. Should the engine protocol permit loopback to a child `llama-server`, or must the VLM engine use an in-process binding?
3. Does the VLM engine get its own Python 3.13 env (PaddlePaddle), or use the Transformers backend on 3.14?
4. Is fine-tuning a PP-OCRv6 recognizer for Vietnamese (Apache-2.0, about 70 extra classes) in scope for a later phase? It would unify the OCR stack on RapidOCR.
5. Is a CC-BY-SA fixtures subfolder ever acceptable, or is BY-SA permanently excluded?

Status: DONE_WITH_CONCERNS
Summary: The recommended stack is Tesseract 5.5.3 (tessdata_best vie+eng) for Vietnamese, RapidOCR/PP-OCRv6 for other languages, and an opt-in PaddleOCR-VL-1.6 via llama.cpp. Fixtures should be a 60-document, mostly generated set plus Art. 15 Vietnamese legal texts, PD scans and CC-BY/US-government English sources, all tracked in a TOML manifest.
Concerns/Blockers: ARCHITECTURE.md's current RapidOCR-only OCR path cannot output Vietnamese tone letters (verified by dictionary inspection and a local run). VLM CPU latency and Python 3.14 compatibility for PaddleOCR-VL are unmeasured.
