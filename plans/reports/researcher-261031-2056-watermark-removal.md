# Watermark removal for PDFs and document images: open-source options and licence verdicts

Date: 2026-10-31. Read-only research. No repository files were changed and no git commands were run.

## Summary

1. No mature open-source tool removes arbitrary third-party watermarks from PDFs. The only shipped removal routine found (pdfcpu) removes only watermarks that pdfcpu itself inserted, identified by its own marker string.
2. For Level 1 (watermarks stored as PDF objects), build detection on lopdf (MIT, Rust) and use pdfium (BSD-style) for before and after rendering. Both fit the Rust core and both can ship.
3. For Level 2 (watermarks burned into raster), the most licence-clean pipeline is Florence-2 (MIT) for open-vocabulary detection, followed by LaMa (Apache-2.0 code) for inpainting, with OpenCV Telea or NS (Apache-2.0) as the classical fallback. Run all of them as separate engine-pack processes.
4. The LaMa weights are the main open item. The upstream LaMa repository states no weight licence, the big-lama.pt file comes from a 2021 mirror, and the training data (Places365-Challenge) has terms I could not verify.
5. Exclude MAT (CC BY-NC 4.0), Stable Diffusion and SDXL weights (OpenRAIL), FLUX.1-Fill-dev (non-commercial per the tag), SAM 3 weights (custom gated licence), and any Stirling-PDF proprietary directory. IOPaint is archived, so do not build on it.

## 1. Level 1: watermarks stored as PDF objects

### 1.1 What a Level 1 watermark looks like

Level 1 watermarks are structures inside the PDF that can be found and removed without touching pixels. The main forms are:

- Optional content groups (OCGs) with a name such as "Watermark", toggled through /OCProperties.
- Annotations with /Subtype /Watermark. This is a standard annotation subtype but it is rarely used in practice.
- Marked content tagged as an artifact, for example `/Artifact <</Subtype /Watermark /Type /Pagination>> BDC ... EMC`. The artifact subtypes come from the spec. A secondary search result lists Pagination subtypes as Header, Footer and Watermark, and Layout and Background as separate types (unverified against ISO 32000-2 itself; the spec was not retrievable in this run).
- Repeated XObjects or text drawn on every page. This is a heuristic signal, not a PDF structure.

Most real-world watermarks are not any of these. Commercial watermarks are usually plain content-stream drawing operators or form XObjects that are indistinguishable from page content except by repetition. That is the main reason Level 1 coverage will be partial.

### 1.2 Candidate libraries and tools

| Project | Role | Licence (code) | Stars | Latest release | Last push | Watermark code found? |
|---|---|---|---|---|---|---|
| [qpdf/qpdf](https://github.com/qpdf/qpdf) | Structural PDF transformer. Has a JSON mode (manual/json.rst). | Apache-2.0 | 5,540 | v12.4.2, 2026-09-27 | 2026-09-26 | No (tree search: 0 matches) |
| [pikepdf/pikepdf](https://github.com/pikepdf/pikepdf) | Python binding to qpdf | MPL-2.0 | 2,819 | not checked | 2026-10-25 | No (0 matches) |
| [J-F-Liu/lopdf](https://github.com/J-F-Liu/lopdf) | Rust PDF manipulation library | MIT | 2,289 | v0.46.0, 2026-10-21 | 2026-10-31 | No (0 matches) |
| [pdfcpu/pdfcpu](https://github.com/pdfcpu/pdfcpu) | Go PDF CLI and library | Apache-2.0 | 8,891 | v0.16.4, 2026-10-27 | 2026-10-28 | Yes, but only its own marks (see 1.3) |
| [mozilla/pdf.js](https://github.com/mozilla/pdf.js) | Reader and renderer. No removal API. | Apache-2.0 | 54,068 | v6.4.299, 2026-10-03 | 2026-10-31 | No |
| [apache/pdfbox](https://github.com/apache/pdfbox) | Java PDF library (GitHub mirror) | Apache-2.0 | 3,138 | no GitHub release object | 2026-10-31 | One file path matches "watermark"; not inspected (unverified) |
| [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) | Prebuilt PDFium binaries | MIT (repository licence) | 1,505 | chromium/8125, 2026-10-26 | 2026-10-26 | No |
| Google PDFium (upstream) | Renderer and editor engine | BSD-style (LICENSE text read) | n/a | n/a | n/a | No |
| [pypdfium2-team/pypdfium2](https://github.com/pypdfium2-team/pypdfium2) | Python binding to PDFium | Multiple files: LICENSES/ holds Apache-2.0, BSD-3-Clause, MIT, MPL-2.0, LGPL-3.0-or-later, CC-BY-4.0. GitHub reports no single licence. | 851 | 5.14.0, 2026-10-04 | 2026-10-31 | No |
| [Stirling-Tools/Stirling-PDF](https://github.com/Stirling-Tools/Stirling-PDF) | Full PDF web app | NOASSERTION (MIT core, proprietary directories) | 95,460 | v3.2.2, 2026-10-24 | 2026-10-31 | No remove-watermark feature (see 1.4) |

pdfium exposes the primitives needed for object-level removal. The public headers list `FPDFPage_RemoveObject`, `FPDFPage_GetAnnotCount` and `FPDFPage_RemoveAnnot`, and `FPDFPage_CountObjects` (verified in public/fpdf_edit.h and public/fpdf_annot.h). pdfium does not implement any watermark heuristic; the caller must write it.

lopdf and qpdf expose the same objects (catalog, OCProperties, page dictionaries, Annots, content streams). Neither ships a watermark detector. Detection logic is therefore ours to write in all cases.

### 1.3 pdfcpu: the only shipped removal routine found

pdfcpu has `watermark remove` and `stamp remove` commands (verified from the pdfcpu.io usage page). The implementation in `pkg/pdfcpu/stamp.go` matches a fixed string written by pdfcpu itself: `/Artifact <</Subtype /Watermark /Type /Pagination >>BDC`. The removal path (`removeArtifacts`) searches content streams for exactly that prefix. It also removes the resources pdfcpu added for that watermark: extended graphics states, forms and OCGs.

Consequences:

- It removes watermarks that pdfcpu added, not third-party watermarks.
- It is a good reference for marker-based detection and for the artifact-wrapping convention.
- It is an Apache-2.0 Go program, so it can be used as a separate process without licence friction. Its value is as a template, not as a remover of real-world watermarks.

### 1.4 Stirling-PDF

The repository tree contains `WatermarkController.java` (add watermark), `RemoveImagesController.java` (remove images) and front-end assets for "remove-annotations" and "remove-image". It has no remove-watermark controller or tool (verified by tree search). Its licence is NOASSERTION because the root is MIT but directories such as `app/proprietary`, `app/saas` and `engine/` are proprietary (see [[stirling-pdf-competitive-facts]] from earlier research). Do not copy code from it.

### 1.5 Existing GitHub projects

A search for "remove watermark pdf" returned only small, unmaintained scripts (0 to 3 stars, last pushes between 2021 and 2026). None is credible as a dependency. The search also found no maintained open-source tool for OCG-based watermark removal (search performed; results were low quality, so absence is probable but not proven).

### 1.6 Detection heuristics for Level 1 (proposed, not taken from a library)

Each heuristic is a design proposal for the Rust core. None has been validated on a corpus.

1. Enumerate `/OCProperties` and flag OCGs whose names match a watermark lexicon (watermark, draft, confidential, sample, copy, trial). Turning an OCG off is reversible and should be the default action.
2. Flag annotations with `/Subtype /Watermark`, and annotations whose appearance streams repeat on every page.
3. Flag artifact-tagged blocks with `/Subtype /Watermark`, as pdfcpu does.
4. Flag XObjects or text runs that appear on at least 80 percent of pages with the same transform and bounding box. This catches most "watermark on every page" cases. The threshold is an unverified guess and needs a corpus to tune.
5. Keep a preview: render before and after with pdfium, show the diff, and allow undo. This also limits false positives, which would otherwise delete real content.

## 2. Level 2: watermarks burned into raster images

### 2.1 Candidate pipeline components

Each row lists code licence, weight licence, verification status, and the known quality and speed data.

| Component | Code licence | Weight licence | Activity | Quality and speed notes |
|---|---|---|---|---|
| [advimman/lama](https://github.com/advimman/lama) (LaMa, big-lama) | Apache-2.0 (LICENSE file; repo 10,351 stars) | Not stated in the LaMa README (no licence text found). The HF mirror [smartywu/big-lama](https://huggingface.co/smartywu/big-lama) carries an apache-2.0 tag, but that is the uploader's declaration (unverified upstream). | Last push 2025-02-05. No releases. | Strong on large masks and smooth backgrounds. Training data is Places365-Challenge, per the README. One anecdote reports about 25 s per HD image on a Xeon CPU (unverified source, variable mask). |
| [Sanster/IOPaint](https://github.com/Sanster/IOPaint) (lama-cleaner successor) | Apache-2.0 | Uses LaMa and diffusion models | **Archived**. Last push 2025-04-29, last release iopaint-1.5.3 (2024-11-23). | Usable as a reference. Do not take it as a dependency. |
| [advimman/lama](https://github.com/advimman/lama) via [D-Ogi/WatermarkRemover-AI](https://github.com/D-Ogi/WatermarkRemover-AI) | MIT (repo, 2,086 stars, latest v0.68.0 on 2026-09-16, last push 2026-10-30). Its THIRD_PARTY_NOTICES reproduces Apache-2.0 for code adapted from IOPaint. | Downloads big-lama.pt (205,669,692 bytes, SHA-256 pinned) from the 2021 [Sanster/models add_big_lama](https://github.com/Sanster/models/releases/tag/add_big_lama) release. Its own notice says "Code licensing does not replace model/data terms." | Active | Full Level 2 pipeline (see 2.2). The README contains no legal or copyright framing. |
| [microsoft Florence-2](https://huggingface.co/microsoft/Florence-2-large) (large) | Not on GitHub: the microsoft/Florence-2 repository returned 404 in this run. Inference code ships in transformers. | MIT (HF card) | HF last modified 2025-08-04; about 391k downloads | Open-vocabulary detection of "watermark" or specific logo text. Quality on watermarks: unverified. |
| [mlx-community/Florence-2-large-ft-bf16](https://huggingface.co/mlx-community/Florence-2-large-ft-bf16) | MLX conversion | MIT (HF tag) | last modified 2024-11-21 | Apple Silicon MLX path for detection. Speed unverified. |
| Florence-2 CoreML (e.g. [mlboydaisuke/Florence-2-base-CoreML](https://huggingface.co/mlboydaisuke/Florence-2-base-CoreML)) | Converted | Inherits MIT, licence tag not checked | not checked | Notes warn of FP16 overflow and Apple Neural Engine buffer limits. |
| [facebookresearch/segment-anything](https://github.com/facebookresearch/segment-anything) (SAM 1) | Apache-2.0 | Apache-2.0 (vit-base HF tag) | Last push 2024-09-18; 55,084 stars | Segmentation to refine masks. Useful only if the detection box is too coarse. |
| [facebookresearch/sam2](https://github.com/facebookresearch/sam2) (SAM 2) | Apache-2.0 | sam2.1-hiera-large: apache-2.0 (HF tag) | Last push 2026-05-30; 20,082 stars | Same role as SAM 1. |
| SAM 3 ([facebook/sam3](https://huggingface.co/facebook/sam3)) | Not checked | HF tag "other"; manual gated access and custom terms | last modified 2025-11-20 | **Exclude.** Custom licence, not OSI. |
| [opencv/opencv](https://github.com/opencv/opencv) `cv::inpaint` | Apache-2.0 (repo 91,390 stars; latest 5.0.0 on 2026-06-06) | Not applicable | Very active | Telea and NS modes confirmed in `modules/photo/src/inpaint.cpp`. Classical; fine for thin scratches and small marks, poor on large textured regions. The docs site returned 403, so the description is from source, not docs. |
| [PaddlePaddle/PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR) | Apache-2.0 (92,028 stars; v3.7.0 on 2026-06-11) | Model weights not checked (unverified) | Active | Text detection alternative to Florence-2 if OCR-based masking is needed. |
| [Picsart-AI-Research/MI-GAN](https://github.com/Picsart-AI-Research/MI-GAN) | MIT (705 stars; last push 2026-09-14) | Not checked. MLX ports exist (mlx-community/MI-GAN-256-places2-fp16), and the name indicates Places2 training. | Active | Fast light-weight inpainting alternative. Places2 training data carries the same unverified terms question as LaMa. |
| [fenglinglwb/MAT](https://github.com/fenglinglwb/MAT) | **CC BY-NC 4.0** (LICENSE file read) | Non-commercial | Last push 2024-07-02; 983 stars | **Exclude.** Non-commercial licence. |
| [zuruoke/watermark-removal](https://github.com/zuruoke/watermark-removal) | MIT (5,199 stars; latest v1.0.0 on 2026-06-05; last push 2026-10-24) | Method and weights not checked | Active | General-image inpainting for watermarks. Method and quality not verified. |

### 2.2 The WatermarkRemover-AI pipeline (reference design)

WatermarkRemover-AI (MIT) runs three steps:

1. Florence-2 `<OPEN_VOCABULARY_DETECTION>` with prompts such as "watermark" or a brand name returns bounding boxes.
2. A mask is built from each box, in either "box" mode or "stroke" mode. Stroke mode thresholds the difference image and dilates the strokes.
3. LaMa (big-lama, via an IOPaint-derived adapter) inpaints the masked region. Mask and crop logic are adapted from IOPaint under Apache-2.0, with notices included in the repository.

The design is sound and reusable. Vendoring the whole repository is not advisable, because its weights come from a 2021 external release and its model sources are pinned to a community mirror of Florence-2 (florence-community/Florence-2-large, which carries an MIT tag).

### 2.3 Diffusion inpainting

| Model | Licence | Verdict |
|---|---|---|
| stabilityai/stable-diffusion-xl-base-1.0 | openrail++ (HF tag) | Do not ship. OpenRAIL terms restrict use. |
| black-forest-labs/FLUX.1-Fill-dev | "other" (HF tag). This is the FLUX [dev] non-commercial terms, per the public model page (text not verified in this run). | Do not ship for a commercial product. |
| black-forest-labs/FLUX.1-schnell | apache-2.0 (HF tag) | Apache-2.0 weights, but it is not an inpainting model. Using it for fill requires separate work (unverified). |
| stabilityai/stable-diffusion-2-inpainting | No licence tag returned by the API (unverified). The Stable Diffusion family generally uses OpenRAIL-M. | Do not ship until verified. |

Diffusion inpainting produces better textures than LaMa on large holes but is slow on CPU and has the most restrictive weight licences. It is out of scope for a shipped default.

### 2.4 Document-specific de-watermarking

| Work | Code and weights | Verdict |
|---|---|---|
| TextLogo dataset and FE-WRNet (MDPI, Nov 2025) | Code and dataset licences not found (unverified). | Do not ship until code and licence are confirmed. |
| [foduucom/Watermark_Removal](https://huggingface.co/foduucom/Watermark_Removal) (HF) | apache-2.0 tag, last modified 2025-01-18. Method, data and quality not checked. | Candidate, conditional. Needs an evaluation on our own documents. |
| [OrdinaryDev83/dnn-watermark](https://github.com/OrdinaryDev83/dnn-watermark) (visible watermark generator) | No licence; 8 stars; last push 2024-02-05. | Do not ship. Possibly usable as a data generator only after a licence review. |
| [adobe/trustmark](https://github.com/adobe/trustmark) | GitHub reports NOASSERTION. The LICENSE file has an Adobe copyright notice followed by MIT text. | Not a fit. TrustMark is an invisible robust-watermark system, not a visible watermark remover. Legal status of the notice needs a check if it is ever considered. |

No peer-reviewed or benchmarked open model specific to visible document watermarks was found with a permissive licence and public code. This gap is important for the decision below.

### 2.5 Apple Silicon

- LaMa has Core ML ports: [mallman/CoreMLaMa](https://github.com/mallman/CoreMLaMa) (Apache-2.0, 136 stars, last push 2024-11-19), [Jia-Liu/big-lama-coreml](https://huggingface.co/Jia-Liu/big-lama-coreml) (apache-2.0 tag, 800x800 inputs), and [mlboydaisuke/LaMa-CoreML](https://huggingface.co/mlboydaisuke/LaMa-CoreML) (apache-2.0 tag, iOS 17 target). Their weights carry the same provenance issue as big-lama.pt.
- No MLX port of LaMa was found. MI-GAN has MLX ports.
- Florence-2 has Core ML conversions and an MLX conversion (see table 2.1).
- No measured latency on M-series hardware was found for any of these. Performance on Apple Silicon is therefore unverified and must be benchmarked.

### 2.6 Speed and quality data (what was found)

- LaMa-Dilated, a quantized Qualcomm build, runs in about 9.9 s on a mobile CPU at 512x512 (Qualcomm model card, mobile SoC, not comparable to desktop).
- The same family runs in about 34 to 120 ms on mobile NPUs (Qualcomm model card).
- A single anecdote reports about 25 s for an HD image on a Xeon CPU with a variable mask (unverified, not comparable).
- No independent quality benchmark for watermark removal exists in this research. Quality ranking between LaMa, MAT, MI-GAN and diffusion is therefore unverified.

## 3. Legal and ethical positioning

- **US statute.** 17 U.S.C. 1202(b) prohibits intentionally removing or altering copyright management information (CMI) when the person knows or has reasonable grounds to know that the act will induce, enable, facilitate or conceal infringement. Whether a visible watermark is CMI is unsettled; the statute text was read through a summarising fetch, not verbatim (unverified wording). Source: [17 U.S.C. 1202 on LII](https://www.law.cornell.edu/uscode/text/17/1202).
- **Vietnam.** Article 19 of the Law on Intellectual Property (as amended in 2022) gives authors an indefinite moral right to protect the integrity of their work. Removing a watermark from a third-party work can touch this right. Sources were secondary (lawnet.vn, vietanlaw, WIPO text). Decree 17/2023/ND-CP sets detailed rules. The 2025 amendments were not checked. Because AriadShift's operator is in Vietnam, legal review is needed here.
- **Vendor framing.** Adobe's terms are reported (secondary blog, not verified on Adobe's own page, which returned 403) to prohibit using its tools to remove third-party marks, while allowing removal of marks the user added with Acrobat. Stirling-PDF ships "add watermark" and "remove annotations" but no "remove watermark", which is a deliberate product choice visible in its tree.
- **Practical framing for a product.** Position the feature as removing marks the user added, or marks on documents they own, with a preview, undo, an audit record, and a warning when a mark looks third-party. This matches how the reputable tools describe the feature and reduces the DMCA and moral-rights exposure. Any stock-photo or licensed-content use should be refused.

## 4. Adoption and abandonment risk

- LaMa: 10k stars and Apache-2.0, but the last push was 2025-02-05. Treat it as stable code with no maintenance. Pin the commit.
- IOPaint: archived. Do not depend on it.
- MAT: last push 2024-07-02 and non-commercial. Excluded for licence reasons regardless of maintenance.
- Florence-2: Microsoft model, MIT, widely used (about 391k downloads on HF). Low abandonment risk for weights; code lives in transformers.
- SAM 2: active, Apache-2.0, low risk.
- OpenCV: very active, Apache-2.0, very low risk.
- pdfcpu and lopdf: active, permissive, moderate risk (small maintainer teams, but both are recent).
- Stirling-PDF: active but licence-split. Use as reference only.

## 5. Recommendation

Ranked choices, best first, per level.

| Level | Recommended stack | Licence verdict | Main risk |
|---|---|---|---|
| 1. Objects (OCG, annots, artifacts, repeated XObjects) | Rust core: lopdf (MIT) for parsing and removal, our own heuristics (section 1.6), and pdfium (BSD-style) for before and after rendering. Use pdfcpu (Apache-2.0) as a separate-process reference only if needed. | **Ship** (lopdf, pdfium). Ship-as-separate-process for pdfcpu. Do not use pikepdf in a Python pack without a named MPL-2.0 review. | Heuristics will miss watermarks drawn as plain content, and may remove real content. Mitigate with preview, undo and reversible OCG toggling. |
| 2a. Detection of raster watermarks | Florence-2-large (MIT weights, via transformers) in a Python engine pack, with the prompt "watermark" and optional brand prompts. | **Ship-as-separate-process** | CPU speed unmeasured. Open-vocabulary false positives. Pin the HF revision and verify the community-mirror terms. |
| 2b. Inpainting of raster watermarks | LaMa big-lama (Apache-2.0 code) as a separate process. Core ML ports for Apple devices. | **Ship-as-separate-process, conditional.** Ship only after the weight provenance and Places365 training-data terms are verified in writing. | Weight licence not stated upstream. Training data terms unverified. Repo stale since 2025-02. |
| 2c. Classical fallback | OpenCV `cv::inpaint` (Telea or NS), Apache-2.0, as a separate process. | **Ship-as-separate-process** (no licence blocker) | Low quality on large textured regions. |
| 2d. Reference design only | WatermarkRemover-AI pipeline (MIT code, Apache-2.0 adapter). Copy the design, not the repository, and keep its notices if any code is adapted. | **Reference, not dependency** | Weights from a 2021 external release and a community Florence-2 mirror. |
| Excluded | MAT (CC BY-NC 4.0); SDXL and Stable Diffusion inpainting (OpenRAIL); FLUX.1-Fill-dev (non-commercial); SAM 3 weights (custom gated); IOPaint (archived); Stirling-PDF proprietary directories; TextLogo/FE-WRNet until code licence is known. | **Do not ship** | Licence incompatibility with the project rules. |

Decision order: ship Level 1 first, since it needs no model and no weight review. Ship Level 2 only after the LaMa weight and data review is complete, and benchmark Florence-2 and LaMa on our own scanned and photographed documents before choosing the default.

## 6. Limitations

- No code was run. Speed and quality figures are from secondary sources or are absent. Every such claim is marked unverified.
- The LaMa weight licence and Places365 dataset terms were not confirmed. The Places2 website refused connections in this run, and the Places365 GitHub repository states no dataset licence. This is the largest open legal question.
- Adobe help pages and the OpenCV documentation returned 403, so those claims rest on secondary sources or source code.
- The ISO 32000-2 text was not retrieved, so the artifact subtype list is from secondary material.
- The TextLogo and foduucom models were not evaluated on any document corpus.
- No Apple Silicon latency was measured.
- The DMCA and Vietnamese moral-rights summaries are not legal advice and need counsel review before any product claim.

## Unresolved questions

1. Does the LaMa big-lama weight have a licence statement from its authors? The Sanster/models release and the HF mirror are not authoritative.
2. Are the Places365-Challenge image terms non-commercial? This affects LaMa, MI-GAN and any model trained on Places2.
3. Which of the Florence-2 distributions (microsoft, florence-community, mlx-community) should be pinned, and is each one's card licence confirmed?
4. What is the target corpus (scans, photos, or born-digital PDFs)? The Level 2 choice depends on it, and no benchmark has been run.
5. Does the product operate in Vietnam, and does the moral-right analysis need counsel input before launch?
6. Is pdfcpu's marker-based removal acceptable as a model for our Level 1 detector, or should the Rust core implement its own marker set?

Status: DONE_WITH_CONCERNS
Summary: The report at /Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/plans/reports/researcher-261031-2056-watermark-removal.md ranks lopdf and pdfium as shippable for Level 1, and Florence-2 plus OpenCV as shippable separate-process for Level 2, with LaMa conditional on weight and Places365 data review.
Concerns/Blockers: The LaMa weight licence and Places365 dataset terms are unverified, and no benchmark or Apple Silicon measurement was run. Adobe and OpenCV docs returned 403, and the ISO artifact subtypes come from secondary sources.
