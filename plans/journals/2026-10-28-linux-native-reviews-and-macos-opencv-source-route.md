---
title: Linux native reviews and macOS OpenCV source route
date: 2026-10-28
summary: Completed named Linux reviews and implemented a video-disabled macOS OpenCV source route; native macOS validation remains pending.
---

# Linux native reviews and macOS OpenCV source route

The locked macOS OpenCV wheels have positive GPL binary declarations. RapidOCR and TableFormer hard-import cv2, so removal would lose shipped features. Fourteen compatible stable wheels all fail. The approved next route is native macOS source compilation with video I/O disabled, complete wheel repair/audit, and hashes from actual outputs.

Linux review names all 805 native files across both mandatory input sets. Full upstream notices fill wheel omissions; weak-copyleft objects require ELF hashes and exact source pointers. GNU source evidence records only the ABI baseline proven by version tags and carries the three-year source offer.

The macOS source route is 3bfe1a49bce94896cc53dda6c56af373c29df297. Final notice correction 8b84f0a removes exactly the previously reviewed inapplicable IPP section from the copied OpenCV sidecar. Its new regression fails beforehand and all 71 integrity tests pass afterwards; all other notice sections remain byte-identical. The pack gate is running. The first diagnostic helper expected one extra named extension but the source adds two; comparing all fixture paths proves 54 named files. No product rule was weakened. Native macOS compilation, real output hashes and its full inventory remain pending on a macOS host. No push or workflow dispatch occurred. AgentWiki publish skipped.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.

A direct audit also caught the glibc notice's stored digest referring to a
form-feed-normalized variant. The retained file's text is unchanged; its digest
now matches the committed bytes. A new regression checks all 14 upstream notice
sidecars. The active final gate uses the amended source head, with prior
cancelled runs preserved and no partial census reused.

The exact-source pack gate finished ALL GREEN at 8b84f0ab8282e82e165e41144dd8a70b229c2617. Both mandatory
Linux inventories have zero findings; 205 focused and 299 full Docling tests,
75 bench tests, all 135 reproducible fixtures and lock/audit/lint checks pass.
The canonical report and report-only successor preserve this source result.
Native macOS validation/output pinning remains the explicit outstanding item.
